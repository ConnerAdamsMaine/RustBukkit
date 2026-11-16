use std::collections::HashSet;

use rustbukkit_chunks::BlockPos;
use rustbukkit_encoding::{MAX_PACKET_LENGTH, Writer};
use rustbukkit_inventory::ItemKind;
use thiserror::Error;
use uuid::Uuid;

use super::registry::{NbtEncodeError, NbtTag, encode_network_nbt};
use super::{MAX_UTF16_UNITS, parse_identifier};

pub const PLAY_LOGIN_ID: i32 = 50;
pub const PLAYER_POSITION_ID: i32 = 73;
pub const RESPAWN_ID: i32 = 84;
pub const SPAWN_POSITION_ID: i32 = 99;
pub const TIME_ID: i32 = 115;

// 26.3 combat/health clientbound IDs from the generated `packets.json` report
// (`registry_data/26.3/packet_ids.json`); asserted by `tests/protocol_ids_26_3.rs`.
pub const ENTITY_EVENT_ID: i32 = 34;
pub const HURT_ANIMATION_ID: i32 = 43;
pub const SET_ENTITY_MOTION_ID: i32 = 103;
pub const SET_HEALTH_ID: i32 = 106;

/// Vanilla Entity Event codes for the two combat states a living player can
/// broadcast: hurt (tilt/sound) and death.
pub const ENTITY_EVENT_HURT: i8 = 2;
pub const ENTITY_EVENT_DEATH: i8 = 3;

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(i32)]
pub enum GameMode {
    Survival = 0,
    Creative = 1,
    Adventure = 2,
    Spectator = 3,
}

#[derive(Debug, Clone, Copy)]
pub struct GlobalPosition<'a> {
    pub dimension: &'a str,
    pub position:  BlockPos,
}

#[derive(Debug, Clone, Copy)]
pub struct CommonSpawnInfo<'a> {
    pub dimension_type_id:   i32,
    pub dimension:           &'a str,
    pub seed:                i64,
    pub game_mode:           GameMode,
    pub previous_game_mode:  Option<GameMode>,
    pub is_debug:            bool,
    pub is_flat:             bool,
    pub last_death_location: Option<GlobalPosition<'a>>,
    pub portal_cooldown:     i32,
    pub sea_level:           i32,
}

