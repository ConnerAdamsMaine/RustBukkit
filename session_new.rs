use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use rustbukkit_config::ServerConfig;
use rustbukkit_decoding::Reader;
use rustbukkit_encoding::Writer;
use rustbukkit_protocol::{
    ClientboundPacket, Connection, LoginTransition, PlayInbound, VersionPack,
};
use serde_json::json;
use tokio::sync::{Semaphore, broadcast, watch};
use tokio::time::MissedTickBehavior;
use tracing::{debug, trace};
use uuid::Uuid;

use crate::player_location::{PlayerLocation, PlayerLocationStore};
use crate::session_registry::SessionRegistry;
use crate::world_persistence::WorldStore;

#[derive(Debug, Clone)]
pub enum BlockEvent {
    Placed {
        position:  (i32, i32, i32),
        block:     u32,
        entity_id: Option<i32>,
    },
    Broken {
        position:  (i32, i32, i32),
        entity_id: Option<i32>,
    },
}

async fn read_frame(connection: &mut Connection) -> Result<rustbukkit_decoding::Frame> {
    connection
        .read_frame()
        .await?
        .ok_or_else(|| anyhow::anyhow!("connection closed"))
}

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use rustbukkit_config::ServerConfig;
use rustbukkit_decoding::Reader;
use rustbukkit_encoding::Writer;
use rustbukkit_protocol::{
    ClientboundPacket, Connection, LoginTransition, PlayInbound, VersionPack,
};
use serde_json::json;
use tokio::sync::{Semaphore, broadcast, watch};
use tokio::time::MissedTickBehavior;
use tracing::{debug, trace};
use uuid::Uuid;

use crate::player_location::{PlayerLocation, PlayerLocationStore};
use crate::session_registry::SessionRegistry;
use crate::world_persistence::WorldStore;

#[derive(Debug, Clone)]
pub enum BlockEvent {
    Placed {
        position:  (i32, i32, i32),
        block:     u32,
        entity_id: Option<i32>,
    },
    Broken {
        position:  (i32, i32, i32),
        entity_id: Option<i32>,
    },
