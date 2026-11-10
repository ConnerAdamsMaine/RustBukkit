use anyhow::Result;
use rustbukkit_bin::server;
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use rustbukkit_config::ServerConfig;
    use rustbukkit_protocol::VersionRegistry;

    #[test]
    fn checked_in_configuration_selects_26_3_networking() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let config = ServerConfig::load(&root.join("server.conf")).unwrap();
        let version = VersionRegistry::select(&config.mc_version).unwrap();
        assert_eq!(version.protocol_version(), 777);
    }
}