#[derive(Debug, Clone, Copy)]
pub struct PlayLogin<'a> {
    pub entity_id:            i32,
    pub hardcore:             bool,
    pub dimensions:           &'a [&'a str],
    pub max_players:          i32,
    pub view_distance:        i32,
    pub simulation_distance:  i32,
    pub reduced_debug_info:   bool,
    pub show_death_screen:    bool,
    pub limited_crafting:     bool,
    pub spawn:                CommonSpawnInfo<'a>,
    pub online_mode:          bool,
    pub enforces_secure_chat: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerPosition {
    pub teleport_id:    i32,
    pub position:       [f64; 3],
    pub delta:          [f64; 3],
    pub yaw:            f32,
    pub pitch:          f32,
    pub relative_flags: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockUpdate {
    pub registry_id:  i32,
    pub ticks:        i64,
    pub partial_tick: f32,
    pub rate:         f32,
}

#[derive(Debug, Error, PartialEq)]
pub enum ClientboundError {
    #[error("invalid or oversized dimension identifier")]
    InvalidDimension,
    #[error("duplicate dimension identifier")]
    DuplicateDimension,
    #[error("dimension type registry ID must be nonnegative: {0}")]
    InvalidDimensionTypeId(i32),
    #[error("spawn coordinates exceed the packed position range")]
    InvalidPosition,
    #[error("clock registry ID must be nonnegative: {0}")]
    InvalidClockId(i32),
    #[error("duplicate clock registry ID: {0}")]
    DuplicateClockId(i32),
    #[error("packet exceeds the maximum frame length")]
    PacketTooLarge,
    #[error("health {0} is not a finite, non-negative value")]
    InvalidHealth(f32),
    #[error(transparent)]
    Nbt(#[from] NbtEncodeError),
}

pub fn encode_play_login(login: PlayLogin<'_>) -> Result<Vec<u8>, ClientboundError> {
    const PACKET_ID_BYTES: usize = 1;
    const MIN_DIMENSION_BYTES: usize = 3;

    if login.dimensions.len() > (MAX_PACKET_LENGTH - PACKET_ID_BYTES) / MIN_DIMENSION_BYTES {
        return Err(ClientboundError::PacketTooLarge);
    }

    let spawn = validate_spawn_info(login.spawn)?;
    let mut payload_len = 4
        + 1
        + variable_length(login.dimensions.len() as u64)
        + varint_length(login.max_players)
        + varint_length(login.view_distance)
        + varint_length(login.simulation_distance)
        + 3
        + spawn_info_length(&spawn)
        + 2;
    let mut dimensions = Vec::with_capacity(login.dimensions.len());
    let mut seen = HashSet::with_capacity(login.dimensions.len());
    for dimension in login.dimensions {
        let identifier = checked_identifier(dimension)?;
        if !seen.insert(identifier) {
            return Err(ClientboundError::DuplicateDimension);
        }
        payload_len += identifier_length(identifier);
        if payload_len + PACKET_ID_BYTES > MAX_PACKET_LENGTH {
            return Err(ClientboundError::PacketTooLarge);
        }
        dimensions.push(identifier);
    }
    if payload_len + PACKET_ID_BYTES > MAX_PACKET_LENGTH {
        return Err(ClientboundError::PacketTooLarge);
    }

    let mut writer = Writer::with_capacity(payload_len);
    writer.i32(login.entity_id);
    writer.bool(login.hardcore);
    writer.varint(dimensions.len() as i32);
    for dimension in dimensions {
        write_identifier(&mut writer, dimension);
    }
    writer.varint(login.max_players);
    writer.varint(login.view_distance);
    writer.varint(login.simulation_distance);
    writer.bool(login.reduced_debug_info);
    writer.bool(login.show_death_screen);
    writer.bool(login.limited_crafting);
    write_spawn_info(&mut writer, &spawn);
    writer.bool(login.online_mode);
    writer.bool(login.enforces_secure_chat);
    Ok(writer.finish())
}

pub fn encode_respawn(info: CommonSpawnInfo<'_>, data_to_keep: u8) -> Result<Vec<u8>, ClientboundError> {
    let spawn = validate_spawn_info(info)?;
    let mut writer = Writer::with_capacity(spawn_info_length(&spawn) + 1);
    write_spawn_info(&mut writer, &spawn);
    writer.u8(data_to_keep);
    Ok(writer.finish())
}

/// Encode the 26.3 Server Data packet: a network-NBT text Component (unnamed
/// root tag) followed by `Optional<byte[]>` icon PNG data.
///
/// Captured official payload for MOTD "VanillaProbe" and no icon is
/// `08 00 0c "VanillaProbe" 00` (`registry_data/26.3/golden_packets.json`).
pub fn encode_server_data(motd: &NbtTag, icon: Option<&[u8]>) -> Result<Vec<u8>, ClientboundError> {
    let mut writer = Writer::new();
    writer.raw(&encode_network_nbt(motd, MAX_PACKET_LENGTH)?);
    match icon {
        None => writer.bool(false),
        Some(bytes) => {
            writer.bool(true);
            let len = i32::try_from(bytes.len()).map_err(|_| ClientboundError::PacketTooLarge)?;
            writer.varint(len);
            writer.raw(bytes);
        }
    }
    Ok(writer.finish())
}

/// `minecraft:player` in the 26.3 entity-type registry, read from the official
/// server's `add_entity` packet (`registry_data/26.3/golden_packets.json`).
pub const PLAYER_ENTITY_TYPE_ID: i32 = 120;

/// Encode a Player Info Update adding players to the tab list.
///
/// Action mask `0x09` = ADD_PLAYER | UPDATE_LISTED. The official 26.3 server
/// encodes the actions as an EnumSet bitmask, then a VarInt count, and each
/// entry's fields in action order: UUID, name, profile properties, listed.
pub fn encode_player_info_add(entries: &[(Uuid, &str)]) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.u8(0x09);
    writer.varint(entries.len() as i32);
    for (uuid, username) in entries {
        writer.uuid(*uuid);
        writer.string(username);
        writer.varint(0); // GameProfile property count
        writer.bool(true); // listed in the tab list
    }
    writer.finish()
}

/// Encode Add Entity for a player avatar.
///
/// 26.3 layout: entity id, UUID, entity type, absolute position, movement Vec3
/// (three shorts), x/y/head rotation bytes, data VarInt.
pub fn encode_add_player(entity_id: i32, uuid: Uuid, position: [f64; 3], yaw: f32, pitch: f32) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.varint(entity_id);
    writer.uuid(uuid);
    writer.varint(PLAYER_ENTITY_TYPE_ID);
    for coordinate in position {
        writer.f64(coordinate);
    }
    // Movement is `Vec3.LP_STREAM_CODEC` (`LpVec3`): a zero vector is the single
    // byte 0x00, otherwise a 6-byte quantized encoding. Writing three zero
    // shorts leaves 5 trailing bytes and makes a real client reject the packet.
    writer.u8(0);
    writer.u8(rotation_byte(pitch));
    writer.u8(rotation_byte(yaw));
    writer.u8(rotation_byte(yaw));
    writer.varint(0);
    writer.finish()
}

