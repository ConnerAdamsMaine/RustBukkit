use rustbukkit_chunks::{BlockKind, BlockPos, ChunkSnapshot, FlatWorld};
use rustbukkit_decoding::Reader;
use rustbukkit_encoding::Writer;
use rustbukkit_inventory::{Inventory, InventoryAction, ItemKind, ItemStack};

use crate::nbt::{Tag, encode_root};
use crate::version::{
    BlockFace,
    ClientboundPacket,
    DigAction,
    EntityVelocity,
    ItemEntitySpawn,
    LoginError,
    PlayDecodeError,
    PlayInbound,
    PresenceChange,
    validate_username,
};

const OVERWORLD: &str = "minecraft:overworld";
const PLAINS_BIOME_ID: i32 = 1;

pub fn parse_login_start(payload: &[u8]) -> Result<String, LoginError> {
    let mut reader = Reader::new(payload);
    let username = reader.string(16)?;
    validate_username(username)?;
    if reader.bool()? {
        reader.i64()?;
        let key_length = reader.varint()?;
        if !(0..=8192).contains(&key_length) {
            return Err(LoginError::InvalidPayload);
        }
        reader.raw(key_length as usize)?;
        let signature_length = reader.varint()?;
        if !(0..=8192).contains(&signature_length) {
            return Err(LoginError::InvalidPayload);
        }
        reader.raw(signature_length as usize)?;
    }
    if reader.bool()? {
        reader.uuid()?;
    }
    if reader.remaining() != 0 {
        return Err(LoginError::InvalidPayload);
    }
    Ok(username.to_owned())
}

