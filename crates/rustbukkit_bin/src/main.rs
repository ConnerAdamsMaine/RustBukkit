mod player_location;
mod server;
mod session;

use anyhow::Result;
use rustbukkit_config::ServerConfig;
use rustbukkit_protocol::VersionRegistry;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_line_number(true)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .compact()
        .init();
    let config = ServerConfig::load(std::path::Path::new("server.conf"))?;
    let version = VersionRegistry::select(&config.mc_version)?;
    server::run(config, version).await
}
