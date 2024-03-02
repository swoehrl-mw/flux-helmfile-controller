use super::util::map_finalizer_error;
use crate::crd::{Helmfile, SourceRefKind};
use crate::error::{Error, Result};
use crate::extcrds::gitrepositories::GitRepository;
use crate::flux::artifact::FluxSourceAdapterImpl;
use crate::helmfile::HelmfileAdapterImpl;
use crate::k8sclient::K8sClientImpl;
use crate::metrics::{
    l, NUM_CLEANUPS_FAILED, NUM_CLEANUPS_STARTED, NUM_RECONCILES_FAILED, NUM_RECONCILES_PENDING,
    NUM_RECONCILES_STARTED,
};
use crate::reconciler::{cleanup_helmfile, reconcile_helmfile, ReconcileResult};
use crate::store::{ControllerStoreRef, NamespacedName};
use crate::util::NS;
use futures::StreamExt;
use kube::runtime::finalizer::Event as Finalizer;
use kube::runtime::reflector::ObjectRef;
use kube::runtime::{controller, finalizer, reflector, WatchStreamExt};
use kube::runtime::{
    controller::{Action, Controller},
    watcher,
};
use kube::{Api, Client, Resource, ResourceExt};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

static FINALIZER: &str = "flux.maibornwolff.de";
static REQUEUE_ERROR_SECONDS: u64 = 30;
static REQUEUE_DEFAULT_SECONDS: u64 = 300;
static REQUEUE_PENDING_SECONDS: u64 = 10;

pub async fn run(client: Client, store: ControllerStoreRef) {
    let context = Arc::new(Context {
        client: client.clone(),
        store: store.clone(),
    });
    let api = Api::<Helmfile>::all(client.clone());
    let api_repo = Api::<GitRepository>::all(client.clone());

    let (reader, writer) = reflector::store();
    let changed_helmfiles = watcher(api, watcher::Config::default())
        .reflect(writer)
        .default_backoff()
        .touched_objects()
        .predicate_filter(predicate_filter);

    Controller::for_stream(changed_helmfiles, reader)
        .with_config(controller::Config::default().concurrency(2))
        .watches(api_repo, watcher::Config::default(), move |repo| {
            tokio::task::block_in_place(|| map_repo(repo, store.clone()))
        })
        .shutdown_on_signal()
        .run(reconcile_with_finalizer, error_policy, context)
        .for_each(|res| async move {
            match res {
                Ok(_) => (),
                Err(e) => tracing::warn!("reconcile failed: {:?}", e),
            }
        })
        .await;
}

fn predicate_filter(obj: &Helmfile) -> Option<u64> {
    let mut hasher = DefaultHasher::new();
    if let Some(finalizers) = obj.meta().finalizers.as_ref() {
        finalizers.hash(&mut hasher);
    }
    if let Some(labels) = obj.meta().labels.as_ref() {
        labels.hash(&mut hasher);
    }
    if let Some(generation) = obj.meta().generation.as_ref() {
        generation.hash(&mut hasher);
    }
    if let Some(uid) = obj.meta().uid.as_ref() {
        uid.hash(&mut hasher);
    }
    Some(hasher.finish())
}

// Context for the reconciler
#[derive(Clone)]
pub struct Context {
    pub client: Client,
    pub store: ControllerStoreRef,
}

async fn reconcile_with_finalizer(obj: Arc<Helmfile>, ctx: Arc<Context>) -> Result<Action> {
    let ns = obj.namespace().unwrap_or_else(|| NS.to_owned());
    let api: Api<Helmfile> = Api::namespaced(ctx.client.clone(), &ns);

    finalizer(&api, FINALIZER, obj, |event| async {
        match event {
            Finalizer::Apply(obj) => {
                let labels = l(&obj);
                let result = reconcile(obj, ctx.clone()).await;
                if result.is_err() {
                    NUM_RECONCILES_FAILED.get_or_create(&labels).inc();
                }
                result
            }
            Finalizer::Cleanup(obj) => {
                let labels = l(&obj);
                let result = cleanup(obj, ctx.clone()).await;
                if result.is_err() {
                    NUM_CLEANUPS_FAILED.get_or_create(&labels).inc();
                }
                result
            }
        }
    })
    .await
    .map_err(map_finalizer_error)
}

