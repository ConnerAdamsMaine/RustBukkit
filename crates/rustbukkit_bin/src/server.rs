use std::sync::Arc;

use anyhow::Result;
use rustbukkit_config::ServerConfig;
use rustbukkit_protocol::{Connection, VersionPack};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tracing::{error, info};

use crate::player_location::PlayerLocationStore;
use crate::session;

pub async fn run(config: ServerConfig, version: &'static dyn VersionPack) -> Result<()> {
    let listener = TcpListener::bind(config.bind).await?;
    info!(
        "[STARTUP] Listening on {} for Minecraft {} (protocol {})",
        config.bind,
        version.minecraft_version(),
        version.protocol_version()
    );
    let config = Arc::new(config);
    let slots = Arc::new(Semaphore::new(config.max_players as usize));
    let locations = Arc::new(PlayerLocationStore::new("world/playerdata".into()));
    loop {
        let (stream, peer_addr) = listener.accept().await?;
        info!("[CONNECTION] accepted {peer_addr}");
        let config = Arc::clone(&config);
        let slots = Arc::clone(&slots);
        let locations = Arc::clone(&locations);
        tokio::spawn(async move {
            let mut connection = Connection::new(stream, peer_addr);
            if let Err(error) = session::run(&mut connection, &config, version, slots, &locations).await {
                error!("[CONNECTION] peer={peer_addr} state={:?} error={error}", connection.state());
            }
            if let Err(error) = connection.shutdown().await {
                error!("[CONNECTION] peer={peer_addr} shutdown error={error}");
            }
        });
    }
}
