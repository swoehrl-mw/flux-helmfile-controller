use crate::crd::{DecryptionProviderKind, DeploymentResult, DeploymentStatus};
use crate::error::{Error, Result};
use crate::helmfile::{HelmfileAdapter, HelmfileResult};
use crate::k8sclient::K8sClient;
use crate::metrics::{l, NUM_RECONCILES_PENDING};
use crate::store::{ControllerStoreRef, HelmfileState};
use crate::util::{timestamp_now, NS};
use crate::{
    crd::Helmfile, extcrds::gitrepositories::GitRepository, flux::artifact::FluxSourceAdapter,
    helmfile,
};
use kube::api::Patch;
use kube::{Resource, ResourceExt};
use serde_json::json;
use std::io::Write;
use tempfile::NamedTempFile;

const SECRETS_KEY_AGE: &str = "age.agekey";
const SECRETS_ENV_KEY_AGE: &str = "SOPS_AGE_KEY_FILE";
const ACTION_LABEL: &str = "controller/action";
const ACTION_LABEL_SYNC: &str = "sync";

pub enum ReconcileResult {
    Success,
    Failed(String),
    FailedRetriesExhausted(String),
    Pending(String),
}

pub async fn reconcile_helmfile(
    client: impl K8sClient,
    helmfile_adapter: impl HelmfileAdapter,
    flux_adapter: impl FluxSourceAdapter,
    store: ControllerStoreRef,
    obj: &Helmfile,
    repo: GitRepository,
) -> Result<ReconcileResult> {
    let name = obj.name_any();
    let ns = obj.namespace().unwrap_or_else(|| NS.to_owned());
    tracing::info!("Starting reconcile of helmfile {name} in namespace {ns}");

    let existing_state = {
        let mut store = store.write().await;
        store.state.remove(&obj.into())
    };
    let num_retries = existing_state.as_ref().and_then(|s| s.num_retries);

    // Prepare any needed secrets
    let (key_file, env) = if let Some(decryption) = obj.spec.decryption.as_ref() {
        let result = match decryption.provider {
            DecryptionProviderKind::SopsAge => {
                prepare_age_key(&client, &ns, &decryption.secret_ref.name).await?
            }
        };
        (Some(result.0), Some(result.1))
    } else {
        (None, None)
    };

    // Retrieve artifact information
    let Some(artifact) = repo.status.and_then(|el| el.artifact) else {
        let source_name = &obj.spec.source_ref.name;
        let reason = format!(
            "Could not yet find artifact for GitRepository {source_name} in namespace {ns}"
        );
        NUM_RECONCILES_PENDING.get_or_create(&l(obj)).inc();
        tracing::info!("{reason}. Requeuing");
        return Ok(ReconcileResult::Pending(reason));
    };
    // download and extract artifact
    let (location, digest) = flux_adapter
        .fetch_and_extract_artifact(existing_state, &artifact)
        .await?;

    let action = action(obj);
    // Use sync on first run
    let mode = match (action, obj.status.is_some()) {
        (Action::None, true) => helmfile::Mode::Apply,
        (Action::None, false) => helmfile::Mode::Sync,
        (Action::Sync, _) => helmfile::Mode::Sync,
    };

    // run helmfile
    let manifest_dir = if let Some(path) = obj.spec.path.as_ref() {
        location.path().join(path)
    } else {
        location.path().to_path_buf()
    };
    let result = helmfile_adapter.apply(mode, &manifest_dir, obj, env).await;
    tracing::info!("Got result from helmfile: {:?}", result);
    let num_retries = update_retries(num_retries, &result);
    let exhausted = if let Some(retry) = num_retries {
        if let Some(allowed_retries) = obj.spec.options.as_ref().and_then(|o| o.retries) {
            if allowed_retries > 0 {
                retry >= allowed_retries
            } else {
                allowed_retries == 0
            }
        } else {
            false
        }
    } else {
        false
    };

    // store temp dir in store
    {
        let mut store = store.write().await;
        store.state.insert(
            obj.into(),
            HelmfileState {
                current_digest: digest,
                location,
                num_retries,
            },
        );
    }

    // update status
    update_status(&client, &name, &ns, &result).await?;

    if action != Action::None {
        // delete action label
        remove_action_label(&client, obj, &name, &ns).await?;
    }

    // place obj in store for reference from watcher
    {
        let mut store = store.write().await;
        store.helmfiles.insert(obj.into(), (*obj).clone());
    }

    drop(key_file);

    tracing::info!("Finished reconcile of helmfile {name} in namespace {ns}");
    Ok(map_result(result, exhausted))
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Action {
    None,
    Sync,
}

fn action(obj: &Helmfile) -> Action {
    if let Some(label) = obj.meta().labels.as_ref().and_then(|m| m.get(ACTION_LABEL)) {
        if label == ACTION_LABEL_SYNC {
            return Action::Sync;
        }
    }
    Action::None
}

fn update_retries(num_retries: Option<i32>, result: &HelmfileResult) -> Option<i32> {
    match result {
        HelmfileResult::Applied | HelmfileResult::NoChange => None,
        HelmfileResult::Failed(_) => Some(num_retries.unwrap_or(0) + 1),
    }
}

pub async fn cleanup_helmfile(
    client: impl K8sClient,
    helmfile_adapter: impl HelmfileAdapter,
    flux_adapter: impl FluxSourceAdapter,
    store: ControllerStoreRef,
    obj: &Helmfile,
    repo: Option<GitRepository>,
) -> Result<ReconcileResult> {
    let name = obj.name_any();
    let ns = obj.namespace().unwrap_or_else(|| NS.to_owned());
    tracing::info!("Starting cleanup of helmfile {name} in namespace {ns}");

    // Prepare any needed secrets
    let (key_file, env) = if let Some(decryption) = obj.spec.decryption.as_ref() {
        let result = match decryption.provider {
            DecryptionProviderKind::SopsAge => {
                prepare_age_key(&client, &ns, &decryption.secret_ref.name).await?
            }
        };
        (Some(result.0), Some(result.1))
    } else {
        (None, None)
    };

    let existing_state = {
        let mut store = store.write().await;
        store.state.remove(&obj.into())
    };

    // if gitrepo not exists see if last version is still in store
    let location = if let Some(artifact) = repo.and_then(|r| r.status).and_then(|el| el.artifact) {
        flux_adapter
            .fetch_and_extract_artifact(existing_state, &artifact)
            .await?
            .0
    } else if let Some(state) = existing_state {
        state.location
    } else {
        // if neither in store nor gitrepo exists, just quietly end
        tracing::warn!(
            "Could not cleanup helmfile {} because source is missing.",
            name
        );
        return Ok(ReconcileResult::Success);
    };

    // Run destroy
    let manifest_dir = if let Some(path) = obj.spec.path.as_ref() {
        location.path().join(path)
    } else {
        location.path().to_path_buf()
    };
    let result = helmfile_adapter.destroy(&manifest_dir, obj, env).await;
    // TBD: Handle failed destroy and keep location in store
    tracing::info!("Finished cleanup of helmfile {name} in namespace {ns} with result: {result:?}");

    drop(key_file);
    Ok(ReconcileResult::Success)
}

async fn update_status(
    client: &impl K8sClient,
    name: &str,
    namespace: &str,
    helmfile_status: &HelmfileResult,
) -> Result<()> {
    let (status, reason) = match helmfile_status {
        HelmfileResult::NoChange => {
            return Ok(());
        }
        HelmfileResult::Applied => (DeploymentResult::Successful, None),
        HelmfileResult::Failed(reason) => (DeploymentResult::Failed, Some(reason.clone())),
    };
    let status = DeploymentStatus {
        status,
        reason,
        last_update: timestamp_now(),
    };
    let new_status = Patch::Apply(json!({
        "apiVersion": Helmfile::api_version(&()),
        "kind": Helmfile::kind(&()),
        "status": status
    }));
    client
        .patch_helmfile_status(namespace, name, &new_status)
        .await?;
    Ok(())
}

async fn remove_action_label(
    client: &impl K8sClient,
    obj: &Helmfile,
    name: &str,
    namespace: &str,
) -> Result<()> {
    if obj.meta().labels.is_some() {
        let patch = Patch::Merge(json!({
            "apiVersion": Helmfile::api_version(&()),
            "kind": Helmfile::kind(&()),
            "metadata": {
                "labels": {
                    ACTION_LABEL: null,
                }
            }
        }));
        client
            .patch_helmfile_metadata(namespace, name, &patch)
            .await?;
    }
    Ok(())
}

fn map_result(value: HelmfileResult, exhausted: bool) -> ReconcileResult {
    match (value, exhausted) {
        (HelmfileResult::Applied, _) => ReconcileResult::Success,
        (HelmfileResult::NoChange, _) => ReconcileResult::Success,
        (HelmfileResult::Failed(reason), false) => ReconcileResult::Failed(reason),
        (HelmfileResult::Failed(reason), true) => ReconcileResult::FailedRetriesExhausted(reason),
    }
}

async fn prepare_age_key(
    client: &impl K8sClient,
    namespace: &str,
    secret_name: &str,
) -> Result<(NamedTempFile, (String, String))> {
    // get secret
    let secret = client.get_secret(namespace, secret_name).await?;
    let Some(data) = secret.data else {
        return Err(Error::MissingSecret(format!(
            "Could not get data from secret {secret_name}"
        )));
    };
    let Some(value) = data.get(SECRETS_KEY_AGE) else {
        return Err(Error::MissingSecret(format!(
            "Secret {secret_name} does not have key {SECRETS_KEY_AGE}"
        )));
    };

    // write data to temp file
    let mut file = tempfile::NamedTempFile::new()?;
    file.write_all(&value.0)?;

    let env = (
        SECRETS_ENV_KEY_AGE.to_owned(),
        file.path()
            .to_str()
            .ok_or(Error::CryptoHandling(
                "Could not get path to temporary key file".to_owned(),
            ))?
            .to_owned(),
    );

    Ok((file, env))
}

#[cfg(test)]
mod tests {
    use kube::core::ObjectMeta;
    use tempfile::TempDir;

    use super::*;
    use crate::extcrds::gitrepositories::{
        GitRepositorySpec, GitRepositoryStatus, GitRepositoryStatusArtifact,
    };
    use crate::flux::artifact::MockFluxSourceAdapter;
    use crate::helmfile::MockHelmfileAdapter;
    use crate::k8sclient::tests::*;
    use crate::store::new_store;

    fn minimal_helmfile(name: &str, ns: &str) -> Helmfile {
        Helmfile {
            metadata: ObjectMeta {
                name: Some(name.to_owned()),
                namespace: Some(ns.to_owned()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn minimal_helmfile_gitrepo(name: &str, ns: &str) -> (Helmfile, GitRepository) {
        let helmfile = Helmfile {
            metadata: ObjectMeta {
                name: Some(name.to_owned()),
                namespace: Some(ns.to_owned()),
                ..Default::default()
            },
            ..Default::default()
        };
        let git = GitRepository {
            metadata: ObjectMeta {
                name: Some(name.to_owned()),
                namespace: Some(ns.to_owned()),
                ..Default::default()
            },
            spec: GitRepositorySpec {
                ..Default::default()
            },
            status: Some(GitRepositoryStatus {
                artifact: Some(GitRepositoryStatusArtifact::default()),
                ..Default::default()
            }),
        };
        (helmfile, git)
    }

    #[tokio::test]
    async fn test_cleanup_helmfile_nop() {
        let client = MockClient::new();
        let store = new_store();
        let obj = minimal_helmfile("foo", "bar");
        let helmfile_adapter = MockHelmfileAdapter::new();
        let flux_adapter = MockFluxSourceAdapter::new();
        let result =
            cleanup_helmfile(client, helmfile_adapter, flux_adapter, store, &obj, None).await;
        assert!(matches!(result, Ok(ReconcileResult::Success)));
    }

    #[tokio::test]
    async fn test_cleanup_helmfile_withrepo() {
        let client = MockClient::new();
        let store = new_store();
        let (obj, git) = minimal_helmfile_gitrepo("foo", "bar");

        let mut helmfile_adapter = MockHelmfileAdapter::new();
        let mut flux_adapter = MockFluxSourceAdapter::new();

        flux_adapter
            .expect_fetch_and_extract_artifact()
            .once()
            .returning(|_, _| Ok((TempDir::new().unwrap(), "digest".to_string())));
        helmfile_adapter
            .expect_destroy()
            .once()
            .returning(|_, _, _| HelmfileResult::Applied);

        let result = cleanup_helmfile(
            client,
            helmfile_adapter,
            flux_adapter,
            store,
            &obj,
            Some(git),
        )
        .await;
        assert!(matches!(result, Ok(ReconcileResult::Success)));
    }

    #[tokio::test]
    async fn test_reconcile_helmfile_no_artifact() {
        let client = MockClient::new();
        let store = new_store();
        let (obj, mut git) = minimal_helmfile_gitrepo("foo", "bar");
        git.status = None;

        let helmfile_adapter = MockHelmfileAdapter::new();
        let flux_adapter = MockFluxSourceAdapter::new();

        let result =
            reconcile_helmfile(client, helmfile_adapter, flux_adapter, store, &obj, git).await;
        assert!(matches!(result, Ok(ReconcileResult::Pending(_))));
    }

    #[tokio::test]
    async fn test_reconcile_helmfile_success() {
        let mut client = MockClient::new();
        let store = new_store();
        let (obj, git) = minimal_helmfile_gitrepo("foo", "bar");

        let mut helmfile_adapter = MockHelmfileAdapter::new();
        let mut flux_adapter = MockFluxSourceAdapter::new();

        flux_adapter
            .expect_fetch_and_extract_artifact()
            .once()
            .returning(|_, _| Ok((TempDir::new().unwrap(), "digest".to_string())));
        helmfile_adapter
            .expect_apply()
            .once()
            .returning(|_, _, _, _| HelmfileResult::Applied);
        client
            .expect_patch_helmfile_status()
            .once()
            .withf(|_, _, patch| match patch {
                Patch::Apply(v) => {
                    let status =
                        serde_json::from_value::<DeploymentStatus>(v["status"].clone()).unwrap();
                    status.status == DeploymentResult::Successful
                }
                _ => false,
            })
            .returning(|_, _, _| Ok(()));

        let result =
            reconcile_helmfile(client, helmfile_adapter, flux_adapter, store, &obj, git).await;
        assert!(matches!(result, Ok(ReconcileResult::Success)));
    }
}
