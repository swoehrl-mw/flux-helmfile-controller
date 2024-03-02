use crate::error::{Error, Result};
use crate::extcrds::gitrepositories::GitRepositoryStatusArtifact;
use crate::store::HelmfileState;
use async_trait::async_trait;
use bytes::Buf;
use flate2::read::GzDecoder;
use tempfile::TempDir;
use url::Url;

pub struct FluxSourceAdapterImpl {}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait FluxSourceAdapter {
    async fn fetch_and_extract_artifact(
        &self,
        state: Option<HelmfileState>,
        artifact: &GitRepositoryStatusArtifact,
    ) -> Result<(TempDir, String)>;
}

#[async_trait]
impl FluxSourceAdapter for FluxSourceAdapterImpl {
    async fn fetch_and_extract_artifact(
        &self,
        state: Option<HelmfileState>,
        artifact: &GitRepositoryStatusArtifact,
    ) -> Result<(TempDir, String)> {
        let digest = artifact
            .digest
            .as_ref()
            .map_or_else(|| artifact.path.replace('/', "_"), |el| el.clone());

        // Reuse existing download if digest matches
        if let Some(state) = state {
            if state.current_digest == digest {
                return Ok((state.location, state.current_digest));
            }
        }

        let mut url: Url =
            Url::parse(&artifact.url).map_err(|e| Error::ArtifactDownload(e.to_string()))?;
        if let Ok(override_host) = std::env::var("SOURCE_CONTROLLER_HOST") {
            if let Some((host, port)) = override_host.split_once(':') {
                url.set_host(Some(host))
                    .map_err(|e| Error::ArtifactDownload(e.to_string()))?;
                url.set_port(Some(port.parse().map_err(|_| {
                    Error::ArtifactDownload("Could not parse port".to_string())
                })?))
                .map_err(|_| Error::ArtifactDownload("Could not set port".to_owned()))?;
            } else {
                url.set_host(Some(override_host.as_str()))
                    .map_err(|e| Error::ArtifactDownload(e.to_string()))?;
            }
        }
        let result = reqwest::get(url).await?;
        if !result.status().is_success() {
            return Err(Error::ArtifactDownload(result.status().to_string()));
        }
        // create temp location
        let location = TempDir::new_in("tmp")?;

        // Extract artifact to tmp location
        let data = result.bytes().await?;
        let archive = GzDecoder::new(data.reader());
        let mut archive = tar::Archive::new(archive);
        archive.unpack(location.path())?;

        // Return temp location and new digest
        Ok((location, digest))
    }
}
