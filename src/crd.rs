use kube::CustomResource;
use schemars::JsonSchema;
use serde_derive::{Deserialize, Serialize};

#[derive(
    CustomResource, Serialize, Deserialize, Debug, Clone, Hash, PartialEq, Eq, JsonSchema, Default,
)]
#[kube(
    group = "flux.maibornwolff.de",
    version = "v1alpha1",
    kind = "Helmfile",
    namespaced
)]
#[kube(status = "DeploymentStatus")]
#[kube(derive = "Default")]
#[serde(rename_all = "camelCase")]
pub struct HelmfileSpec {
    /// reconcile interval
    pub interval: Option<String>,
    /// config for the git repo to use
    pub source_ref: SourceRef,
    /// a path in the source repo to use, if not set repo root is used
    pub path: Option<String>,
    /// environment to use for helmfile (helmfile -e)
    pub environment: Option<String>,
    /// decryption information
    pub decryption: Option<Decryption>,
    /// options for helmfile exection
    pub options: Option<Options>,
    /// name of the serviceAccount to impersonate
    pub service_account_name: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Hash, PartialEq, Eq, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct SourceRef {
    /// kind of the source, currently only GitRepository is supported
    pub kind: SourceRefKind,
    /// name of the source object
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Hash, PartialEq, Eq, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct Decryption {
    /// kind of the decryption provider
    pub provider: DecryptionProviderKind,
    /// name of the secret containing decryption keys
    pub secret_ref: LocalObjectReference,
}

#[derive(Serialize, Deserialize, Debug, Clone, Hash, PartialEq, Eq, JsonSchema, Default)]
pub enum SourceRefKind {
    #[default]
    GitRepository,
}

#[derive(Serialize, Deserialize, Debug, Clone, Hash, PartialEq, Eq, JsonSchema, Default)]
#[serde(rename_all = "kebab-case")]
pub enum DecryptionProviderKind {
    #[default]
    SopsAge,
    // SopsPGP,
}

#[derive(Serialize, Deserialize, Debug, Clone, Hash, PartialEq, Eq, JsonSchema, Default)]
pub struct Options {
    /// timeout for running helmfile commands, will be aborted afterwards
    pub timeout: Option<String>,
    /// number of retries, 0 means never, negative means retry forever, default is retry forever
    pub retries: Option<i32>,
    /// if set to true `helmfile destroy` will be run when the Helmfile object is deleted
    pub prune: Option<bool>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentStatus {
    pub status: DeploymentResult,
    pub reason: Option<String>,
    pub last_update: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Hash, PartialEq, Eq, JsonSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum DeploymentResult {
    Failed,
    Successful,
    #[default]
    Pending,
}

#[derive(Serialize, Deserialize, Debug, Clone, Hash, PartialEq, Eq, JsonSchema, Default)]
pub struct LocalObjectReference {
    /// Name of the secret
    pub name: String,
}