pub fn encode_remove_entities(entity_ids: &[i32]) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.varint(entity_ids.len() as i32);
    for entity_id in entity_ids {
        writer.varint(*entity_id);
    }
    writer.finish()
}

/// Encode Move Entity Pos & Rot with fixed-point deltas (1/4096 block units).
///
/// 26.3 layout: entity id, packed properties (on-ground flag | step count << 1),
/// a linear `VecDelta` (three shorts when the step count is zero), then yRot and
/// xRot bytes. The older `entity id, shorts, yaw, pitch, on-ground bool` layout
/// is rejected by an unmodified client.
pub fn encode_move_entity(entity_id: i32, delta: [i16; 3], yaw: f32, pitch: f32, on_ground: bool) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.varint(entity_id);
    writer.varint(i32::from(on_ground));
    for component in delta {
        writer.i16(component);
    }
    writer.u8(rotation_byte(yaw));
    writer.u8(rotation_byte(pitch));
    writer.finish()
}

/// Convert degrees to the byte rotation used by 26.3 entity packets.
pub fn rotation_byte(degrees: f32) -> u8 {
    let normalized = if degrees.is_finite() {
        let value = degrees * 256.0 / 360.0;
        let rounded = value.round();
        if rounded < 0.0 {
            (rounded % 256.0 + 256.0) % 256.0
        } else {
            rounded % 256.0
        }
    } else {
        0.0
    };
    normalized as u8
}

pub fn encode_player_position(position: PlayerPosition) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.varint(position.teleport_id);
    for coordinate in position.position {
        writer.f64(coordinate);
    }
    for coordinate in position.delta {
        writer.f64(coordinate);
    }
    writer.f32(position.yaw);
    writer.f32(position.pitch);
    writer.i32(position.relative_flags);
    writer.finish()
}

pub fn encode_spawn_position(
    dimension: &str,
    position: BlockPos,
    yaw: f32,
    pitch: f32,
) -> Result<Vec<u8>, ClientboundError> {
    let identifier = checked_identifier(dimension)?;
    validate_position(position)?;
    let mut writer = Writer::new();
    write_identifier(&mut writer, identifier);
    writer.position(position.x, position.y, position.z);
    writer.f32(yaw);
    writer.f32(pitch);
    Ok(writer.finish())
}

type Identifier<'a> = (&'a str, &'a str);

struct ValidatedSpawnInfo<'a> {
    info:       CommonSpawnInfo<'a>,
    dimension:  Identifier<'a>,
    last_death: Option<(Identifier<'a>, BlockPos)>,
}

fn checked_identifier(value: &str) -> Result<Identifier<'_>, ClientboundError> {
    let identifier = parse_identifier(value).ok_or(ClientboundError::InvalidDimension)?;
    if identifier.0.len() + 1 + identifier.1.len() > MAX_UTF16_UNITS {
        return Err(ClientboundError::InvalidDimension);
    }
    Ok(identifier)
}

fn identifier_length(identifier: Identifier<'_>) -> usize {
    let length = identifier.0.len() + 1 + identifier.1.len();
    variable_length(length as u64) + length
}

fn write_identifier(writer: &mut Writer, identifier: Identifier<'_>) {
    writer.varint((identifier.0.len() + 1 + identifier.1.len()) as i32);
    writer.raw(identifier.0.as_bytes());
    writer.u8(b':');
    writer.raw(identifier.1.as_bytes());
}

fn validate_position(position: BlockPos) -> Result<(), ClientboundError> {
    if !(-33_554_432..=33_554_431).contains(&position.x)
        || !(-2_048..=2_047).contains(&position.y)
        || !(-33_554_432..=33_554_431).contains(&position.z)
    {
        return Err(ClientboundError::InvalidPosition);
    }
    Ok(())
}