pub fn parse_play(id: i32, payload: &[u8]) -> Result<PlayInbound, PlayDecodeError> {
    let mut reader = Reader::new(payload);
    let packet = match id {
        0x00 => PlayInbound::TeleportConfirm(reader.varint()?),
        0x07 => {
            match reader.varint()? {
                0 => PlayInbound::RespawnRequest,
                1 => PlayInbound::Other(id),
                _ => return Err(PlayDecodeError::InvalidPayload),
            }
        }
        0x0b => {
            if reader.u8()? != 0 {
                return Ok(PlayInbound::Other(id));
            }
            let state_id = reader.varint()?;
            let clicked_slot = reader.i16()?;
            let button = reader.i8()?;
            let mode = reader.varint()?;
            if state_id < 0 {
                return Err(PlayDecodeError::InvalidPayload);
            }
            let action = match (mode, clicked_slot, button) {
                (4, 9..=45, 0) => InventoryAction::DropOne(clicked_slot as u8),
                (4, 9..=45, 1) => InventoryAction::DropStack(clicked_slot as u8),
                (0, -999, 0) => InventoryAction::DropCursorStack,
                (0, -999, 1) => InventoryAction::DropCursorOne,
                (0..=2 | 5..=6, _, _) => InventoryAction::Rearrange,
                _ => return Err(PlayDecodeError::InvalidPayload),
            };
            let count = reader.varint()?;
            if !(0..=46).contains(&count) {
                return Err(PlayDecodeError::InvalidPayload);
            }
            let mut changes = Vec::with_capacity(count as usize);
            for _ in 0..count {
                let slot = reader.i16()?;
                if !(0..46).contains(&slot) {
                    return Err(PlayDecodeError::InvalidPayload);
                }
                changes.push((slot as u8, read_slot(&mut reader)?));
            }
            let carried = read_slot(&mut reader)?;
            PlayInbound::ClickContainer {
                state_id,
                action,
                changes,
                carried,
            }
        }
        0x10 => {
            let target_entity_id = reader.varint()?;
            if target_entity_id < 0 {
                return Err(PlayDecodeError::InvalidPayload);
            }
            let attack = match reader.varint()? {
                0 => {
                    if !matches!(reader.varint()?, 0 | 1) {
                        return Err(PlayDecodeError::InvalidPayload);
                    }
                    false
                }
                1 => true,
                2 => {
                    let (x, y, z) = (reader.f32()?, reader.f32()?, reader.f32()?);
                    if !x.is_finite()
                        || !y.is_finite()
                        || !z.is_finite()
                        || !matches!(reader.varint()?, 0 | 1)
                    {
                        return Err(PlayDecodeError::InvalidPayload);
                    }
                    false
                }
                _ => return Err(PlayDecodeError::InvalidPayload),
            };
            let sneaking = reader.bool()?;
            if attack {
                PlayInbound::Attack {
                    target_entity_id,
                    sneaking,
                }
            } else {
                PlayInbound::Other(id)
            }
        }
        0x12 => PlayInbound::KeepAlive(reader.i64()?),
        0x14 | 0x15 => {
            let x = reader.f64()?;
            let y = reader.f64()?;
            let z = reader.f64()?;
            let look = if id == 0x15 {
                let yaw = reader.f32()?;
                let pitch = reader.f32()?;
                if !yaw.is_finite() || !pitch.is_finite() {
                    return Err(PlayDecodeError::InvalidPayload);
                }
                Some((yaw, pitch))
            } else {
                None
            };
            let on_ground = reader.bool()?;
            if !x.is_finite() || !y.is_finite() || !z.is_finite() {
                return Err(PlayDecodeError::InvalidPayload);
            }
            match look {
                Some((yaw, pitch)) => {
                    PlayInbound::PositionAndLook {
                        x,
                        y,
                        z,
                        yaw,
                        pitch,
                        on_ground,
                    }
                }
                None => PlayInbound::Position { x, y, z, on_ground },
            }
        }
        0x16 => {
            let yaw = reader.f32()?;
            let pitch = reader.f32()?;
            let on_ground = reader.bool()?;
            if !yaw.is_finite() || !pitch.is_finite() {
                return Err(PlayDecodeError::InvalidPayload);
            }
            PlayInbound::Look {
                yaw,
                pitch,
                on_ground,
            }
        }
        0x17 => PlayInbound::OnGround(reader.bool()?),
        0x1d => {
            let action = match reader.varint()? {
                0 => DigAction::Start,
                1 => DigAction::Cancel,
                2 => DigAction::Finish,
                3 => DigAction::DropStack,
                4 => DigAction::DropOne,
                _ => return Ok(PlayInbound::Other(id)),
            };
            let (x, y, z) = reader.position()?;
            let position = BlockPos::new(x, y, z).ok_or(PlayDecodeError::InvalidPayload)?;
            let face = reader.i8()?;
            if matches!(action, DigAction::Start | DigAction::Cancel | DigAction::Finish)
                && !(0..=5).contains(&face)
            {
                return Err(PlayDecodeError::InvalidPayload);
            }
            let sequence = reader.varint()?;
            if sequence < 0 {
                return Err(PlayDecodeError::InvalidPayload);
            }
            PlayInbound::Dig {
                action,
                position,
                sequence,
            }
        }
        0x28 => {
            let slot = reader.i16()?;
            if !(0..=8).contains(&slot) {
                return Err(PlayDecodeError::InvalidPayload);
            }
            PlayInbound::HeldSlot(slot as u8)
        }
        0x31 => {
            let hand = reader.varint()?;
            if !(0..=1).contains(&hand) {
                return Err(PlayDecodeError::InvalidPayload);
            }
            let (x, y, z) = reader.position()?;
            let target = BlockPos::new(x, y, z).ok_or(PlayDecodeError::InvalidPayload)?;
            let face = match reader.varint()? {
                0 => BlockFace::Down,
                1 => BlockFace::Up,
                2 => BlockFace::North,
                3 => BlockFace::South,
                4 => BlockFace::West,
                5 => BlockFace::East,
                _ => return Err(PlayDecodeError::InvalidPayload),
            };
            for _ in 0..3 {
                let cursor = reader.f32()?;
                if !cursor.is_finite() || !(0.0..=1.0).contains(&cursor) {
                    return Err(PlayDecodeError::InvalidPayload);
                }
            }
            reader.bool()?;
            let sequence = reader.varint()?;
            if sequence < 0 {
                return Err(PlayDecodeError::InvalidPayload);
            }
            PlayInbound::Place {
                target,
                face,
                main_hand: hand == 0,
                sequence,
            }
        }
        _ => return Ok(PlayInbound::Other(id)),
    };
    if reader.remaining() != 0 {
        return Err(PlayDecodeError::InvalidPayload);
    }
    Ok(packet)
}

// These are 1.19.2 global block-state IDs, not block registry IDs.
fn block_state_id(block: BlockKind) -> i32 {
    match block {
        BlockKind::Air => 0,
        BlockKind::Bedrock => 74,
        BlockKind::Dirt => 10,
        BlockKind::GrassBlock => 9,
        BlockKind::Stone => 1,
        BlockKind::Cobblestone => 14,
        BlockKind::Sand => 107,
        BlockKind::Water => 75,
    }
}

pub fn block_update_payload(position: BlockPos, block: BlockKind) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.position(position.x, position.y, position.z);
    writer.varint(block_state_id(block));
    writer.finish()
}

fn item_id(item: ItemKind) -> i32 {
    match item {
        ItemKind::GrassBlock => 14,
        ItemKind::Dirt => 15,
        ItemKind::Stone => 1,
        ItemKind::Cobblestone => 22,
        ItemKind::Sand => 40,
    }
}

