mod api;
mod controller;
mod crd;
mod error;
mod extcrds;
mod flux;
mod helmfile;
mod k8sclient;
mod metrics;
mod reconciler;
mod store;
mod util;

use tracing_subscriber::{prelude::*, EnvFilter};

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    init_logging();
    metrics::init_metrics().await;
    let client = kube::Client::try_default()
        .await
        .expect("Could not initialize kube client");
    let store = store::new_store();
    let handle = tokio::spawn(api::server());
    controller::run(client, store).await;
    handle.abort();
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .expect("Could not init logging");

    let subscriber = tracing_subscriber::registry().with(filter);

    let log_mode = std::env::var("LOGGING_MODE").unwrap_or_else(|_| "plain".to_string());
    if log_mode.to_lowercase() == "json" {
        subscriber
            .with(tracing_subscriber::fmt::layer().json())
            .init();
    } else {
        subscriber.with(tracing_subscriber::fmt::layer()).init();
    }
}
