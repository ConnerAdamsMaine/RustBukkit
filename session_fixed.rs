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

#[allow(clippy::too_many_arguments)]
pub async fn run(
    connection: &mut Connection,
    config: &ServerConfig,
    version: &'static dyn VersionPack,
    session_uuid: Uuid,
    _slots: Arc<Semaphore>,
    locations: &PlayerLocationStore,
    _registry: &SessionRegistry,
    _world: &rustbukkit_chunks::World,
    _world_store: &WorldStore,
    _block_events: &broadcast::Sender<BlockEvent>,
    _health_events: &broadcast::Sender<(Uuid, Vec<u8>)>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let frame = read_frame(connection).await?;
    if frame.id != 0 {
        bail!("expected handshake packet ID 0, got {}", frame.id);
    }
    let mut handshake = Reader::new(&frame.payload);
    let _protocol_version = handshake.varint()?;
    let _host = handshake.string(255)?;
    let _port = handshake.raw(2)?;
    let next_state = handshake.varint()?;
    match next_state {
        1 => status(connection, config, version).await,
        2 => login(connection, config, version, session_uuid, locations, shutdown).await,
        other => bail!("unsupported next protocol state: {other}"),
    }
}

async fn status(
    connection: &mut Connection,
    config: &ServerConfig,
    version: &'static dyn VersionPack,
) -> Result<()> {
    let request = read_frame(connection).await?;
    if request.id != 0 || !request.payload.is_empty() {
        bail!("expected status request");
    }
    let mut writer = Writer::new();
    let json_response = json!({
        "version": { "name": version.minecraft_version(), "protocol": version.protocol_version() },
        "players": { "max": config.max_players, "online": 0, "sample": [] },
        "description": { "text": config.motd },
        "enforcesSecureChat": false
    });
    writer.string(&json_response.to_string());
    connection
        .write_frame(version.status_response_id(), &writer.finish())
        .await?;

    let ping = read_frame(connection).await?;
    if ping.id != version.status_pong_id() || ping.payload.len() != 8 {
        bail!("invalid status ping");
    }
    connection
        .write_frame(version.status_pong_id(), &ping.payload)
        .await?;
    Ok(())
}

async fn login(
    connection: &mut Connection,
    _config: &ServerConfig,
    version: &'static dyn VersionPack,
    session_uuid: Uuid,
    locations: &PlayerLocationStore,
    shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let login_proto = version
        .login_protocol()
        .ok_or_else(|| anyhow::anyhow!("login protocol not supported"))?;
    let start_frame = read_frame(connection).await?;
    let username = login_proto.parse_login_start(&start_frame.payload)?;
    let player_uuid = offline_uuid(&username);

    let success_payload = login_proto.login_success_payload(player_uuid, &username, session_uuid);
    connection
        .write_frame(login_proto.login_success_id(), &success_payload)
        .await?;

    let config_proto = version
        .configuration_protocol()
        .ok_or_else(|| anyhow::anyhow!("configuration protocol not supported"))?;
    let ack_frame = read_frame(connection).await?;
    if ack_frame.id != config_proto.login_acknowledged_id() {
        bail!("expected login acknowledgment packet, got {}", ack_frame.id);
    }

    match version.login_transition() {
        LoginTransition::Configuration => {
            negotiate_configuration(connection, version, &username, player_uuid, locations, shutdown).await
        }
        LoginTransition::Play => {
            bail!("direct play transition not implemented for this version")
        }
    }
}

async fn negotiate_configuration(
    connection: &mut Connection,
    version: &'static dyn VersionPack,
    username: &str,
    player_uuid: Uuid,
    locations: &PlayerLocationStore,
    shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let config_proto = version
        .configuration_protocol()
        .ok_or_else(|| anyhow::anyhow!("configuration protocol not supported"))?;
    for (id, payload) in config_proto.initial_packets() {
        connection.write_frame(id, &payload).await?;
    }

    let spawn = PlayerLocation::spawn(64.0);
    let mut location = locations.load(player_uuid, username, spawn).await?;

    loop {
        let frame = read_frame(connection).await;
        match frame {
            Ok(wire_frame) => {
                match config_proto.parse_inbound(wire_frame.id, &wire_frame.payload) {
                    Ok(rustbukkit_protocol::ConfigurationInbound::Finish) => {
                        connection
                            .write_frame(config_proto.login_acknowledged_id(), &[])
                            .await?;
                        break;
                    }
                    Ok(rustbukkit_protocol::ConfigurationInbound::KeepAlive(id)) => {
                        let mut w = Writer::new();
                        w.i64(id);
                        connection.write_frame(0x04, &w.finish()).await?;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        debug!("[CONFIGURATION] error parsing inbound: {error}");
                    }
                }
            }
            Err(_) => break,
        }
    }

    // Now enter play state
    play(connection, version, &mut location, shutdown).await
}

