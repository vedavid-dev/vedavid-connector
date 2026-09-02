use tonic::transport::Server;
use vedavid_connector::pb::connector_server::ConnectorServer;
use vedavid_connector::prom::Prometheus;
use vedavid_connector::service::QueryService;

const DEFAULT_LISTEN: &str = "127.0.0.1:50051";
const DEFAULT_PROMETHEUS: &str = "http://127.0.0.1:9090";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let listen = std::env::var("VEDAVID_LISTEN").unwrap_or_else(|_| DEFAULT_LISTEN.into());
    let upstream =
        std::env::var("VEDAVID_PROMETHEUS_URL").unwrap_or_else(|_| DEFAULT_PROMETHEUS.into());
    let addr = listen.parse()?;

    tracing::info!(%addr, %upstream, "serving Connector");
    Server::builder()
        .add_service(ConnectorServer::new(QueryService::new(Prometheus::new(
            &upstream,
        ))))
        .serve_with_shutdown(addr, async {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("shutting down");
        })
        .await?;
    Ok(())
}
