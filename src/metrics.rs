use crate::crd::Helmfile;
use kube::ResourceExt;
use lazy_static::lazy_static;
use prometheus_client::{
    encoding::{text::encode, EncodeLabelSet},
    metrics::{counter::Counter, family::Family},
    registry::Registry,
};
use tokio::sync::Mutex;

lazy_static! {
    static ref REGISTRY: Mutex<Registry> = Mutex::new(<Registry>::default());
    pub static ref NUM_RECONCILES_STARTED: Family<HelmfileLabels, Counter> =
        Family::<HelmfileLabels, Counter>::default();
    pub static ref NUM_RECONCILES_PENDING: Family<HelmfileLabels, Counter> =
        Family::<HelmfileLabels, Counter>::default();
    pub static ref NUM_RECONCILES_FAILED: Family<HelmfileLabels, Counter> =
        Family::<HelmfileLabels, Counter>::default();
    pub static ref NUM_CLEANUPS_STARTED: Family<HelmfileLabels, Counter> =
        Family::<HelmfileLabels, Counter>::default();
    pub static ref NUM_CLEANUPS_FAILED: Family<HelmfileLabels, Counter> =
        Family::<HelmfileLabels, Counter>::default();
}

#[derive(Clone, Hash, PartialEq, Eq, EncodeLabelSet, Debug)]
pub struct HelmfileLabels {
    pub namespace: String,
    pub name: String,
}

pub fn l(obj: &Helmfile) -> HelmfileLabels {
    HelmfileLabels {
        namespace: obj.namespace().unwrap_or_else(|| "default".to_owned()),
        name: obj.name_any(),
    }
}

pub async fn init_metrics() {
    let base = "flux_helmfile";
    let mut registry = REGISTRY.lock().await;
    registry.register(
        format!("{base}_reconciles_started_count"),
        "Number of reconciles started",
        NUM_RECONCILES_STARTED.clone(),
    );
    registry.register(
        format!("{base}_reconciles_pending_count"),
        "Number of reconciles started",
        NUM_RECONCILES_PENDING.clone(),
    );
    registry.register(
        format!("{base}_reconciles_failed_count"),
        "Number of reconciles failed",
        NUM_RECONCILES_FAILED.clone(),
    );
    registry.register(
        format!("{base}_cleanups_started_count"),
        "Number of cleanups started",
        NUM_CLEANUPS_STARTED.clone(),
    );
    registry.register(
        format!("{base}_cleanups_failed_count"),
        "Number of cleanups failed",
        NUM_CLEANUPS_FAILED.clone(),
    );
}

pub async fn metrics() -> Result<String, std::fmt::Error> {
    let mut buffer = String::new();
    let registry = REGISTRY.lock().await;
    encode(&mut buffer, &registry)?;
    Ok(buffer)
}