async fn play(
    connection: &mut Connection,
    version: &'static dyn VersionPack,
    position: &mut PlayerLocation,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    // Send Join Game packet
    let join = version.join_game_payload(20, 4, 4);
    connection
        .write_frame(version.clientbound_id(ClientboundPacket::JoinGame), &join)
        .await?;

    // Send Server Data
    let mut server_data = Writer::new();
    server_data.bool(true); // enforced secure chat
    server_data.string(&json!({ "text": "RustBukkit" }).to_string());
    server_data.bool(false); // no chat previews
    server_data.bool(false); // offline server does not enforce signed chat
    connection
        .write_frame(version.clientbound_id(ClientboundPacket::ServerData), &server_data.finish())
        .await?;

    // Send Health
    let mut health = Writer::new();
    health.f32(20.0);
    health.varint(20);
    health.f32(5.0);
    connection
        .write_frame(version.clientbound_id(ClientboundPacket::UpdateHealth), &health.finish())
        .await?;

    // Send Time
    let mut time = Writer::new();
    time.i64(0);
    time.i64(1000);
    connection
        .write_frame(version.clientbound_id(ClientboundPacket::UpdateTime), &time.finish())
        .await?;

    // Send Spawn Position
    let mut spawn = Writer::new();
    spawn.position(0, 64, 0);
    spawn.f32(0.0);
    connection
        .write_frame(version.clientbound_id(ClientboundPacket::SpawnPosition), &spawn.finish())
        .await?;

    // Send initial Player Position
    let mut position_packet = Writer::new();
    position_packet.f64(position.x);
    position_packet.f64(position.y);
    position_packet.f64(position.z);
    position_packet.f32(0.0);
    position_packet.f32(0.0);
    position_packet.i8(0);
    position_packet.varint(1);
    position_packet.bool(false); // dismount vehicle
    connection
        .write_frame(version.clientbound_id(ClientboundPacket::PlayerPosition), &position_packet.finish())
        .await?;

    // Send initial chunks around spawn
    let center = (position.x / 16.0).floor() as i32;
    let mut loaded = std::collections::HashSet::new();
    send_chunks(connection, version, center, 0, 4, &mut loaded).await?;

    let mut interval = tokio::time::interval(Duration::from_secs(15));
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    interval.tick().await;

    let mut next_keepalive = 1i64;
    let mut outstanding: Option<(i64, Instant)> = None;

    loop {
        tokio::select! {
            biased;
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    return Ok(());
                }
            }
            frame = connection.read_frame() => {
                let Some(frame) = frame? else { return Ok(()); };
                match version.parse_play(frame.id, &frame.payload)? {
                    PlayInbound::TeleportConfirm(id) => {
                        if id != 1 { bail!("invalid teleport acknowledgement"); }
                    }
                    PlayInbound::KeepAlive(received) => {
                        if outstanding.map(|(id, _)| id) != Some(received) {
                            bail!("invalid keepalive reply");
                        }
                        outstanding = None;
                    }
                    PlayInbound::Position { x, y, z, .. } => {
                        *position = PlayerLocation { x, y, z, ..*position };
                        trace!("[MOVE] x={} y={} z={}", position.x, position.y, position.z);
                    }
                    PlayInbound::PositionAndLook { x, y, z, yaw, pitch, on_ground, .. } => {
                        *position = PlayerLocation { x, y, z, yaw, pitch, on_ground, ..*position };
                        trace!("[MOVE] x={} y={} z={} yaw={} pitch={} on_ground={}", position.x, position.y, position.z, position.yaw, position.pitch, position.on_ground);
                    }
                    PlayInbound::Look { yaw, pitch, on_ground } => {
                        position.yaw = yaw;
                        position.pitch = pitch;
                        position.on_ground = on_ground;
                        trace!("[LOOK] yaw={} pitch={} on_ground={}", position.yaw, position.pitch, position.on_ground);
                    }
                    PlayInbound::OnGround(on_ground) => {
                        position.on_ground = on_ground;
                        trace!("[ON_GROUND] on_ground={}", position.on_ground);
                    }
                    PlayInbound::HeldSlot(slot) => {
                        position.selected_hotbar_slot = slot;
                        trace!("[HELD_SLOT] slot={}", position.selected_hotbar_slot);
                    }
                    _ => {} // Ignore other play packets
                }

                // Chunk loading based on position
                let center = (position.x / 16.0).floor() as i32;
                let current_z = (position.z / 16.0).floor() as i32;
                send_chunks(connection, version, center, current_z, 4, &mut loaded).await?;
            }
            _ = interval.tick() => {
                // Send keepalive
                let mut ka = Writer::new();
                ka.i64(next_keepalive);
                connection
                    .write_frame(version.clientbound_id(ClientboundPacket::KeepAlive), &ka.finish())
                    .await?;
                outstanding = Some((next_keepalive, Instant::now()));
                next_keepalive = next_keepalive.wrapping_add(1);
            }
            _ = async {
                if let Some((_, sent)) = outstanding {
                    if sent.elapsed() >= Duration::from_secs(30) {
                        return Err(anyhow::anyhow!("keepalive timed out"));
                    }
                }
                Ok::<_, anyhow::Error>(())
            } => {}
        }
    }
}

async fn send_chunks(
    connection: &mut Connection,
    version: &'static dyn VersionPack,
    center_x: i32,
    center_z: i32,
    view_distance: u8,
    loaded: &mut std::collections::HashSet<(i32, i32)>,
) -> Result<()> {
    let radius = view_distance as i32;
    for distance in 0..=radius {
        for dz in -distance..=distance {
            for dx in -distance..=distance {
                let chunk = (center_x + dx, center_z + dz);
                if loaded.insert(chunk) {
                    let payload = version.chunk_payload(chunk.0, chunk.1);
                    connection.write_frame(version.clientbound_id(ClientboundPacket::ChunkData), &payload).await?;
                    debug!("[TX] ChunkData chunk=({},{})", chunk.0, chunk.1);
                }
            }
        }
    }
    Ok(())
}

fn offline_uuid(username: &str) -> Uuid {
    let digest = md5::compute(format!("OfflinePlayer:{username}").as_bytes());
    let mut bytes = digest.0;
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}