fn validate_spawn_info(info: CommonSpawnInfo<'_>) -> Result<ValidatedSpawnInfo<'_>, ClientboundError> {
    if info.dimension_type_id < 0 {
        return Err(ClientboundError::InvalidDimensionTypeId(info.dimension_type_id));
    }
    let dimension = checked_identifier(info.dimension)?;
    let last_death = match info.last_death_location {
        Some(location) => {
            let dimension = checked_identifier(location.dimension)?;
            validate_position(location.position)?;
            Some((dimension, location.position))
        }
        None => None,
    };
    Ok(ValidatedSpawnInfo {
        info,
        dimension,
        last_death,
    })
}

fn spawn_info_length(spawn: &ValidatedSpawnInfo<'_>) -> usize {
    varint_length(spawn.info.dimension_type_id)
        + identifier_length(spawn.dimension)
        + 8
        + 1
        + 1
        + 2
        + 1
        + spawn
            .last_death
            .map_or(0, |(dimension, _)| identifier_length(dimension) + 8)
        + varint_length(spawn.info.portal_cooldown)
        + varint_length(spawn.info.sea_level)
}

fn write_spawn_info(writer: &mut Writer, spawn: &ValidatedSpawnInfo<'_>) {
    let info = spawn.info;
    writer.varint(info.dimension_type_id);
    write_identifier(writer, spawn.dimension);
    writer.i64(info.seed);
    writer.varint(info.game_mode as i32);
    writer.varint(info.previous_game_mode.map_or(0, |mode| mode as i32 + 1));
    writer.bool(info.is_debug);
    writer.bool(info.is_flat);
    writer.bool(spawn.last_death.is_some());
    if let Some((dimension, position)) = spawn.last_death {
        write_identifier(writer, dimension);
        writer.position(position.x, position.y, position.z);
    }
    writer.varint(info.portal_cooldown);
    writer.varint(info.sea_level);
}

fn varint_length(value: i32) -> usize {
    variable_length(value as u32 as u64)
}

pub fn encode_time(game_time: i64, clocks: &[ClockUpdate]) -> Result<Vec<u8>, ClientboundError> {
    const TIME_ID_BYTES: usize = 1;
    const MIN_CLOCK_BYTES: usize = 10;

    if clocks.len() > (MAX_PACKET_LENGTH - TIME_ID_BYTES - 8 - 1) / MIN_CLOCK_BYTES {
        return Err(ClientboundError::PacketTooLarge);
    }

    let mut ids = HashSet::with_capacity(clocks.len());
    let mut payload_len = 8 + variable_length(clocks.len() as u64);
    for clock in clocks {
        if clock.registry_id < 0 {
            return Err(ClientboundError::InvalidClockId(clock.registry_id));
        }
        if !ids.insert(clock.registry_id) {
            return Err(ClientboundError::DuplicateClockId(clock.registry_id));
        }
        payload_len += variable_length(clock.registry_id as u64) + variable_length(clock.ticks as u64) + 8;
        if payload_len + TIME_ID_BYTES > MAX_PACKET_LENGTH {
            return Err(ClientboundError::PacketTooLarge);
        }
    }

    let mut writer = Writer::with_capacity(payload_len);
    writer.i64(game_time);
    writer.varint(clocks.len() as i32);
    for clock in clocks {
        writer.varint(clock.registry_id);
        writer.varlong(clock.ticks);
        writer.f32(clock.partial_tick);
        writer.f32(clock.rate);
    }
    Ok(writer.finish())
}

fn variable_length(mut value: u64) -> usize {
    let mut bytes = 1;
    while value > 0x7f {
        value >>= 7;
        bytes += 1;
    }
    bytes
}
pub const BLOCK_UPDATE_ID: i32 = 8;

// 26.3 play clientbound IDs (generated `packets.json` report); asserted by
// `tests/protocol_ids_26_3.rs`.
pub const CONTAINER_CLOSE_ID: i32 = 17;
pub const CONTAINER_SET_CONTENT_ID: i32 = 18;
pub const CONTAINER_SET_SLOT_ID: i32 = 20;
pub const SET_CURSOR_ITEM_ID: i32 = 98;
pub const SET_HELD_SLOT_ID: i32 = 107;

/// 26.3 item registry IDs, captured from the official server by encoding
/// `Items.GRASS_BLOCK/DIRT/STONE/COBBLESTONE/SAND` with the official codecs and
/// reading the encoded VarInt (`target/probe/CodecProbe`, 2026-11-10). The
/// client resolves item IDs from its own built-in registry, which uses the same
/// fixed order, so these constants are universal for vanilla 26.3 clients.
pub fn item_registry_id(kind: ItemKind) -> i32 {
    match kind {
        ItemKind::GrassBlock => 54,
        ItemKind::Dirt => 55,
        ItemKind::Stone => 1,
        ItemKind::Cobblestone => 62,
        ItemKind::Sand => 88,
    }
}

/// Encode one network Slot: a VarInt count; when the count is non-zero, a
/// VarInt item ID, a VarInt added-component count and a VarInt removed-component
/// count (the 26.3 encoding has no presence boolean; `0` alone means empty).
///
/// Official probe bytes: dirt ×3 encodes as `03 37 00 00`; empty is `00`.
fn write_slot(writer: &mut Writer, stack: Option<(i32, u8)>) {
    match stack {
        None => writer.varint(0),
        Some((item_id, count)) => {
            writer.varint(i32::from(count));
            writer.varint(item_id);
            writer.varint(0); // added components
            writer.varint(0); // removed components
        }
    }
}

fn item_stack(stack: Option<rustbukkit_inventory::ItemStack>) -> Option<(i32, u8)> {
    stack.map(|stack| (item_registry_id(stack.kind()), stack.count()))
}

/// Encode `container_set_content` (ID 18): VarInt container ID, VarInt state
/// ID, VarInt count, the ordered Slot list, then the carried Slot.
///
/// Official probe (window 0, state 5, dirt ×3 + stone ×1, carried dirt ×3):
/// `00 05 02 03 37 00 00 01 01 00 00 03 37 00 00`.
pub fn encode_container_set_content(
    container_id: i32,
    state_id: i32,
    items: &[Option<rustbukkit_inventory::ItemStack>],
    carried: Option<rustbukkit_inventory::ItemStack>,
) -> Result<Vec<u8>, ClientboundError> {
    let mut writer = Writer::with_capacity(items.len() * 4 + 8);
    writer.varint(container_id);
    writer.varint(state_id);
    writer.varint(items.len() as i32);
    for stack in items {
        write_slot(&mut writer, item_stack(*stack));
    }
    write_slot(&mut writer, item_stack(carried));
    Ok(writer.finish())
}

/// Encode `container_set_slot` (ID 20): VarInt container ID, VarInt state ID,
/// signed short slot index, then one Slot.
///
/// Official probe (window 0, state 7, slot 36, dirt ×3):
/// `00 07 00 24 03 37 00 00`.
pub fn encode_container_set_slot(
    container_id: i32,
    state_id: i32,
    slot: i16,
    stack: Option<rustbukkit_inventory::ItemStack>,
) -> Vec<u8> {
    let mut writer = Writer::with_capacity(12);
    writer.varint(container_id);
    writer.varint(state_id);
    writer.i16(slot);
    write_slot(&mut writer, item_stack(stack));
    writer.finish()
}

/// Encode `set_cursor_item` (ID 98): a single Slot.
///
/// Official probe: dirt ×3 is `03 37 00 00`, empty is `00`.
pub fn encode_set_cursor_item(stack: Option<rustbukkit_inventory::ItemStack>) -> Vec<u8> {
    let mut writer = Writer::with_capacity(8);
    write_slot(&mut writer, item_stack(stack));
    writer.finish()
}

/// Encode `set_held_slot` (ID 107): the selected hotbar slot as a VarInt.
///
/// Official probe: slot 4 encodes as the single byte `04`.
pub fn encode_set_held_slot(slot: u8) -> Vec<u8> {
    let mut writer = Writer::with_capacity(1);
    writer.varint(i32::from(slot));
    writer.finish()
}

/// Encode `container_close` (ID 17): the closed container ID as a VarInt.
pub fn encode_container_close(container_id: i32) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.varint(container_id);
    writer.finish()
}

