use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use rustbukkit_chunks::{BlockKind, BlockPos, ChunkPos, World};
use rustbukkit_config::ServerConfig;
use rustbukkit_decoding::Reader;
use rustbukkit_encoding::Writer;
use rustbukkit_inventory::Inventory;
use rustbukkit_protocol::{
    BlockFace,
    ClientboundPacket,
    ConfigurationInbound,
    Connection,
    LoginTransition,
    PlayInbound,
    ProtocolState,
    VersionPack,
};
use serde_json::json;
use tokio::sync::{Semaphore, broadcast, watch};
use tokio::time::MissedTickBehavior;
use tracing::{debug, info, trace, warn};
use uuid::Uuid;

use crate::player_location::{PlayerLocation, PlayerLocationStore};
use crate::session_registry::{CombatEvent, PresenceEvent, SessionRegistration, SessionRegistry};

/// Authoritative block change that every playing session applies.
#[derive(Debug, Clone, Copy)]
pub enum BlockEvent {
    Changed {
        source:   Uuid,
        position: BlockPos,
        block:    BlockKind,
    },
}

/// Maximum interaction distance between a player's torso and a block centre.
const MAX_REACH_SQUARED: f64 = 36.0;

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
    slots: Arc<Semaphore>,
    locations: &PlayerLocationStore,
    registry: &Arc<SessionRegistry>,
    world: &World,
    block_events: &broadcast::Sender<BlockEvent>,
    presence_events: &broadcast::Sender<PresenceEvent>,
    shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let frame = read_frame(connection).await?;
    if frame.id != 0 {
        bail!("expected handshake packet ID 0, got {}", frame.id);
    }
    let mut handshake = Reader::new(&frame.payload);
    let protocol_version = handshake.varint()?;
    let _host = handshake.string(255)?;
    let _port = handshake.raw(2)?;
    let next_state = handshake.varint()?;
    if handshake.remaining() != 0 {
        bail!("trailing bytes in handshake");
    }
    match next_state {
        1 => status(connection, config, version).await,
        2 => {
            if protocol_version != version.protocol_version() {
                bail!(
                    "unsupported protocol version {protocol_version}; this server speaks {}",
                    version.protocol_version()
                );
            }
            login(
                connection,
                config,
                version,
                session_uuid,
                slots,
                locations,
                registry,
                world,
                block_events,
                presence_events,
                shutdown,
            )
            .await
        }
        other => bail!("unsupported next protocol state: {other}"),
    }
}

async fn status(
    connection: &mut Connection,
    config: &ServerConfig,
    version: &'static dyn VersionPack,
) -> Result<()> {
    connection.set_state(ProtocolState::Status)?;
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

#[allow(clippy::too_many_arguments)]
async fn login(
    connection: &mut Connection,
    config: &ServerConfig,
    version: &'static dyn VersionPack,
    session_uuid: Uuid,
    slots: Arc<Semaphore>,
    locations: &PlayerLocationStore,
    registry: &Arc<SessionRegistry>,
    world: &World,
    block_events: &broadcast::Sender<BlockEvent>,
    presence_events: &broadcast::Sender<PresenceEvent>,
    shutdown: watch::Receiver<bool>,
) -> Result<()> {
    connection.set_state(ProtocolState::Login)?;
    let login_proto = version
        .login_protocol()
        .ok_or_else(|| anyhow::anyhow!("login protocol not supported"))?;
    let start_frame = read_frame(connection).await?;
    if start_frame.id != 0 {
        bail!("expected login start packet, got {}", start_frame.id);
    }
    let username = login_proto.parse_login_start(&start_frame.payload)?;
    let player_uuid = offline_uuid(&username);

    let _slot = match slots.try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            write_login_disconnect(connection, version, "Server is full").await;
            bail!("server is full");
        }
    };
    let reservation = match registry.reserve(player_uuid) {
        Some(reservation) => reservation,
        None => {
            write_login_disconnect(connection, version, "You are already connected").await;
            bail!("duplicate login for {username}");
        }
    };
    connection.set_player_name(username.clone());

    let success_payload = login_proto.login_success_payload(player_uuid, &username, session_uuid);
    connection
        .write_frame(login_proto.login_success_id(), &success_payload)
        .await?;

    let ack_frame = read_frame(connection).await?;
    if ack_frame.id != login_proto.login_acknowledged_id() {
        bail!(
            "expected login acknowledgment packet {}, got {}",
            login_proto.login_acknowledged_id(),
            ack_frame.id
        );
    }

    match version.login_transition() {
        LoginTransition::Configuration => {
            negotiate_configuration(
                connection,
                config,
                version,
                &username,
                player_uuid,
                reservation,
                locations,
                registry,
                world,
                block_events,
                presence_events,
                shutdown,
            )
            .await
        }
        LoginTransition::Play => bail!("direct play transition not implemented for this version"),
    }
}

