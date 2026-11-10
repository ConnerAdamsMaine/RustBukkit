use std::collections::HashSet;

use rustbukkit_chunks::BlockPos;
use rustbukkit_encoding::{MAX_PACKET_LENGTH, Writer};
use thiserror::Error;

use super::{MAX_UTF16_UNITS, parse_identifier};

pub const PLAY_LOGIN_ID: i32 = 50;
pub const PLAYER_POSITION_ID: i32 = 73;
pub const RESPAWN_ID: i32 = 84;
pub const SPAWN_POSITION_ID: i32 = 99;
pub const TIME_ID: i32 = 115;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Error, PartialEq, Eq)]
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

    use super::{BLOCK_UPDATE_ID, encode_block_update};

    #[test]
    fn block_update_encodes_correctly() {
        let pos = BlockPos::new(100, 64, -50).unwrap();
        let payload = encode_block_update(pos, 5); // 5 = grass_block

        // Verify we can encode it into a frame
        let frame = encode_frame(BLOCK_UPDATE_ID, &payload).unwrap();
        assert!(!frame.is_empty());
    }
}