/// Encode the Set Health packet (ID 106 in the 26.3 Play state).
///
/// Layout: f32 health, VarInt food level, f32 saturation. The vanilla client
/// shows the death screen once its synced health reaches zero.
///
/// HorizonX layout: `SetHealth { health: f32, food: i32, saturation: f32 }`.
pub fn encode_set_health(health: f32, food: i32, saturation: f32) -> Result<Vec<u8>, ClientboundError> {
    if !health.is_finite() || health < 0.0 {
        return Err(ClientboundError::InvalidHealth(health));
    }
    let mut writer = Writer::with_capacity(9);
    writer.f32(health);
    writer.varint(food);
    writer.f32(saturation);
    Ok(writer.finish())
}

/// Encode the Hurt Animation packet (ID 43 in the 26.3 Play state).
///
/// Layout: VarInt entity id, f32 yaw of the hurt tilt. Broadcast to every
/// client that can see the entity, the victim included.
///
/// HorizonX layout: `HurtAnimation { id: i32 => VarInt, yaw: f32 }`.
pub fn encode_hurt_animation(entity_id: i32, yaw: f32) -> Vec<u8> {
    let mut writer = Writer::with_capacity(5);
    writer.varint(entity_id);
    writer.f32(yaw);
    writer.finish()
}