fn read_slot(reader: &mut Reader<'_>) -> Result<Option<ItemStack>, PlayDecodeError> {
    if !reader.bool()? {
        return Ok(None);
    }
    let kind = match reader.varint()? {
        14 => ItemKind::GrassBlock,
        15 => ItemKind::Dirt,
        1 => ItemKind::Stone,
        22 => ItemKind::Cobblestone,
        40 => ItemKind::Sand,
        _ => return Err(PlayDecodeError::InvalidPayload),
    };
    let count = reader.i8()?;
    if reader.u8()? != 0 {
        return Err(PlayDecodeError::InvalidPayload);
    }
    let stack = u8::try_from(count)
        .ok()
        .and_then(|count| ItemStack::new(kind, count))
        .ok_or(PlayDecodeError::InvalidPayload)?;
    Ok(Some(stack))
}

fn write_slot(writer: &mut Writer, stack: Option<ItemStack>) {
    match stack {
        Some(stack) => {
            writer.bool(true);
            writer.varint(item_id(stack.kind()));
            writer.i8(stack.count() as i8);
            writer.u8(0); // empty NBT
        }
        None => writer.bool(false),
    }
}

pub fn inventory_payload(inventory: &Inventory, state_id: i32) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.u8(0); // player inventory
    writer.varint(state_id);
    writer.varint(46);
    for slot in 0..46 {
        write_slot(&mut writer, inventory.window_slot(slot as u8));
    }
    write_slot(&mut writer, inventory.carried());
    writer.finish()
}

pub fn inventory_slot_payload(slot: u8, stack: Option<ItemStack>, state_id: i32) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.i8(0); // player inventory
    writer.varint(state_id);
    writer.i16(i16::from(slot) + 36);
    write_slot(&mut writer, stack);
    writer.finish()
}

pub fn held_slot_payload(slot: u8) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.i8(slot as i8);
    writer.finish()
}

fn angle_byte(angle: f32) -> i8 {
    (angle * 256.0 / 360.0).floor() as i32 as i8
}

fn velocity_short(value: f64) -> i16 {
    (value.clamp(-3.9, 3.9) * 8000.0) as i16
}

pub fn item_entity_spawn_payload(spawn: ItemEntitySpawn) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.varint(spawn.entity_id);
    writer.uuid(spawn.uuid);
    writer.varint(44); // 1.19.2 item entity registry ID
    writer.f64(spawn.x);
    writer.f64(spawn.y);
    writer.f64(spawn.z);
    writer.i8(0); // pitch
    writer.i8(0); // yaw
    writer.i8(0); // head pitch
    writer.varint(0); // object data
    writer.i16(velocity_short(spawn.velocity.x));
    writer.i16(velocity_short(spawn.velocity.y));
    writer.i16(velocity_short(spawn.velocity.z));
    writer.finish()
}

pub fn entity_teleport_payload(entity_id: i32, pose: crate::version::PlayerPose) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.varint(entity_id);
    writer.f64(pose.x);
    writer.f64(pose.y);
    writer.f64(pose.z);
    writer.i8(angle_byte(pose.yaw));
    writer.i8(angle_byte(pose.pitch));
    writer.bool(pose.on_ground);
    writer.finish()
}

pub fn item_entity_metadata_payload(entity_id: i32, stack: ItemStack) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.varint(entity_id);
    writer.u8(8); // item entity stack metadata index
    writer.varint(6); // slot metadata type
    write_slot(&mut writer, Some(stack));
    writer.u8(0xff); // metadata terminator
    writer.finish()
}

pub fn entity_velocity_payload(entity_id: i32, velocity: EntityVelocity) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.varint(entity_id);
    writer.i16(velocity_short(velocity.x));
    writer.i16(velocity_short(velocity.y));
    writer.i16(velocity_short(velocity.z));
    writer.finish()
}

pub fn combat_death_payload(player_id: i32, message: &str) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.varint(player_id);
    writer.i32(-1); // no killer entity
    writer.string(message);
    writer.finish()
}

pub fn player_health_payload(health: f32, food: i32) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.f32(health);
    writer.varint(food);
    writer.f32(5.0);
    writer.finish()
}

pub fn respawn_payload(copy_metadata: bool) -> Vec<u8> {
    let mut payload = Writer::new();
    payload.identifier(OVERWORLD); // dimension
    payload.identifier(OVERWORLD); // world name
    payload.i64(0); // hashed seed
    payload.i8(0); // survival
    payload.u8(0xff); // no previous game mode
    payload.bool(false); // debug world
    payload.bool(false); // non-flat world
    payload.bool(copy_metadata);
    payload.bool(false); // no death location
    payload.finish()
}

