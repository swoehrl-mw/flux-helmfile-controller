use crate::{crd::Helmfile, util::NS};
use kube::ResourceExt;
use std::{collections::HashMap, sync::Arc};
use tempfile::TempDir;
use tokio::sync::RwLock;

pub type ControllerStoreRef = Arc<RwLock<ControllerStore>>;

pub struct HelmfileState {
    pub current_digest: String,
    pub location: TempDir,
    pub num_retries: Option<i32>,
}

pub fn new_store() -> ControllerStoreRef {
    let store = ControllerStore {
        helmfiles: HashMap::new(),
        state: HashMap::new(),
    };
    Arc::new(RwLock::new(store))
}

pub struct ControllerStore {
    pub helmfiles: HashMap<NamespacedName, Helmfile>,
    pub state: HashMap<NamespacedName, HelmfileState>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NamespacedName {
    pub name: String,
    pub namespace: String,
}

impl From<&Helmfile> for NamespacedName {
    fn from(obj: &Helmfile) -> Self {
        let name = obj.name_any();
        let namespace = obj.namespace().unwrap_or_else(|| NS.to_owned());
        NamespacedName { name, namespace }
    }
}

impl From<&Arc<Helmfile>> for NamespacedName {
    fn from(obj: &Arc<Helmfile>) -> Self {
        let name = obj.name_any();
        let namespace = obj.namespace().unwrap_or_else(|| NS.to_owned());
        NamespacedName { name, namespace }
    }
}