/// Encode the Entity Event packet (ID 34 in the 26.3 Play state).
///
/// Layout: i32 entity id, i8 event code. Vanilla event 2 marks a living
/// entity as hurt (`SharedMonsterEvent`-style hurt broadcast).
///
/// HorizonX layout: `EntityEvent { id: i32 => Int, event: i8 }`.
pub fn encode_entity_event(entity_id: i32, event: i8) -> Vec<u8> {
    let mut writer = Writer::with_capacity(5);
    writer.i32(entity_id);
    writer.i8(event);
    writer.finish()
}

/// Encode the Set Entity Motion packet (ID 103 in the 26.3 Play state).
///
/// Layout: VarInt entity id, then the velocity as `Vec3.LP_STREAM_CODEC`
/// (LpVec3). Used to push knockback computed by the authoritative server to
/// the victim's client.
///
/// HorizonX layout: `SetEntityMotion { id: i32 => VarInt, velocity: [f64; 3] => LpVec3 }`.
pub fn encode_set_entity_motion(entity_id: i32, velocity: [f64; 3]) -> Vec<u8> {
    let mut writer = Writer::with_capacity(8);
    writer.varint(entity_id);
    write_lp_vec3(&mut writer, velocity);
    writer.finish()
}

/// The 26.3 `Vec3.LP_STREAM_CODEC` (LpVec3): one, five or nine bytes encoding
/// three f64 velocity components with a shared per-axis scale. Ported from
/// HorizonX `horizon-protocol/src/buf.rs` (`LpVec3::write`/`read`), which
/// mirrors the vanilla `ByteToLongStreamCodec` packing.
mod lp_vec3 {
    #[cfg(test)]
    fn unpack(v: u64) -> f64 {
        ((v & 32767) as f64).min(32766.0) * 2.0 / 32766.0 - 1.0
    }

    fn pack(v: f64) -> u64 {
        ((v * 0.5 + 0.5) * 32766.0).round() as u64
    }

    fn sanitize(v: f64) -> f64 {
        if v.is_nan() {
            0.0
        } else {
            v.clamp(-1.717_986_918_3e10, 1.717_986_918_3e10)
        }
    }

    /// Quantize a velocity the way a client decodes it after the wire write.
    /// Test-only; kept beside `write` so the codec's decode semantics cannot
    /// drift away from the vanilla round trip.
    #[cfg(test)]
    pub fn quantize(v: [f64; 3]) -> [f64; 3] {
        let mut writer = rustbukkit_encoding::Writer::new();
        write(&mut writer, v);
        let bytes = writer.finish();
        let mut reader = rustbukkit_decoding::Reader::new(&bytes);
        read(&mut reader).unwrap_or([0.0; 3])
    }

    pub fn write(writer: &mut rustbukkit_encoding::Writer, v: [f64; 3]) {
        let [x, y, z] = v.map(sanitize);
        let length = x.abs().max(y.abs()).max(z.abs());
        // Below the smallest representable step the client reads a zero vector.
        if length < 3.051_944_088_384_301e-5 {
            writer.u8(0);
            return;
        }
        let scale = length.ceil() as u64;
        let partial = scale & 3 != scale;
        let markers = if partial { scale & 3 | 4 } else { scale };
        let s = scale as f64;
        let buffer = markers | pack(x / s) << 3 | pack(y / s) << 18 | pack(z / s) << 33;
        writer.u8(buffer as u8);
        writer.u8((buffer >> 8) as u8);
        writer.i32((buffer >> 16) as i32);
        if partial {
            writer.varint((scale >> 2) as i32);
        }
    }