pub fn remove_entity_payload(entity_id: i32) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.varint(1);
    writer.varint(entity_id);
    writer.finish()
}

pub fn presence_packets(change: PresenceChange<'_>) -> Vec<(ClientboundPacket, Vec<u8>)> {
    match change {
        PresenceChange::Add {
            entity_id,
            uuid,
            username,
            pose,
        } => {
            let mut info = Writer::new();
            info.varint(0); // add player
            info.varint(1);
            info.uuid(uuid);
            info.string(username);
            info.varint(0); // no profile properties
            info.varint(0); // survival
            info.varint(0); // latency
            info.bool(false); // no display name
            info.bool(false); // no public key

            let mut spawn = Writer::new();
            spawn.varint(entity_id);
            spawn.uuid(uuid);
            spawn.f64(pose.x);
            spawn.f64(pose.y);
            spawn.f64(pose.z);
            spawn.i8(angle_byte(pose.yaw));
            spawn.i8(angle_byte(pose.pitch));
            vec![
                (ClientboundPacket::PlayerInfo, info.finish()),
                (ClientboundPacket::SpawnPlayer, spawn.finish()),
            ]
        }
        PresenceChange::Move { entity_id, pose } => {
            let mut teleport = Writer::new();
            teleport.varint(entity_id);
            teleport.f64(pose.x);
            teleport.f64(pose.y);
            teleport.f64(pose.z);
            teleport.i8(angle_byte(pose.yaw));
            teleport.i8(angle_byte(pose.pitch));
            teleport.bool(pose.on_ground);

            let mut head = Writer::new();
            head.varint(entity_id);
            head.i8(angle_byte(pose.yaw));
            vec![
                (ClientboundPacket::EntityTeleport, teleport.finish()),
                (ClientboundPacket::EntityHeadRotation, head.finish()),
            ]
        }
        PresenceChange::Remove { entity_id, uuid } => {
            let mut info = Writer::new();
            info.varint(4); // remove player
            info.varint(1);
            info.uuid(uuid);
            vec![
                (ClientboundPacket::RemoveEntities, remove_entity_payload(entity_id)),
                (ClientboundPacket::PlayerInfo, info.finish()),
            ]
        }
    }
}

fn dimension_type() -> Tag<'static> {
    Tag::Compound(vec![
        ("piglin_safe", Tag::Byte(0)),
        ("natural", Tag::Byte(1)),
        ("ambient_light", Tag::Float(0.0)),
        ("infiniburn", Tag::String("#minecraft:infiniburn_overworld")),
        ("respawn_anchor_works", Tag::Byte(0)),
        ("has_skylight", Tag::Byte(1)),
        ("bed_works", Tag::Byte(1)),
        ("effects", Tag::String(OVERWORLD)),
        ("has_raids", Tag::Byte(1)),
        ("min_y", Tag::Int(FlatWorld::MIN_Y)),
        ("height", Tag::Int(FlatWorld::HEIGHT)),
        ("logical_height", Tag::Int(FlatWorld::HEIGHT)),
        ("coordinate_scale", Tag::Double(1.0)),
        ("ultrawarm", Tag::Byte(0)),
        ("has_ceiling", Tag::Byte(0)),
        ("monster_spawn_block_light_limit", Tag::Int(0)),
        (
            "monster_spawn_light_level",
            Tag::Compound(vec![
                ("type", Tag::String("minecraft:uniform")),
                (
                    "value",
                    Tag::Compound(vec![("min_inclusive", Tag::Int(0)), ("max_inclusive", Tag::Int(7))]),
                ),
            ]),
        ),
    ])
}

fn plains_biome() -> Tag<'static> {
    Tag::Compound(vec![
        ("precipitation", Tag::String("rain")),
        ("temperature", Tag::Float(0.8)),
        ("downfall", Tag::Float(0.4)),
        (
            "effects",
            Tag::Compound(vec![
                ("sky_color", Tag::Int(7_907_327)),
                ("water_fog_color", Tag::Int(329_011)),
                ("fog_color", Tag::Int(12_638_463)),
                ("water_color", Tag::Int(4_159_204)),
                (
                    "mood_sound",
                    Tag::Compound(vec![
                        ("tick_delay", Tag::Int(6000)),
                        ("offset", Tag::Double(2.0)),
                        ("sound", Tag::String("minecraft:ambient.cave")),
                        ("block_search_extent", Tag::Int(8)),
                    ]),
                ),
            ]),
        ),
    ])
}

