use rustbukkit_chunks::{BlockKind, FlatWorld};
use rustbukkit_decoding::Reader;
use rustbukkit_encoding::Writer;

use crate::nbt::{Tag, encode_root};
use crate::version::{LoginError, PlayDecodeError, PlayInbound};

const OVERWORLD: &str = "minecraft:overworld";
const PLAINS_BIOME_ID: i32 = 1;

pub fn parse_login_start(payload: &[u8]) -> Result<String, LoginError> {
    let mut reader = Reader::new(payload);
    let username = reader.string(16)?;
    if username.is_empty() || !username.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err(LoginError::InvalidUsername);
    }
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
        0x12 => PlayInbound::KeepAlive(reader.i64()?),
        0x14 | 0x15 => {
            let x = reader.f64()?;
            let y = reader.f64()?;
            let z = reader.f64()?;
            if id == 0x15 {
                let yaw = reader.f32()?;
                let pitch = reader.f32()?;
                if !yaw.is_finite() || !pitch.is_finite() {
                    return Err(PlayDecodeError::InvalidPayload);
                }
            }
            reader.bool()?;
            if !x.is_finite() || !y.is_finite() || !z.is_finite() {
                return Err(PlayDecodeError::InvalidPayload);
            }
            PlayInbound::Position { x, y, z }
        }
        0x16 => {
            let yaw = reader.f32()?;
            let pitch = reader.f32()?;
            reader.bool()?;
            if !yaw.is_finite() || !pitch.is_finite() {
                return Err(PlayDecodeError::InvalidPayload);
            }
            PlayInbound::Look
        }
        0x17 => {
            reader.bool()?;
            PlayInbound::OnGround
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

pub fn join_game_payload(max_players: u32, view_distance: u8, simulation_distance: u8) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.i32(1); // player entity ID
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
    writer.bool(true); // flat world
    writer.bool(false); // no prior death position
    writer.finish()
}

fn heightmap() -> Vec<u8> {
    // 384-block height requires 9 bits per entry, with 7 entries per long.
    let mut longs = vec![0i64; 37];
    let height = (FlatWorld::SURFACE_Y + 1 - FlatWorld::MIN_Y) as i64;
    for index in 0..256 {
        longs[index / 7] |= height << ((index % 7) * 9);
    }
    encode_root(vec![
        ("MOTION_BLOCKING", Tag::LongArray(longs.clone())),
        ("WORLD_SURFACE", Tag::LongArray(longs)),
    ])
}

fn section(writer: &mut Writer, section_y: i32) {
    let base_y = section_y * 16;
    let mut palette = vec![BlockKind::Air];
    for y in base_y..base_y + 16 {
        let block = FlatWorld.block_at(y);
        if !palette.contains(&block) {
            palette.push(block);
        }
    }
    let non_air = (base_y..base_y + 16)
        .filter(|&y| FlatWorld.block_at(y) != BlockKind::Air)
        .count()
        * 256;
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
        for y in base_y..base_y + 16 {
            let block = FlatWorld.block_at(y);
            let index = palette.iter().position(|&item| item == block).unwrap_or(0) as i64;
            let packed = (0..16).fold(0i64, |value, shift| value | (index << (shift * 4)));
            for _ in 0..16 {
                writer.i64(packed);
            }
        }
    }
    // A single plains biome fills the 4x4x4 biome cells in this section.
    writer.u8(0);
    writer.varint(PLAINS_BIOME_ID);
    writer.varint(0);
}

pub fn chunk_payload(x: i32, z: i32) -> Vec<u8> {
    let mut writer = Writer::with_capacity(64 * 1024);
    writer.i32(x);
    writer.i32(z);
    writer.raw(&heightmap());
    let mut data = Writer::new();
    for section_y in -4..20 {
        section(&mut data, section_y);
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
    use rustbukkit_chunks::BlockKind;

    use super::{block_state_id, chunk_payload, join_game_payload};

    #[test]
    fn maps_semantic_blocks_to_1_19_2_states() {
        assert_eq!(block_state_id(BlockKind::Air), 0);
        assert_eq!(block_state_id(BlockKind::Bedrock), 74);
        assert_eq!(block_state_id(BlockKind::Dirt), 10);
        assert_eq!(block_state_id(BlockKind::GrassBlock), 9);
    }

    #[test]
    fn payloads_are_bounded() {
        assert!(join_game_payload(20, 4, 4).len() < 4096);
        assert!(chunk_payload(0, 0).len() < 100_000);
    }
}