    /// Decode back what `write` produced (used by round-trip tests only).
    #[cfg(test)]
    pub fn read(reader: &mut rustbukkit_decoding::Reader<'_>) -> Option<[f64; 3]> {
        let lowest = u64::from(reader.u8().ok()?);
        if lowest == 0 {
            return Some([0.0; 3]);
        }
        let middle = u64::from(reader.u8().ok()?);
        let highest = u64::from(u32::from_be_bytes(reader.raw(4).ok()?.try_into().ok()?));
        let buffer = highest << 16 | middle << 8 | lowest;
        let mut scale = lowest & 3;
        if lowest & 4 == 4 {
            scale |= u64::try_from(reader.varint().ok()?).ok()? << 2;
        }
        let s = scale as f64;
        Some([
            unpack(buffer >> 3) * s,
            unpack(buffer >> 18) * s,
            unpack(buffer >> 33) * s,
        ])
    }
}

fn write_lp_vec3(writer: &mut Writer, velocity: [f64; 3]) {
    lp_vec3::write(writer, velocity);
}

/// Encode a Block Update packet (ID 8 in the 26.3 Play state)
/// Sent when a block changes in the world
pub fn encode_block_update(position: BlockPos, block_state_id: i32) -> Vec<u8> {
    let mut writer = Writer::new();

    // Position is packed as: x << 38 | (z & 0x3FFFFFF) << 12 | (y & 0xFFF)
    let packed_x = (position.x as i64) & 0x3FFFFFF;
    let packed_y = (position.y as i64) & 0xFFF;
    let packed_z = (position.z as i64) & 0x3FFFFFF;
    let packed = (packed_x << 38) | (packed_z << 12) | packed_y;

    writer.varlong(packed);
    writer.varint(block_state_id);

    writer.finish()
}

#[cfg(test)]
mod tests {
    use rustbukkit_chunks::BlockPos;
    use rustbukkit_encoding::encode_frame;

    use super::{
        BLOCK_UPDATE_ID,
        ClientboundError,
        ENTITY_EVENT_HURT,
        encode_add_player,
        encode_block_update,
        encode_container_close,
        encode_container_set_content,
        encode_container_set_slot,
        encode_entity_event,
        encode_hurt_animation,
        encode_player_info_add,
        encode_set_cursor_item,
        encode_set_entity_motion,
        encode_set_health,
        encode_set_held_slot,
        lp_vec3,
        rotation_byte,
    };

    #[test]
    fn player_info_and_add_entity_match_the_captured_26_3_layout() {
        let uuid = uuid::Uuid::from_u128(0x0011_2233_4455_6677_8899_aabb_ccdd_eeff);
        let info = encode_player_info_add(&[(uuid, "Probe")]);
        let mut expected = vec![0x09, 0x01];
        expected.extend_from_slice(uuid.as_bytes());
        expected.extend_from_slice(&[0x05, b'P', b'r', b'o', b'b', b'e', 0x00, 0x01]);
        assert_eq!(info, expected);

        let add = encode_add_player(74, uuid, [8.2, -58.82, -32.09], 90.0, 0.0);
        assert_eq!(add.len(), 47);
        assert_eq!(add[0], 74);
        assert_eq!(add[17], 120);
        assert_eq!(*add.last().unwrap(), 0);
        assert_eq!(rotation_byte(90.0), 64);
    }

    #[test]
    fn block_update_encodes_correctly() {
        let pos = BlockPos::new(100, 64, -50).unwrap();
        let payload = encode_block_update(pos, 5); // 5 = grass_block

        // Verify we can encode it into a frame
        let frame = encode_frame(BLOCK_UPDATE_ID, &payload).unwrap();
        assert!(!frame.is_empty());
    }

