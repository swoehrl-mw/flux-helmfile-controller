use async_trait::async_trait;
use k8s_openapi::api::core::v1::Secret;
use kube::api::{Patch, PatchParams};
use kube::client::Client;
use kube::Api;
use serde_json::Value;

const PATCH_OWNER: &str = "flux-helmfile-controller";

#[async_trait]
pub trait K8sClient: Clone {
    async fn get_secret(&self, namespace: &str, name: &str) -> Result<Secret, kube::error::Error>;
    async fn patch_helmfile_metadata(
        &self,
        namespace: &str,
        name: &str,
        patch: &Patch<Value>,
    ) -> Result<(), kube::error::Error>;
    async fn patch_helmfile_status(
        &self,
        namespace: &str,
        name: &str,
        patch: &Patch<Value>,
    ) -> Result<(), kube::error::Error>;
}

#[derive(Clone)]
pub struct K8sClientImpl {
    client: Client,
}

impl K8sClientImpl {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl K8sClient for K8sClientImpl {
    async fn get_secret(&self, namespace: &str, name: &str) -> Result<Secret, kube::error::Error> {
        let api = Api::<Secret>::namespaced(self.client.clone(), namespace);
        api.get(name).await
    }
    async fn patch_helmfile_metadata(
        &self,
        namespace: &str,
        name: &str,
        patch: &Patch<Value>,
    ) -> Result<(), kube::error::Error> {
        let api = Api::<Secret>::namespaced(self.client.clone(), namespace);
        let ps = PatchParams::apply(PATCH_OWNER);
        api.patch_metadata(name, &ps, patch).await?;
        Ok(())
    }
    async fn patch_helmfile_status(
        &self,
        namespace: &str,
        name: &str,
        patch: &Patch<Value>,
    ) -> Result<(), kube::error::Error> {
        let api = Api::<Secret>::namespaced(self.client.clone(), namespace);
        let ps = PatchParams::apply(PATCH_OWNER).force();
        api.patch_status(name, &ps, patch).await?;
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use mockall::mock;

    mock! {
        pub Client {}
        #[async_trait]
        impl K8sClient for Client {
            async fn get_secret(&self, namespace: &str, name: &str) -> Result<Secret, kube::error::Error>;
            async fn patch_helmfile_metadata(&self, namespace: &str, name: &str, patch: &Patch<Value>) -> Result<(), kube::error::Error>;
            async fn patch_helmfile_status(&self, namespace: &str, name: &str, patch: &Patch<Value>) -> Result<(), kube::error::Error>;
        }
        impl Clone for Client {
            fn clone(&self) -> Self;
        }
    }
}
