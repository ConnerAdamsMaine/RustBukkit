// Core modules
mod chunk;
mod consts;
mod core;
mod error_tracker;
mod network;
mod player;
mod terrain;
mod world;

mod serialization;

// Developer SDK modules (feature-gated)
#[cfg(feature = "dev-sdk")]
mod sdk;

// Re-export commonly used types
use anyhow::Result;
pub use error_tracker::{ErrorKey, ErrorTracker};

use crate::consts::SERVER_ADDR;
use crate::core::MinecraftServer;
#[cfg(feature = "dev-sdk")]
use crate::sdk::PacketLogger;

#[cfg(feature = "dev-sdk")]
pub static LOGGER: std::sync::LazyLock<PacketLogger> =
    std::sync::LazyLock::new(|| PacketLogger::new().expect("Failed to initialize PacketLogger"));

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging with a custom format
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_line_number(true)
        .with_max_level(tracing::Level::DEBUG)
        .compact()
        .init();

    let error_tracker = std::sync::Arc::new(ErrorTracker::new());

    // Start the Minecraft server
    let server = MinecraftServer::new(SERVER_ADDR, error_tracker.clone()).await?;
    server.run().await?;

    Ok(())
}
