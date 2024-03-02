use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("KubernetesClientError: {0}")]
    KubernetesClient(#[from] kube::Error),
    #[error("InvalidKubernetesObject: {0}")]
    InvalidKubernetesObject(String),
    #[error("ArtifactDownloadError: {0}")]
    ArtifactDownloadReqwest(#[from] reqwest::Error),
    #[error("ArtifactDownloadError: {0}")]
    ArtifactDownload(String),
    #[error("ArtifactArchiveExtractError: {0}")]
    ArtifactExtract(#[from] std::io::Error),
    #[error("Missing or error accessing secret: {0}")]
    MissingSecret(String),
    #[error("Error during handling of crypto keys: {0}")]
    CryptoHandling(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
