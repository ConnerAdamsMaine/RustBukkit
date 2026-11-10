use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use rustbukkit_config::ServerConfig;
use rustbukkit_protocol::{Connection, VersionPack};
use tokio::net::TcpListener;
use tokio::sync::{Semaphore, broadcast, watch};
use tokio::task::JoinSet;
use tokio::time::{Duration, timeout};
use tracing::{error, info};

use crate::player_location::PlayerLocationStore;
use crate::session::{self, BlockEvent};
use crate::session_registry::SessionRegistry;
use crate::world_persistence::WorldStore;

pub async fn run(config: ServerConfig, version: &'static dyn VersionPack) -> Result<()> {
    let listener = TcpListener::bind(config.bind).await?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let signal = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            let _ = shutdown_tx.send(true);
        }
    });
    let result = serve(config, version, listener, PathBuf::from("world"), shutdown_rx).await;
    signal.abort();
    result
}

/// Serve an already-bound listener until `shutdown` flips to `true`.
///
/// Splitting the listener out keeps integration tests on the exact code path the
/// production binary uses instead of a test-only server implementation.
pub async fn serve(
    config: ServerConfig,
    version: &'static dyn VersionPack,
    listener: TcpListener,
    world_root: PathBuf,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    info!(
        "[STARTUP] Listening on {} for Minecraft {} (protocol {})",
        listener.local_addr()?,
        version.minecraft_version(),
        version.protocol_version()
    );
    let config = Arc::new(config);
    let slots = Arc::new(Semaphore::new(config.max_players as usize));
    let locations = Arc::new(PlayerLocationStore::new(world_root.join("playerdata")));
    let registry = Arc::new(SessionRegistry::default());
    let world_store = Arc::new(WorldStore::new(world_root));
    let world = Arc::new(world_store.load().await?);
    let (block_events, _) = broadcast::channel::<BlockEvent>(1024);
    let session_uuid = uuid::Uuid::new_v4();
    let mut sessions = JoinSet::new();
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = tick.tick() => {
                let started = std::time::Instant::now();
                registry.tick(&world);
                tracing::trace!(duration = ?started.elapsed(), "server gameplay tick");
            }
            accepted = listener.accept() => {
                let (stream, peer_addr) = accepted?;
                info!("[CONNECTION] accepted {peer_addr}");
                let config = Arc::clone(&config);
                let slots = Arc::clone(&slots);
                let locations = Arc::clone(&locations);
                let registry = Arc::clone(&registry);
                let world = Arc::clone(&world);
                let block_events = block_events.clone();
                let shutdown = shutdown.clone();
                sessions.spawn(async move {
                    let mut connection = Connection::new(stream, peer_addr);
                    if let Err(error) = session::run(
                        &mut connection,
                        &config,
                        version,
                        session_uuid,
                        slots,
                        &locations,
                        &registry,
                        &world,
                        &block_events,
                        shutdown,
                    )
                    .await
                    {
                        error!("[CONNECTION] peer={peer_addr} state={:?} error={error}", connection.state());
                    }
                    if let Err(error) = connection.shutdown().await {
                        error!("[CONNECTION] peer={peer_addr} shutdown error={error}");
                    }
                });
            }
            Some(completed) = sessions.join_next(), if !sessions.is_empty() => {
                if let Err(error) = completed {
                    error!("[CONNECTION] session task failed: {error}");
                }
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    info!("[SHUTDOWN] closing active sessions");
                    break;
                }
            }
        }
    }

    while !sessions.is_empty() {
        match timeout(Duration::from_secs(config.connection_timeout_seconds), sessions.join_next()).await {
            Ok(Some(Ok(()))) => {}
            Ok(Some(Err(error))) => error!("[SHUTDOWN] session task failed: {error}"),
            Ok(None) => break,
            Err(_) => {
                error!("[SHUTDOWN] session grace period expired");
                sessions.abort_all();
                break;
            }
        }
    }
    world_store.save(&world).await?;
    info!("[SHUTDOWN] world saved");
    Ok(())
}