fn registry_entry(name: &'static str, id: i32, element: Tag<'static>) -> Tag<'static> {
    Tag::Compound(vec![
        ("name", Tag::String(name)),
        ("id", Tag::Int(id)),
        ("element", element),
    ])
}

fn registry_codec() -> Vec<u8> {
    encode_root(vec![
        (
            "minecraft:chat_type",
            Tag::Compound(vec![
                ("type", Tag::String("minecraft:chat_type")),
                (
                    "value",
                    Tag::CompoundList(vec![registry_entry(
                        "minecraft:chat",
                        0,
                        Tag::Compound(vec![
                            (
                                "chat",
                                Tag::Compound(vec![
                                    ("translation_key", Tag::String("chat.type.text")),
                                    ("parameters", Tag::StringList(vec!["sender", "content"])),
                                ]),
                            ),
                            (
                                "narration",
                                Tag::Compound(vec![
                                    ("translation_key", Tag::String("chat.type.text.narrate")),
                                    ("parameters", Tag::StringList(vec!["sender", "content"])),
                                ]),
                            ),
                        ]),
                    )]),
                ),
            ]),
        ),
        (
            "minecraft:dimension_type",
            Tag::Compound(vec![
                ("type", Tag::String("minecraft:dimension_type")),
                ("value", Tag::CompoundList(vec![registry_entry(OVERWORLD, 0, dimension_type())])),
            ]),
        ),
        (
            "minecraft:worldgen/biome",
            Tag::Compound(vec![
                ("type", Tag::String("minecraft:worldgen/biome")),
                (
                    "value",
                    Tag::CompoundList(vec![registry_entry(
                        "minecraft:plains",
                        PLAINS_BIOME_ID,
                        plains_biome(),
                    )]),
                ),
            ]),
        ),
    ])
}

pub fn join_game_payload(
    entity_id: i32,
    max_players: u32,
    view_distance: u8,
    simulation_distance: u8,
) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.i32(entity_id); // player entity ID
    writer.bool(false); // hardcore
    writer.u8(0); // survival
    writer.i8(-1); // no previous game mode
    writer.varint(1);
    writer.identifier(OVERWORLD);
    writer.raw(&registry_codec());
    writer.identifier(OVERWORLD); // dimension type key
    writer.identifier(OVERWORLD); // world key
    writer.i64(0); // hashed seed
    writer.varint(max_players as i32);
    writer.varint(i32::from(view_distance));
    writer.varint(i32::from(simulation_distance));
    writer.bool(false); // reduced debug info
    writer.bool(true); // respawn screen
    writer.bool(false); // debug world
    writer.bool(false); // non-flat world
    writer.bool(false); // no prior death position
    writer.finish()
}

fn heightmap(chunk: &ChunkSnapshot) -> Vec<u8> {
    // 384-block height requires 9 bits per entry, with 7 entries per long.
    let mut longs = vec![0i64; 37];
    for index in 0..256 {
        let x = (index % 16) as u8;
        let z = (index / 16) as u8;
        let height = (FlatWorld::MIN_Y..FlatWorld::MIN_Y + FlatWorld::HEIGHT)
            .rev()
            .find(|&y| chunk.block_at(x, y, z) != BlockKind::Air)
            .map_or(0, |y| i64::from(y + 1 - FlatWorld::MIN_Y));
        longs[index / 7] |= height << ((index % 7) * 9);
    }
    encode_root(vec![
        ("MOTION_BLOCKING", Tag::LongArray(longs.clone())),
        ("WORLD_SURFACE", Tag::LongArray(longs)),
    ])
}

fn section(writer: &mut Writer, section_y: i32, chunk: &ChunkSnapshot) {
    let base_y = section_y * 16;
    let mut palette = vec![BlockKind::Air];
    let mut indices = Vec::with_capacity(4096);
    let mut non_air = 0;
    for y in base_y..base_y + 16 {
        for z in 0..16 {
            for x in 0..16 {
                let block = chunk.block_at(x, y, z);
                if block != BlockKind::Air {
                    non_air += 1;
                }
                let index = match palette.iter().position(|&item| item == block) {
                    Some(index) => index,
                    None => {
                        palette.push(block);
                        palette.len() - 1
                    }
                };
                indices.push(index as i64);
            }
        }
    }
    writer.i16(non_air as i16);
    if palette.len() == 1 {
        writer.u8(0);
        writer.varint(block_state_id(palette[0]));
        writer.varint(0);
    } else {
        writer.u8(4);
        writer.varint(palette.len() as i32);
        for block in &palette {
            writer.varint(block_state_id(*block));
        }
        writer.varint(256);
        for group in indices.as_chunks::<16>().0 {
            let packed = group
                .iter()
                .enumerate()
                .fold(0i64, |value, (shift, index)| value | (index << (shift * 4)));
            writer.i64(packed);
        }
    }
    // A single plains biome fills the 4x4x4 biome cells in this section.
    writer.u8(0);
    writer.varint(PLAINS_BIOME_ID);
    writer.varint(0);
}

