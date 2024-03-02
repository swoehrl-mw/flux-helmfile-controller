use crate::crd::Helmfile;
use crate::util::NS;
use async_trait::async_trait;
use kube::ResourceExt;
use std::str;
use std::{path::Path, time::Duration};
use tokio::{process::Command, time};

#[derive(Debug)]
pub enum HelmfileResult {
    Applied,
    NoChange,
    Failed(String),
}

#[derive(Debug, Clone, Copy)]
pub enum Mode {
    Apply,
    Sync,
}

pub struct HelmfileAdapterImpl {}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait HelmfileAdapter {
    async fn apply(
        &self,
        mode: Mode,
        location: &Path,
        obj: &Helmfile,
        extra_env: Option<(String, String)>,
    ) -> HelmfileResult;
    async fn destroy(
        &self,
        location: &Path,
        obj: &Helmfile,
        extra_env: Option<(String, String)>,
    ) -> HelmfileResult;
}

#[async_trait]
impl HelmfileAdapter for HelmfileAdapterImpl {
    async fn apply(
        &self,
        mode: Mode,
        location: &Path,
        obj: &Helmfile,
        extra_env: Option<(String, String)>,
    ) -> HelmfileResult {
        let mut cmd = Command::new("helmfile");
        cmd.kill_on_drop(true); // make sure we can cancel the process if it takes too long
        match mode {
            Mode::Apply => cmd
                .arg("apply")
                .arg("--skip-diff-on-install")
                .arg("--suppress-diff")
                .arg("--detailed-exitcode"),
            Mode::Sync => cmd.arg("sync"),
        };
        if let Some(environment) = obj.spec.environment.as_ref() {
            cmd.arg("-e").arg(environment);
        }
        if let Some(service_account) = obj.spec.service_account_name.as_ref() {
            cmd.arg("--args").arg(format!(
                "--kube-as-user=system:serviceaccount:{}:{service_account}",
                obj.namespace().unwrap_or_else(|| NS.to_owned())
            ));
        }
        cmd.current_dir(location);

        if let Some(extra_env) = extra_env {
            cmd.env(extra_env.0, extra_env.1);
        }

        let timeout = obj
            .spec
            .options
            .as_ref()
            .and_then(|o| o.timeout.as_ref())
            .cloned()
            .unwrap_or_else(|| "10m".to_owned());
        let timeout = parse_duration::parse(&timeout).unwrap_or_else(|err| {
            tracing::warn!("Could not parse duration: '{timeout}: {err}");
            Duration::from_secs(10 * 60)
        });

        match time::timeout(timeout, cmd.output()).await {
            Ok(Ok(output)) => match (output.status.code().unwrap_or(0), mode) {
                (2, _) => HelmfileResult::Applied,
                (0, Mode::Apply) => HelmfileResult::NoChange,
                (0, Mode::Sync) => HelmfileResult::Applied,
                _ => HelmfileResult::Failed(
                    str::from_utf8(&output.stderr)
                        .unwrap_or("failed to read stderr")
                        .to_owned(),
                ),
            },
            Ok(Err(err)) => HelmfileResult::Failed(err.to_string()),
            Err(_) => HelmfileResult::Failed("timeout".to_owned()),
        }
    }

    async fn destroy(
        &self,
        location: &Path,
        obj: &Helmfile,
        extra_env: Option<(String, String)>,
    ) -> HelmfileResult {
        let mut cmd = Command::new("helmfile");
        cmd.arg("destroy");
        if let Some(environment) = obj.spec.environment.as_ref() {
            cmd.arg("-e").arg(environment);
        }
        if let Some(service_account) = obj.spec.service_account_name.as_ref() {
            cmd.arg("--args").arg(format!(
                "--kube-as-user=system:serviceaccount:{}:{service_account}",
                obj.namespace().unwrap_or_else(|| NS.to_owned())
            ));
        }
        cmd.current_dir(location);

        if let Some(extra_env) = extra_env {
            cmd.env(extra_env.0, extra_env.1);
        }

        let timeout = obj
            .spec
            .options
            .as_ref()
            .and_then(|o| o.timeout.as_ref())
            .cloned()
            .unwrap_or_else(|| "10m".to_owned());
        let timeout = parse_duration::parse(&timeout).unwrap_or_else(|err| {
            tracing::warn!("Could not parse duration: '{timeout}: {err}");
            Duration::from_secs(10 * 60)
        });

        match time::timeout(timeout, cmd.output()).await {
            Ok(Ok(output)) => {
                if output.status.success() {
                    HelmfileResult::Applied
                } else {
                    HelmfileResult::Failed(
                        str::from_utf8(&output.stderr)
                            .unwrap_or("failed to read stderr")
                            .to_owned(),
                    )
                }
            }
            Ok(Err(err)) => HelmfileResult::Failed(err.to_string()),
            Err(_) => HelmfileResult::Failed("timeout".to_owned()),
        }
    }
}