    #[test]
    fn container_packets_match_the_official_26_3_codec_bytes() {
        // Captured by encoding the official 26.3 codecs with the official
        // server jar (`target/probe/CodecProbe.java`, 2026-11-10); see
        // `registry_data/26.3/README.md`.
        use rustbukkit_inventory::{ItemKind, ItemStack};

        let dirt = ItemStack::new(ItemKind::Dirt, 3).unwrap();
        let stone = ItemStack::new(ItemKind::Stone, 1).unwrap();

        let content = encode_container_set_content(0, 5, &[Some(dirt), Some(stone)], Some(dirt)).unwrap();
        assert_eq!(
            content,
            [
                0x00, 0x05, 0x02, 0x03, 0x37, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x03, 0x37, 0x00, 0x00
            ]
        );

        let empty = encode_container_set_content(0, 0, &[None, None], None).unwrap();
        assert_eq!(empty, [0x00, 0x00, 0x02, 0x00, 0x00, 0x00]);

        let slot = encode_container_set_slot(0, 7, 36, Some(dirt));
        assert_eq!(slot, [0x00, 0x07, 0x00, 0x24, 0x03, 0x37, 0x00, 0x00]);

        assert_eq!(encode_set_cursor_item(Some(dirt)), [0x03, 0x37, 0x00, 0x00]);
        assert_eq!(encode_set_cursor_item(None), [0x00]);
        assert_eq!(encode_set_held_slot(4), [0x04]);
        assert_eq!(encode_container_close(0), [0x00]);
    }

    #[test]
    fn set_health_encodes_health_food_and_saturation() {
        let payload = encode_set_health(17.5, 18, 3.0).unwrap();
        assert_eq!(payload, [0x41, 0x8C, 0x00, 0x00, 0x12, 0x40, 0x40, 0x00, 0x00]);
        // Zero health is the legal death sync; negative or non-finite is not.
        assert!(encode_set_health(0.0, 20, 5.0).is_ok());
        assert!(matches!(encode_set_health(-1.0, 20, 5.0), Err(ClientboundError::InvalidHealth(_))));
        assert!(matches!(encode_set_health(f32::NAN, 20, 5.0), Err(ClientboundError::InvalidHealth(_))));
    }

    #[test]
    fn hurt_animation_and_entity_event_match_the_26_3_layouts() {
        // VarInt entity id + BE f32 tilt yaw.
        assert_eq!(encode_hurt_animation(74, 90.0), [0x4A, 0x42, 0xB4, 0x00, 0x00]);
        // BE i32 entity id + Byte event code (vanilla hurt event is 2).
        assert_eq!(encode_entity_event(2, ENTITY_EVENT_HURT), [0x00, 0x00, 0x00, 0x02, 0x02]);
    }

    #[test]
    fn set_entity_motion_encodes_the_lp_vec3_velocity_codec() {
        // Zero velocity is the single-byte LpVec3 marker.
        assert_eq!(encode_set_entity_motion(2, [0.0; 3]), [0x02, 0x00]);
        // Non-finite components sanitize to zero like the vanilla codec.
        assert_eq!(encode_set_entity_motion(2, [f64::NAN, 0.0, 0.0]), [0x02, 0x00]);
        // Knockback-scale velocity: scale 1, no partial varint.
        assert_eq!(encode_set_entity_motion(2, [0.0, 0.4, 0.0]), [0x02, 0xF9, 0xFF, 0x7F, 0xFF, 0x66, 0x61]);
        // Velocity above 3 forces the partial-scale VarInt tail.
        assert_eq!(
            encode_set_entity_motion(2, [10.0, 0.0, 0.0]),
            [0x02, 0xF6, 0xFF, 0x7F, 0xFE, 0xFF, 0xFF, 0x02]
        );
    }

    #[test]
    fn lp_vec3_round_trips_within_the_vanilla_precision() {
        for velocity in [
            [0.0, 0.0, 0.0],
            [0.1, -0.2, 0.3],
            [0.0, 0.4, 0.4],
            [1.5, -3.25, 0.0],
            [12.0, 0.0, -40.5],
            [0.0, -3.92, 0.0],
        ] {
            let quantized = lp_vec3::quantize(velocity);
            let scale = velocity
                .iter()
                .fold(0.0f64, |m, c| m.max(c.abs()))
                .ceil()
                .max(1.0);
            for (encoded, original) in quantized.into_iter().zip(velocity) {
                assert!((encoded - original).abs() <= scale / 32766.0 * 1.01, "{velocity:?}");
            }
        }
        // A near-zero vector collapses to the single zero marker byte.
        let mut writer = rustbukkit_encoding::Writer::new();
        lp_vec3::write(&mut writer, [0.0, 1e-9, 0.0]);
        assert_eq!(writer.finish(), [0]);
    }
}