async fn write_login_disconnect(
    connection: &mut Connection,
    version: &'static dyn VersionPack,
    reason: &str,
) {
    let mut writer = Writer::new();
    writer.string(&json!({ "text": reason }).to_string());
    // Best effort: the socket may already be gone, and the caller reports the real error.
    let _ = connection
        .write_frame(version.clientbound_id(ClientboundPacket::LoginDisconnect), &writer.finish())
        .await;
}

#[allow(clippy::too_many_arguments)]
async fn negotiate_configuration(
    connection: &mut Connection,
    config: &ServerConfig,
    version: &'static dyn VersionPack,
    username: &str,
    player_uuid: Uuid,
    reservation: crate::session_registry::SessionReservation,
    locations: &PlayerLocationStore,
    registry: &Arc<SessionRegistry>,
    world: &World,
    block_events: &broadcast::Sender<BlockEvent>,
    presence_events: &broadcast::Sender<PresenceEvent>,
    shutdown: watch::Receiver<bool>,
) -> Result<()> {
    connection.set_state(ProtocolState::Configuration)?;
    let config_proto = version
        .configuration_protocol()
        .ok_or_else(|| anyhow::anyhow!("configuration protocol not supported"))?;
    for (id, payload) in config_proto.initial_packets() {
        connection.write_frame(id, &payload).await?;
    }
    connection
        .write_frame(config_proto.finish_configuration_id(), &[])
        .await?;

    loop {
        let frame = read_frame(connection).await?;
        match config_proto.parse_inbound(frame.id, &frame.payload) {
            Ok(ConfigurationInbound::Finish) => break,
            Ok(ConfigurationInbound::KeepAlive(id)) => {
                let mut writer = Writer::new();
                writer.i64(id);
                connection.write_frame(0x04, &writer.finish()).await?;
            }
            Ok(_) => {}
            Err(error) => {
                debug!(
                    "[CONFIGURATION] peer={} ignoring packet {}: {error}",
                    connection.peer_addr(),
                    frame.id
                );
            }
        }
    }

    let spawn = PlayerLocation::spawn(f64::from(world.spawn_y(0, 0)));
    let crate::player_location::PlayerSnapshot { location, inventory } =
        locations.load(player_uuid, username, spawn).await?;
    let registration = reservation
        .promote(username.to_owned(), location)
        .ok_or_else(|| anyhow::anyhow!("could not register player session"))?;
    info!(
        "[PLAY] {} joined entity={} at {:.1},{:.1},{:.1}",
        username,
        registration.entity_id().raw(),
        location.x,
        location.y,
        location.z
    );

    play(
        connection,
        config,
        version,
        world,
        registry,
        locations,
        username,
        player_uuid,
        location,
        inventory,
        registration,
        block_events,
        presence_events,
        shutdown,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn play(
    connection: &mut Connection,
    config: &ServerConfig,
    version: &'static dyn VersionPack,
    world: &World,
    registry: &SessionRegistry,
    locations: &PlayerLocationStore,
    username: &str,
    player_uuid: Uuid,
    mut location: PlayerLocation,
    mut inventory: Inventory,
    registration: SessionRegistration,
    block_events: &broadcast::Sender<BlockEvent>,
    presence_events: &broadcast::Sender<PresenceEvent>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    connection.set_state(ProtocolState::Play)?;

    let join = version.join_game_payload(
        registration.entity_id().raw(),
        config.max_players,
        config.view_distance,
        config.simulation_distance,
        world.seed() as i64,
        world.sea_level(),
    );
    connection
        .write_frame(version.clientbound_id(ClientboundPacket::JoinGame), &join)
        .await?;

    let server_data = version.server_data_payload(&config.motd, None);
    connection
        .write_frame(version.clientbound_id(ClientboundPacket::ServerData), &server_data)
        .await?;

    connection
        .write_frame(
            version.clientbound_id(ClientboundPacket::UpdateHealth),
            &version.update_health_payload(20.0, 20, 5.0),
        )
        .await?;

    let time = version.time_payload(1_000);
    connection
        .write_frame(version.clientbound_id(ClientboundPacket::UpdateTime), &time)
        .await?;

    let spawn_block = BlockPos::new(0, world.spawn_y(0, 0) - 1, 0);
    if let Some(spawn_block) = spawn_block {
        let payload = version.spawn_position_payload(spawn_block, 0.0, 0.0);
        connection
            .write_frame(version.clientbound_id(ClientboundPacket::SpawnPosition), &payload)
            .await?;
    }

    write_position(connection, version, 1, location).await?;

    // Teach the client the authoritative player-inventory contents before it
    // can click anything. Slot indices match the crate model: 0-8 craft/armor
    // region (always empty), 9-35 main, 36-44 hotbar, 45 offhand.
    let mut inventory_state_id = 0i32;
    connection
        .write_frame(
            version.clientbound_id(ClientboundPacket::ContainerSetContent),
            &version.container_set_content_payload(
                0,
                inventory_state_id,
                &inventory.window(),
                inventory.carried(),
            ),
        )
        .await?;

    // Announce this player to other sessions and teach this client about the
    // players already online. Presence is broadcast at the session layer
    // because the registry's own Joined event fires before a session enters
    // Play and subscribes.
    let mut joins = presence_events.subscribe();
    let mut presence_rx = registry.subscribe();
    // Combat outcomes (this player as victim included) arrive as broadcasts so
    // every session renders the same authoritative hurt/death feedback.
    let mut combat_rx = registry.subscribe_combat();
    let mut tracked_positions: HashMap<i32, (f64, f64, f64)> = HashMap::new();

    connection
        .write_frame(
            version.clientbound_id(ClientboundPacket::PlayerInfo),
            &version.player_info_add_payload(&[(player_uuid, username)]),
        )
        .await?;
    let others = registry.snapshot_except(player_uuid);
    if !others.is_empty() {
        let entries: Vec<(Uuid, &str)> = others
            .iter()
            .map(|player| (player.uuid, player.username.as_str()))
            .collect();
        connection
            .write_frame(
                version.clientbound_id(ClientboundPacket::PlayerInfo),
                &version.player_info_add_payload(&entries),
            )
            .await?;
        for other in &others {
            let payload = version.add_player_payload(
                other.entity_id,
                other.uuid,
                [other.location.x, other.location.y, other.location.z],
                other.location.yaw,
                other.location.pitch,
            );
            connection
                .write_frame(version.clientbound_id(ClientboundPacket::SpawnEntity), &payload)
                .await?;
            tracked_positions.insert(other.entity_id, (other.location.x, other.location.y, other.location.z));
        }
    }
    if let Some(snapshot) = registry.snapshot_player(player_uuid) {
        let _ = presence_events.send(PresenceEvent::Joined(snapshot));
    }

    let radius = i32::from(config.view_distance);
    let mut view = ChunkView::default();
    update_chunk_view(connection, version, world, &mut view, location.chunk(), radius).await?;

    let mut block_rx = block_events.subscribe();
    let mut keepalive = tokio::time::interval(Duration::from_secs(config.keep_alive_interval_seconds));
    keepalive.set_missed_tick_behavior(MissedTickBehavior::Delay);
    keepalive.tick().await;
    let timeout = Duration::from_secs(config.connection_timeout_seconds);
    let mut awaiting_keepalive: Option<(i64, Instant)> = None;
    let mut next_keepalive = 1i64;
    let mut last_save = Instant::now();
    let mut teleport_id = 1i32;
    // Item pickup runs a few times per second; item entities only appear near
    // the player and the pickup itself is idempotent per item.
    let mut pickup = tokio::time::interval(Duration::from_millis(100));
    pickup.set_missed_tick_behavior(MissedTickBehavior::Delay);
    pickup.tick().await;

    // The loop is wrapped so that player state is persisted even when the
    // connection fails mid-write instead of unwinding past the save below.
    let outcome: Result<()> = async {
    loop {
        if last_save.elapsed() >= Duration::from_secs(30) {
            save_location(locations, player_uuid, username, location, &inventory).await;
            last_save = Instant::now();
        }
        tokio::select! {
            biased;
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            frame = connection.read_frame() => {
                let Some(frame) = frame? else { break; };
                let packet = match version.parse_play(frame.id, &frame.payload) {
                    Ok(packet) => packet,
                    Err(error) => {
                        debug!("[PLAY] {} ignored packet 0x{:02x}: {error}", username, frame.id);
                        continue;
                    }
                };
                let mut movement: Option<(f64, f64, f64, f32, f32, bool)> = None;
                match packet {
                    PlayInbound::TeleportConfirm(id) => {
                        if id != teleport_id {
                            bail!("invalid teleport acknowledgement {id}");
                        }
                    }
                    PlayInbound::KeepAlive(id) => {
                        match awaiting_keepalive {
                            Some((expected, _)) if expected == id => awaiting_keepalive = None,
                            _ => bail!("invalid keepalive reply {id}"),
                        }
                    }
                    PlayInbound::Position { x, y, z } => {
                        movement = Some((x, y, z, location.yaw, location.pitch, location.on_ground));
                    }
                    PlayInbound::PositionAndLook { x, y, z, yaw, pitch, on_ground } => {
                        movement = Some((x, y, z, yaw, pitch, on_ground));
                    }
                    PlayInbound::Look { yaw, pitch, on_ground } => {
                        movement = Some((location.x, location.y, location.z, yaw, pitch, on_ground));
                    }
                    PlayInbound::OnGround(on_ground) => {
                        movement = Some((location.x, location.y, location.z, location.yaw, location.pitch, on_ground));
                    }
                    PlayInbound::HeldSlot(slot) => {
                        if slot < 9 {
                            location.selected_hotbar_slot = slot;
                            inventory.select(slot);
                        }
                    }
                    PlayInbound::ClickContainer { container_id, slot, button, mode, .. } => {
                        let result = if container_id == 0 {
                            inventory.apply_click(rustbukkit_inventory::ContainerClick { slot, button, mode })
                        } else {
                            rustbukkit_inventory::ClickResult { applied: false, dropped: None }
                        };
                        // Throw and cursor drops become world item entities; if
                        // entity allocation fails the stack is returned to the
                        // inventory so it is never silently deleted.
                        if let Some(stack) = result.dropped
                            && registry.drop_from_player(stack, location).is_none()
                        {
                            let leftover = inventory.insert(stack);
                            if leftover.is_some() {
                                warn!("[PLAY] {} could not preserve a dropped stack", username);
                            }
                        }
                        // Full-window resync after every click, applied or
                        // rejected, so client predictions never desync from the
                        // authoritative server state.
                        inventory_state_id = inventory_state_id.wrapping_add(1);
                        connection
                            .write_frame(
                                version.clientbound_id(ClientboundPacket::ContainerSetContent),
                                &version.container_set_content_payload(
                                    0,
                                    inventory_state_id,
                                    &inventory.window(),
                                    inventory.carried(),
                                ),
                            )
                            .await?;
                    }
                    PlayInbound::Attack { target_entity_id, .. } => {
                        // Target existence, aliveness, reach and the attack
                        // cooldown are all validated inside the registry.
                        if registry.attack_player(player_uuid, target_entity_id).is_none() {
                            debug!("[PLAY] {} attack on entity {target_entity_id} rejected", username);
                        }
                    }
                    PlayInbound::RespawnRequest => {
                        let dead = registry
                            .snapshot_player(player_uuid)
                            .is_some_and(|snapshot| !snapshot.living.alive());
                        if !dead {
                            debug!("[PLAY] {} respawn request while alive ignored", username);
                        } else if let Some(snapshot) = registry.respawn_player(
                            player_uuid,
                            PlayerLocation::spawn(f64::from(world.spawn_y(0, 0))),
                        ) {
                            location = snapshot.location;
                            connection
                                .write_frame(
                                    version.clientbound_id(ClientboundPacket::Respawn),
                                    &version.respawn_payload(0, world.seed() as i64, world.sea_level()),
                                )
                                .await?;
                            connection
                                .write_frame(
                                    version.clientbound_id(ClientboundPacket::UpdateHealth),
                                    &version.update_health_payload(snapshot.living.health, 20, 5.0),
                                )
                                .await?;
                            teleport_id = teleport_id.wrapping_add(1);
                            write_position(connection, version, teleport_id, location).await?;
                            // The death drop emptied the authoritative
                            // inventory; resync the client window.
                            inventory_state_id = inventory_state_id.wrapping_add(1);
                            connection
                                .write_frame(
                                    version.clientbound_id(ClientboundPacket::ContainerSetContent),
                                    &version.container_set_content_payload(
                                        0,
                                        inventory_state_id,
                                        &inventory.window(),
                                        inventory.carried(),
                                    ),
                                )
                                .await?;
                        }
                    }
                    PlayInbound::Dig { action, position, .. } => {
                        if action == rustbukkit_protocol::DigAction::Finish {
                            if !within_reach(location, position) {
                                debug!("[PLAY] {} ignored out-of-reach break", username);
                            } else {
                                break_block(connection, version, world, registry, registration.entity_id().raw(), player_uuid, position, username, block_events).await?;
                            }
                        }
                    }
                    PlayInbound::Place { target, face, .. } => {
                        if let Some(position) = adjacent_block(target, face) {
                            if !within_reach(location, position) {
                                debug!("[PLAY] {} ignored out-of-reach placement", username);
                            } else {
                                place_block(connection, version, world, registry, registration.entity_id().raw(), player_uuid, position, username, block_events).await?;
                            }
                        }
                    }
                    _ => {}
                }
                if let Some((x, y, z, yaw, pitch, on_ground)) = movement {
                    if let Some(next) = moved(location, x, y, z, yaw, pitch, on_ground) {
                        location = next;
                        registration.update_location(location);
                        let center = location.chunk();
                        update_chunk_view(connection, version, world, &mut view, center, radius).await?;
                    } else {
                        debug!("[PLAY] {} ignored invalid movement", username);
                        teleport_id = teleport_id.wrapping_add(1);
                        write_position(connection, version, teleport_id, location).await?;
                    }
                }
                trace!("[MOVE] {} {:.2},{:.2},{:.2}", username, location.x, location.y, location.z);
            }
            event = block_rx.recv() => {
                match event {
                    Ok(BlockEvent::Changed { source, position, block }) if source != player_uuid => {
                        let payload = version.block_update_payload(position, block);
                        connection
                            .write_frame(version.clientbound_id(ClientboundPacket::BlockUpdate), &payload)
                            .await?;
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!("[PLAY] {} missed {skipped} block updates", username);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            joined = joins.recv() => {
                match joined {
                    Ok(PresenceEvent::Joined(snapshot)) if snapshot.uuid != player_uuid => {
                        connection
                            .write_frame(
                                version.clientbound_id(ClientboundPacket::PlayerInfo),
                                &version.player_info_add_payload(&[(snapshot.uuid, snapshot.username.as_str())]),
                            )
                            .await?;
                        let payload = version.add_player_payload(
                            snapshot.entity_id,
                            snapshot.uuid,
                            [snapshot.location.x, snapshot.location.y, snapshot.location.z],
                            snapshot.location.yaw,
                            snapshot.location.pitch,
                        );
                        connection
                            .write_frame(version.clientbound_id(ClientboundPacket::SpawnEntity), &payload)
                            .await?;
                        tracked_positions.insert(
                            snapshot.entity_id,
                            (snapshot.location.x, snapshot.location.y, snapshot.location.z),
                        );
                    }
                    Ok(PresenceEvent::Left { entity_id, .. }) => {
                        tracked_positions.remove(&entity_id);
                        connection
                            .write_frame(
                                version.clientbound_id(ClientboundPacket::RemoveEntities),
                                &version.remove_entities_payload(&[entity_id]),
                            )
                            .await?;
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!("[PLAY] {} missed {skipped} presence events", username);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            present = presence_rx.recv() => {
                match present {
                    Ok(PresenceEvent::Moved(snapshot)) if snapshot.uuid != player_uuid => {
                        let Some(last) = tracked_positions.get(&snapshot.entity_id).copied() else {
                            continue;
                        };
                        let delta = [
                            fixed_delta(snapshot.location.x - last.0),
                            fixed_delta(snapshot.location.y - last.1),
                            fixed_delta(snapshot.location.z - last.2),
                        ];
                        let payload = version.move_entity_payload(
                            snapshot.entity_id,
                            delta,
                            snapshot.location.yaw,
                            snapshot.location.pitch,
                            snapshot.location.on_ground,
                        );
                        connection
                            .write_frame(version.clientbound_id(ClientboundPacket::EntityMove), &payload)
                            .await?;
                        tracked_positions.insert(
                            snapshot.entity_id,
                            (snapshot.location.x, snapshot.location.y, snapshot.location.z),
                        );
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!("[PLAY] {} missed {skipped} player movement events", username);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            combat = combat_rx.recv() => {
                match combat {
                    Ok(CombatEvent::Hurt { victim, attacker_entity_id: _, hurt_yaw }) => {
                        send_hurt_effects(connection, version, victim.entity_id, victim.velocity, hurt_yaw).await?;
                        if victim.uuid == player_uuid {
                            // Vanilla `set_health` for the victim's own bar.
                            connection
                                .write_frame(
                                    version.clientbound_id(ClientboundPacket::UpdateHealth),
                                    &version.update_health_payload(victim.living.health, 20, 5.0),
                                )
                                .await?;
                        }
                    }
                    Ok(CombatEvent::Death { victim, attacker_entity_id: _, hurt_yaw }) => {
                        send_hurt_effects(connection, version, victim.entity_id, victim.velocity, hurt_yaw).await?;
                        connection
                            .write_frame(
                                version.clientbound_id(ClientboundPacket::EntityEvent),
                                &version.entity_event_payload(
                                    victim.entity_id,
                                    rustbukkit_protocol::v26_3::clientbound::ENTITY_EVENT_DEATH,
                                ),
                            )
                            .await?;
                        if victim.uuid == player_uuid {
                            // Vanilla death: the victim's stacks become item
                            // entities at the death location (wire presence of
                            // item entities is N6 scope).
                            for stack in inventory.take_all() {
                                if registry.drop_from_player(stack, location).is_none() {
                                    warn!("[PLAY] {} could not drop a stack on death", username);
                                }
                            }
                            match Inventory::empty(location.selected_hotbar_slot) {
                                Some(empty) => inventory = empty,
                                None => warn!("[PLAY] {} kept the inventory after death (invalid selected slot)", username),
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!("[PLAY] {} missed {skipped} combat events", username);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            _ = keepalive.tick() => {
                if let Some((_, sent_at)) = awaiting_keepalive
                    && sent_at.elapsed() >= timeout
                {
                    bail!("keepalive timed out");
                }
                let mut writer = Writer::new();
                writer.i64(next_keepalive);
                connection
                    .write_frame(version.clientbound_id(ClientboundPacket::KeepAlive), &writer.finish())
                    .await?;
                awaiting_keepalive = Some((next_keepalive, Instant::now()));
                next_keepalive = next_keepalive.wrapping_add(1);
            }
            _ = pickup.tick() => {
                // A dead player collects nothing: the corpse must not pick up
                // its own death drops before the respawn path runs.
                let dead = registry
                    .snapshot_player(player_uuid)
                    .is_some_and(|snapshot| !snapshot.living.alive());
                if !dead && registry.pickup_near(player_uuid, &mut inventory) {
                    inventory_state_id = inventory_state_id.wrapping_add(1);
                    connection
                        .write_frame(
                            version.clientbound_id(ClientboundPacket::ContainerSetContent),
                            &version.container_set_content_payload(
                                0,
                                inventory_state_id,
                                &inventory.window(),
                                inventory.carried(),
                            ),
                        )
                        .await?;
                }
            }
        }
    }

    Ok(())
    }
    .await;

    let _ = presence_events.send(PresenceEvent::Left {
        uuid:      player_uuid,
        entity_id: registration.entity_id().raw(),
    });
    save_location(locations, player_uuid, username, location, &inventory).await;
    info!("[PLAY] {} left at {:.1},{:.1},{:.1}", username, location.x, location.y, location.z);
    outcome
}

async fn save_location(
    locations: &PlayerLocationStore,
    player_uuid: Uuid,
    username: &str,
    location: PlayerLocation,
    inventory: &Inventory,
) {
    if let Err(error) = locations.save(player_uuid, username, location, inventory).await {
        warn!("[PLAY] could not save {}: {error}", username);
    }
}

fn fixed_delta(value: f64) -> i16 {
    (value * 4096.0).round().clamp(-32_768.0, 32_767.0) as i16
}

fn moved(
    current: PlayerLocation,
    x: f64,
    y: f64,
    z: f64,
    yaw: f32,
    pitch: f32,
    on_ground: bool,
) -> Option<PlayerLocation> {
    let next = PlayerLocation {
        x,
        y,
        z,
        yaw,
        pitch,
        on_ground,
        selected_hotbar_slot: current.selected_hotbar_slot,
    };
    next.is_valid().then_some(next)
}

fn within_reach(location: PlayerLocation, position: BlockPos) -> bool {
    let dx = f64::from(position.x) + 0.5 - location.x;
    let dy = f64::from(position.y) + 0.5 - (location.y + 0.9);
    let dz = f64::from(position.z) + 0.5 - location.z;
    dx * dx + dy * dy + dz * dz <= MAX_REACH_SQUARED
}

fn adjacent_block(target: BlockPos, face: BlockFace) -> Option<BlockPos> {
    let (x, y, z) = match face {
        BlockFace::Down => (target.x, target.y - 1, target.z),
        BlockFace::Up => (target.x, target.y + 1, target.z),
        BlockFace::North => (target.x, target.y, target.z - 1),
        BlockFace::South => (target.x, target.y, target.z + 1),
        BlockFace::West => (target.x - 1, target.y, target.z),
        BlockFace::East => (target.x + 1, target.y, target.z),
    };
    BlockPos::new(x, y, z)
}

#[allow(clippy::too_many_arguments)]
async fn break_block(
    connection: &mut Connection,
    version: &'static dyn VersionPack,
    world: &World,
    registry: &SessionRegistry,
    entity_id: i32,
    player_uuid: Uuid,
    position: BlockPos,
    username: &str,
    block_events: &broadcast::Sender<BlockEvent>,
) -> Result<()> {
    let current = world.get_block(position);
    if current == BlockKind::Bedrock || current == BlockKind::Air {
        debug!("[PLAY] {} cannot break {current:?}", username);
        return Ok(());
    }
    if world.set_block_if(position, current, BlockKind::Air) {
        debug!("[PLAY] {} broke {current:?} at {:?} (entity {entity_id})", username, position);
        // The authoritative drop spawns as a world item entity; the victim's
        // and observers' sessions pick it up through the pickup interval.
        let _ = registry.drop_from_block(current, position);
        let _ = block_events.send(BlockEvent::Changed {
            source: player_uuid,
            position,
            block: BlockKind::Air,
        });
        let payload = version.block_update_payload(position, BlockKind::Air);
        connection
            .write_frame(version.clientbound_id(ClientboundPacket::BlockUpdate), &payload)
            .await?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn place_block(
    connection: &mut Connection,
    version: &'static dyn VersionPack,
    world: &World,
    registry: &SessionRegistry,
    entity_id: i32,
    player_uuid: Uuid,
    position: BlockPos,
    username: &str,
    block_events: &broadcast::Sender<BlockEvent>,
) -> Result<()> {
    if world.get_block(position) != BlockKind::Air || registry.is_block_occupied(position) {
        return Ok(());
    }
    if world.set_block_if(position, BlockKind::Air, BlockKind::GrassBlock) {
        debug!("[PLAY] {} placed grass at {:?} (entity {entity_id})", username, position);
        let _ = block_events.send(BlockEvent::Changed {
            source: player_uuid,
            position,
            block: BlockKind::GrassBlock,
        });
        let payload = version.block_update_payload(position, BlockKind::GrassBlock);
        connection
            .write_frame(version.clientbound_id(ClientboundPacket::BlockUpdate), &payload)
            .await?;
    }
    Ok(())
}

async fn write_position(
    connection: &mut Connection,
    version: &'static dyn VersionPack,
    teleport_id: i32,
    location: PlayerLocation,
) -> Result<()> {
    let payload = version.player_position_payload(
        teleport_id,
        [location.x, location.y, location.z],
        location.yaw,
        location.pitch,
    );
    connection
        .write_frame(version.clientbound_id(ClientboundPacket::PlayerPosition), &payload)
        .await?;
    Ok(())
}

/// Vanilla hurt feedback sent to every session tracking the victim, the
/// victim's own connection included: hurt animation, the hurt entity event and
/// the authoritative knockback motion. The victim additionally receives its
/// own `set_health` update at the call sites.
async fn send_hurt_effects(
    connection: &mut Connection,
    version: &'static dyn VersionPack,
    victim_entity_id: i32,
    velocity: crate::session_registry::Vec3,
    hurt_yaw: f32,
) -> Result<()> {
    connection
        .write_frame(
            version.clientbound_id(ClientboundPacket::HurtAnimation),
            &version.hurt_animation_payload(victim_entity_id, hurt_yaw),
        )
        .await?;
    connection
        .write_frame(
            version.clientbound_id(ClientboundPacket::EntityEvent),
            &version.entity_event_payload(
                victim_entity_id,
                rustbukkit_protocol::v26_3::clientbound::ENTITY_EVENT_HURT,
            ),
        )
        .await?;
    connection
        .write_frame(
            version.clientbound_id(ClientboundPacket::EntityVelocity),
            &version.entity_velocity_payload(victim_entity_id, [velocity.x, velocity.y, velocity.z]),
        )
        .await?;
    Ok(())
}

#[derive(Default)]
struct ChunkView {
    center: Option<(i32, i32)>,
    loaded: HashSet<(i32, i32)>,
}

async fn update_chunk_view(
    connection: &mut Connection,
    version: &'static dyn VersionPack,
    world: &World,
    view: &mut ChunkView,
    center: (i32, i32),
    radius: i32,
) -> Result<()> {
    if view.center != Some(center) {
        let payload = version.view_center_payload(center.0, center.1);
        connection
            .write_frame(version.clientbound_id(ClientboundPacket::ViewCenter), &payload)
            .await?;

        let keep = radius + 1;
        let stale: Vec<(i32, i32)> = view
            .loaded
            .iter()
            .copied()
            .filter(|&(x, z)| (x - center.0).abs() > keep || (z - center.1).abs() > keep)
            .collect();
        for (x, z) in stale {
            view.loaded.remove(&(x, z));
            let payload = version.forget_chunk_payload(x, z);
            connection
                .write_frame(version.clientbound_id(ClientboundPacket::UnloadChunk), &payload)
                .await?;
        }
        view.center = Some(center);
    }

    for distance in 0..=radius {
        for dx in -distance..=distance {
            for dz in -distance..=distance {
                if dx.abs().max(dz.abs()) != distance {
                    continue;
                }
                let chunk = (center.0 + dx, center.1 + dz);
                if !view.loaded.insert(chunk) {
                    continue;
                }
                let snapshot = world.snapshot_chunk(ChunkPos {
                    x: chunk.0,
                    z: chunk.1,
                });
                let payload = version.chunk_payload(&snapshot, chunk.0, chunk.1);
                connection
                    .write_frame(version.clientbound_id(ClientboundPacket::ChunkData), &payload)
                    .await?;
                debug!("[TX] {} ChunkData chunk=({},{})", connection.player_label(), chunk.0, chunk.1);
            }
        }
    }
    Ok(())
}

/// Offline-mode UUID derivation, identical to the vanilla server's offline profile.
pub fn offline_uuid(username: &str) -> Uuid {
    let digest = md5::compute(format!("OfflinePlayer:{username}").as_bytes());
    let mut bytes = digest.0;
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use rustbukkit_chunks::BlockPos;
    use rustbukkit_protocol::BlockFace;

    use super::{adjacent_block, moved, offline_uuid, within_reach};
    use crate::player_location::PlayerLocation;

    #[test]
    fn offline_uuid_matches_the_vanilla_offline_profile() {
        assert_eq!(offline_uuid("Notch").to_string(), "b50ad385-829d-3141-a216-7e7d7539ba7f");
    }

    #[test]
    fn movement_validation_rejects_non_finite_and_out_of_world_positions() {
        let spawn = PlayerLocation::spawn(64.0);
        assert!(moved(spawn, 1.0, 64.0, 1.0, 0.0, 0.0, true).is_some());
        assert!(moved(spawn, f64::NAN, 64.0, 1.0, 0.0, 0.0, true).is_none());
        assert!(moved(spawn, 1.0, -65.0, 1.0, 0.0, 0.0, true).is_none());
        assert!(moved(spawn, 30_000_000.0, 64.0, 1.0, 0.0, 0.0, true).is_none());
    }

    #[test]
    fn placement_targets_the_adjacent_block_and_checks_reach() {
        let target = BlockPos::new(0, 100, 0).unwrap();
        assert_eq!(adjacent_block(target, BlockFace::Up), BlockPos::new(0, 101, 0));
        assert_eq!(adjacent_block(target, BlockFace::Down), BlockPos::new(0, 99, 0));
        assert_eq!(adjacent_block(target, BlockFace::East), BlockPos::new(1, 100, 0));
        let near = PlayerLocation::spawn(100.0);
        assert!(within_reach(near, BlockPos::new(2, 100, 2).unwrap()));
        assert!(!within_reach(near, BlockPos::new(40, 100, 0).unwrap()));
    }
}