pub fn chunk_payload(x: i32, z: i32, chunk: &ChunkSnapshot) -> Vec<u8> {
    let mut writer = Writer::with_capacity(64 * 1024);
    writer.i32(x);
    writer.i32(z);
    writer.raw(&heightmap(chunk));
    let mut data = Writer::new();
    for section_y in -4..20 {
        section(&mut data, section_y, chunk);
    }
    let data = data.finish();
    writer.varint(data.len() as i32);
    writer.raw(&data);
    writer.varint(0); // no block entities
    writer.bool(true); // trust edges
    // There are 24 chunk sections plus two light boundary sections.
    const LIGHT_MASK: i64 = (1 << 26) - 1;
    writer.varint(1);
    writer.i64(LIGHT_MASK); // sky light mask
    writer.varint(0); // block light mask
    writer.varint(0); // empty sky light mask
    writer.varint(1);
    writer.i64(LIGHT_MASK); // empty block light mask
    writer.varint(26);
    let full_light = [0xffu8; 2048];
    for _ in 0..26 {
        writer.varint(full_light.len() as i32);
        writer.raw(&full_light);
    }
    writer.varint(0); // block light arrays
    writer.finish()
}

#[cfg(test)]
mod tests {
    use rustbukkit_chunks::{BlockKind, ChunkPos, World};
    use rustbukkit_decoding::Reader;
    use rustbukkit_encoding::{Writer, encode_frame};
    use rustbukkit_inventory::{InventoryAction, ItemKind, ItemStack};
    use uuid::Uuid;

    use super::{block_state_id, block_update_payload, chunk_payload, join_game_payload, parse_play};
    use crate::{
        ClientboundPacket,
        DigAction,
        EntityVelocity,
        ItemEntitySpawn,
        PlayInbound,
        PlayerPose,
        PresenceChange,
        legacy_test_fixture,
    };

    fn fingerprint(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }

    #[test]
    fn maps_semantic_blocks_to_1_19_2_states() {
        assert_eq!(block_state_id(BlockKind::Air), 0);
        assert_eq!(block_state_id(BlockKind::Bedrock), 74);
        assert_eq!(block_state_id(BlockKind::Dirt), 10);
        assert_eq!(block_state_id(BlockKind::GrassBlock), 9);
        assert_eq!(block_state_id(BlockKind::Stone), 1);
        assert_eq!(block_state_id(BlockKind::Cobblestone), 14);
        assert_eq!(block_state_id(BlockKind::Sand), 107);
        assert_eq!(block_state_id(BlockKind::Water), 75);
    }

    #[test]
    fn payloads_are_bounded() {
        assert!(join_game_payload(1, 20, 4, 4).len() < 4096);
        let chunk = World::new().snapshot_chunk(ChunkPos { x: 0, z: 0 });
        assert!(chunk_payload(0, 0, &chunk).len() < 100_000);
    }

    #[test]
    fn non_flat_join_and_generated_chunk_payloads_are_bounded() {
        let version = legacy_test_fixture().play_protocol().unwrap();
        let join = encode_frame(
            version.clientbound_id(ClientboundPacket::JoinGame),
            &join_game_payload(1, 20, 4, 4),
        )
        .unwrap();
        let chunk = World::new().snapshot_chunk(ChunkPos { x: 0, z: 0 });
        let chunk =
            encode_frame(version.clientbound_id(ClientboundPacket::ChunkData), &chunk_payload(0, 0, &chunk))
                .unwrap();
        assert!(join.len() < 4096);
        assert!(chunk.len() < 100_000);
        assert_ne!(fingerprint(&chunk), 4167622057084521045);
    }

    #[test]
    fn rejects_non_finite_and_trailing_movement_data() {
        let mut invalid = rustbukkit_encoding::Writer::new();
        invalid.f64(f64::NAN);
        invalid.f64(64.0);
        invalid.f64(0.0);
        invalid.bool(true);
        assert!(parse_play(0x14, &invalid.finish()).is_err());
        let mut valid = rustbukkit_encoding::Writer::new();
        valid.f32(90.0);
        valid.f32(0.0);
        valid.bool(true);
        let mut bytes = valid.finish();
        bytes.push(0);
        assert!(parse_play(0x16, &bytes).is_err());
    }