async fn reconcile(obj: Arc<Helmfile>, ctx: Arc<Context>) -> Result<Action> {
    NUM_RECONCILES_STARTED.get_or_create(&l(&obj)).inc();
    // check if source is already available
    let ns = obj.namespace().unwrap_or_else(|| NS.to_owned());
    let source_name = &obj.spec.source_ref.name;
    let source = get_gitrepository(ctx.client.clone(), &ns, source_name).await;
    if let Some(repo) = source {
        let result = reconcile_helmfile(
            K8sClientImpl::new(ctx.client.clone()),
            HelmfileAdapterImpl {},
            FluxSourceAdapterImpl {},
            ctx.store.clone(),
            &obj,
            repo,
        )
        .await?;
        Ok(requeue_action(&obj.spec.interval, &result))
    } else {
        tracing::info!(
            "Could not yet find GitRepository {source_name} in namespace {ns}. Requeuing"
        );
        NUM_RECONCILES_PENDING.get_or_create(&l(&obj)).inc();
        Ok(Action::requeue(Duration::from_secs(
            REQUEUE_PENDING_SECONDS,
        )))
    }
}

async fn cleanup(obj: Arc<Helmfile>, ctx: Arc<Context>) -> Result<Action> {
    NUM_CLEANUPS_STARTED.get_or_create(&l(&obj)).inc();
    // remove from store so it no longer is mapped
    {
        let mut store = ctx.store.write().await;
        store.helmfiles.remove(&(&obj).into());
    }
    if obj
        .spec
        .options
        .as_ref()
        .and_then(|o| o.prune)
        .unwrap_or(false)
    {
        let ns = obj.namespace().unwrap_or_else(|| NS.to_owned());
        let source_name = &obj.spec.source_ref.name;
        // see if gitrepo still exists
        let source = get_gitrepository(ctx.client.clone(), &ns, source_name).await;
        cleanup_helmfile(
            K8sClientImpl::new(ctx.client.clone()),
            HelmfileAdapterImpl {},
            FluxSourceAdapterImpl {},
            ctx.store.clone(),
            &obj,
            source,
        )
        .await?;
    }
    Ok(Action::await_change())
}

fn map_repo(repo: GitRepository, store: ControllerStoreRef) -> Vec<ObjectRef<Helmfile>> {
    let store = store.blocking_read();
    store
        .helmfiles
        .iter()
        .filter_map(|(key, value)| {
            if matches_gitrepository(key, value, &repo) {
                Some(ObjectRef::from_obj(value))
            } else {
                None
            }
        })
        .collect()
}

fn matches_gitrepository(name: &NamespacedName, obj: &Helmfile, repo: &GitRepository) -> bool {
    obj.spec.source_ref.kind == SourceRefKind::GitRepository
        && obj.spec.source_ref.name == repo.name_any()
        && name.namespace == repo.namespace().unwrap_or_else(|| NS.to_owned())
}

fn error_policy(_obj: Arc<Helmfile>, _error: &Error, _ctx: Arc<Context>) -> Action {
    Action::requeue(Duration::from_secs(REQUEUE_ERROR_SECONDS))
}

async fn get_gitrepository(client: Client, namespace: &str, name: &str) -> Option<GitRepository> {
    let api = Api::<GitRepository>::namespaced(client, namespace);
    if let Ok(result) = api.get(name).await {
        Some(result)
    } else {
        None
    }
}

fn requeue_action(interval: &Option<String>, result: &ReconcileResult) -> Action {
    match result {
        ReconcileResult::Success => Action::requeue(if let Some(interval) = interval {
            parse_duration::parse(interval)
                .unwrap_or_else(|_| Duration::from_secs(REQUEUE_DEFAULT_SECONDS))
        } else {
            Duration::from_secs(REQUEUE_DEFAULT_SECONDS)
        }),
        ReconcileResult::Failed(_) => Action::requeue(Duration::from_secs(REQUEUE_ERROR_SECONDS)),
        ReconcileResult::FailedRetriesExhausted(_) => Action::await_change(),
        ReconcileResult::Pending(_) => {
            Action::requeue(Duration::from_secs(REQUEUE_PENDING_SECONDS))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_requeue_interval() {
        assert_eq!(
            requeue_action(&Some("2m30s".to_string()), &ReconcileResult::Success),
            Action::requeue(Duration::from_secs(2 * 60 + 30))
        );
        assert_eq!(
            requeue_action(&None, &ReconcileResult::Success),
            Action::requeue(Duration::from_secs(REQUEUE_DEFAULT_SECONDS))
        );
        assert_eq!(
            requeue_action(&None, &ReconcileResult::Failed("foobar".to_string())),
            Action::requeue(Duration::from_secs(REQUEUE_ERROR_SECONDS))
        );
        assert_eq!(
            requeue_action(
                &None,
                &ReconcileResult::FailedRetriesExhausted("foobar".to_string())
            ),
            Action::await_change()
        );
        assert_eq!(
            requeue_action(
                &Some("10s".to_string()),
                &ReconcileResult::Failed("foobar".to_string())
            ),
            Action::requeue(Duration::from_secs(REQUEUE_ERROR_SECONDS))
        );
    }
}