    #[test]
    fn health_combat_and_respawn_packets_match_expected_fields() {
        let version = legacy_test_fixture().play_protocol().unwrap();
        assert_eq!(version.clientbound_id(ClientboundPacket::Respawn), 0x3e);
        assert_eq!(version.clientbound_id(ClientboundPacket::CombatEvent), 0x36);
        assert_eq!(version.player_health_payload(17.5, 20), [0x41, 0x8c, 0, 0, 20, 0x40, 0xa0, 0, 0]);
        let death = version.combat_death_payload(7, r#"{"text":"was slain"}"#);
        let mut reader = Reader::new(&death);
        assert_eq!(reader.varint().unwrap(), 7);
        assert_eq!(reader.i32().unwrap(), -1);
        assert_eq!(reader.string(32767).unwrap(), r#"{"text":"was slain"}"#);
        assert_eq!(reader.remaining(), 0);
        let respawn = version.respawn_payload(true);
        let mut reader = Reader::new(&respawn);
        assert_eq!(reader.string(32767).unwrap(), "minecraft:overworld");
        assert_eq!(reader.string(32767).unwrap(), "minecraft:overworld");
        assert_eq!(reader.i64().unwrap(), 0);
        assert_eq!(reader.i8().unwrap(), 0);
        assert_eq!(reader.u8().unwrap(), 0xff);
        assert!(!reader.bool().unwrap()); // debug
        assert!(!reader.bool().unwrap()); // non-flat
        assert!(reader.bool().unwrap()); // copy metadata
        assert!(!reader.bool().unwrap()); // no death location
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn decodes_attack_and_respawn_with_1_19_2_ids() {
        assert_eq!(
            parse_play(0x10, &[0xac, 0x02, 0x01, 0x00]).unwrap(),
            PlayInbound::Attack {
                target_entity_id: 300,
                sneaking:         false,
            },
        );
        assert_eq!(parse_play(0x07, &[0x00]).unwrap(), PlayInbound::RespawnRequest);
        assert!(matches!(parse_play(0x07, &[0x01]).unwrap(), PlayInbound::Other(0x07)));
        assert!(matches!(parse_play(0x10, &[0x01, 0x00, 0x00, 0x00]).unwrap(), PlayInbound::Other(0x10)));
        assert!(parse_play(0x10, &[0x01, 0x01]).is_err()); // missing sneaking flag
        assert!(parse_play(0x10, &[0x01, 0x01, 0x00, 0x00]).is_err()); // trailing byte
        assert!(parse_play(0x07, &[0x02]).is_err());
    }

    #[test]
    fn item_entity_packets_match_1_19_2_fields() {
        let version = legacy_test_fixture().play_protocol().unwrap();
        let uuid = Uuid::from_u128(0x0011_2233_4455_6677_8899_aabb_ccdd_eeff);
        let velocity = EntityVelocity {
            x: 0.1,
            y: 0.2,
            z: -0.1,
        };
        let spawn = version.item_entity_spawn_payload(ItemEntitySpawn {
            entity_id: 300,
            uuid,
            x: 1.5,
            y: 64.0,
            z: -2.0,
            velocity,
        });
        assert_eq!(version.clientbound_id(ClientboundPacket::SpawnEntity), 0x00);
        let mut reader = Reader::new(&spawn);
        assert_eq!(reader.varint().unwrap(), 300);
        assert_eq!(reader.uuid().unwrap(), uuid);
        assert_eq!(reader.varint().unwrap(), 44);
        assert_eq!((reader.f64().unwrap(), reader.f64().unwrap(), reader.f64().unwrap()), (1.5, 64.0, -2.0));
        assert_eq!(reader.raw(3).unwrap(), &[0, 0, 0]);
        assert_eq!(reader.varint().unwrap(), 0);
        assert_eq!((reader.i16().unwrap(), reader.i16().unwrap(), reader.i16().unwrap()), (800, 1600, -800));
        assert_eq!(reader.remaining(), 0);

        let stack = ItemStack::new(ItemKind::Dirt, 5).unwrap();
        let metadata = version.item_entity_metadata_payload(300, stack);
        assert_eq!(version.clientbound_id(ClientboundPacket::EntityMetadata), 0x50);
        assert_eq!(metadata, [0xac, 0x02, 8, 6, 1, 15, 5, 0, 0xff]);
        let motion = version.entity_velocity_payload(300, velocity);
        assert_eq!(version.clientbound_id(ClientboundPacket::EntityVelocity), 0x52);
        assert_eq!(motion, [0xac, 0x02, 0x03, 0x20, 0x06, 0x40, 0xfc, 0xe0]);
        assert_eq!(version.remove_entity_payload(300), [1, 0xac, 0x02]);
    }

    #[test]
    fn decodes_inventory_and_hotbar_drop_actions() {
        let mut click = Writer::new();
        click.u8(0);
        click.varint(0);
        click.i16(9);
        click.i8(1); // Ctrl+Q
        click.varint(4); // drop mode
        click.varint(1);
        click.i16(9);
        click.bool(false);
        click.bool(false); // empty cursor
        assert!(matches!(
            parse_play(0x0b, &click.finish()).unwrap(),
            PlayInbound::ClickContainer {
                action: InventoryAction::DropStack(9),
                ..
            }
        ));

        let mut dig = Writer::new();
        dig.varint(3); // drop selected stack outside inventory
        dig.position(0, 0, 0);
        dig.i8(-1);
        dig.varint(1);
        assert!(matches!(
            parse_play(0x1d, &dig.finish()).unwrap(),
            PlayInbound::Dig {
                action: DigAction::DropStack,
                ..
            }
        ));
    }

    #[test]
    fn fixed_1_19_2_keepalive_block_update_and_unload_frames() {
        let version = legacy_test_fixture().play_protocol().unwrap();
        let keepalive = encode_frame(
            version.clientbound_id(ClientboundPacket::KeepAlive),
            &0x0102_0304_0506_0708i64.to_be_bytes(),
        )
        .unwrap();
        assert_eq!(keepalive, [0x09, 0x20, 1, 2, 3, 4, 5, 6, 7, 8]);

        let position = rustbukkit_chunks::BlockPos::new(0, 63, 0).unwrap();
        let block = encode_frame(
            version.clientbound_id(ClientboundPacket::BlockUpdate),
            &block_update_payload(position, BlockKind::Dirt),
        )
        .unwrap();
        assert_eq!(block, [0x0a, 0x09, 0, 0, 0, 0, 0, 0, 0, 63, 10]);

        let mut unload_payload = Vec::new();
        unload_payload.extend_from_slice(&2i32.to_be_bytes());
        unload_payload.extend_from_slice(&(-1i32).to_be_bytes());
        let unload =
            encode_frame(version.clientbound_id(ClientboundPacket::UnloadChunk), &unload_payload).unwrap();
        assert_eq!(unload, [0x09, 0x1c, 0, 0, 0, 2, 0xff, 0xff, 0xff, 0xff]);
    }

    #[test]
    fn player_presence_packets_have_1_19_2_layouts() {
        let uuid = Uuid::from_u128(0x0011_2233_4455_6677_8899_aabb_ccdd_eeff);
        let pose = PlayerPose {
            x:         1.5,
            y:         64.0,
            z:         -2.0,
            yaw:       90.0,
            pitch:     0.0,
            on_ground: true,
        };
        let packets = super::presence_packets(PresenceChange::Add {
            entity_id: 2,
            uuid,
            username: "Test2",
            pose,
        });
        assert_eq!(packets[0].0, ClientboundPacket::PlayerInfo);
        let mut info = Reader::new(&packets[0].1);
        assert_eq!(info.varint().unwrap(), 0);
        assert_eq!(info.varint().unwrap(), 1);
        assert_eq!(info.uuid().unwrap(), uuid);
        assert_eq!(info.string(16).unwrap(), "Test2");
        assert_eq!(info.raw(5).unwrap(), &[0, 0, 0, 0, 0]);
        assert_eq!(info.remaining(), 0);
        assert_eq!(packets[1].0, ClientboundPacket::SpawnPlayer);
        let mut spawn = Reader::new(&packets[1].1);
        assert_eq!(spawn.varint().unwrap(), 2);
        assert_eq!(spawn.uuid().unwrap(), uuid);
        assert_eq!(spawn.f64().unwrap(), 1.5);
        assert_eq!(spawn.f64().unwrap(), 64.0);
        assert_eq!(spawn.f64().unwrap(), -2.0);
        assert_eq!(spawn.i8().unwrap(), 64);
        assert_eq!(spawn.i8().unwrap(), 0);
        assert_eq!(spawn.remaining(), 0);

        let removed = super::presence_packets(PresenceChange::Remove { entity_id: 2, uuid });
        assert_eq!(removed[0].0, ClientboundPacket::RemoveEntities);
        assert_eq!(removed[0].1, [1, 2]);
        assert_eq!(removed[1].0, ClientboundPacket::PlayerInfo);
        assert_eq!(&removed[1].1[..2], &[4, 1]);
        assert_eq!(&removed[1].1[2..], uuid.as_bytes());
    }
}
