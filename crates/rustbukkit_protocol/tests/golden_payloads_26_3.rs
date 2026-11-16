//! Golden-byte tests against payloads captured from the official 26.3 server.
//!
//! The fixtures in `registry_data/26.3/golden_packets.json` are raw bytes the
//! official server emitted (see `target/probe/dump.js` provenance in
//! `registry_data/26.3/README.md`). They are the reason Login Success and
//! Server Data cannot silently drift again: the synthetic client does not parse
//! a real client's profile/component fields, but a byte comparison does.

use std::path::PathBuf;

use rustbukkit_protocol::VersionRegistry;
use rustbukkit_protocol::v26_3::chunk::PLAINS_BIOME_ID;
use rustbukkit_protocol::v26_3::clientbound::encode_server_data;
use rustbukkit_protocol::v26_3::registry::NbtTag;
use uuid::Uuid;

fn golden() -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../registry_data/26.3/golden_packets.json");
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
    serde_json::from_slice(&bytes).unwrap_or_else(|error| panic!("parsing {}: {error}", path.display()))
}

fn decode_hex(hex: &str) -> Vec<u8> {
    assert!(hex.len().is_multiple_of(2), "odd-length hex fixture");
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("valid hex fixture"))
        .collect()
}

fn take_varint(bytes: &[u8], offset: &mut usize) -> i32 {
    let mut value = 0u32;
    let mut shift = 0;
    loop {
        let byte = bytes[*offset];
        *offset += 1;
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return value as i32;
        }
        shift += 7;
    }
}

#[test]
fn login_success_is_byte_identical_to_the_official_26_3_server() {
    let fixture = decode_hex(golden()["login_success"].as_str().expect("login_success fixture"));
    assert_eq!(fixture.len(), 39, "captured 'Probe' profile payload length");

    let uuid = Uuid::from_slice(&fixture[..16]).expect("profile UUID");
    let mut offset = 16;
    let name_len = take_varint(&fixture, &mut offset) as usize;
    let name = std::str::from_utf8(&fixture[offset..offset + name_len]).expect("profile name");
    offset += name_len;
    let properties = take_varint(&fixture, &mut offset);
    let session = Uuid::from_slice(&fixture[offset..offset + 16]).expect("session UUID");
    offset += 16;

    assert_eq!(offset, fixture.len(), "no trailing bytes after the session UUID");
    assert_eq!(properties, 0, "an offline GameProfile has no properties");

    let pack = VersionRegistry::select("26.3").unwrap();
    let encoded = pack
        .login_protocol()
        .expect("26.3 login protocol")
        .login_success_payload(uuid, name, session);
    assert_eq!(encoded, fixture, "Login Success must match the official GameProfile + session UUID layout");
}

#[test]
fn server_data_matches_the_official_26_3_component_and_icon_optional() {
    let fixture = decode_hex(
        golden()["server_data_plain_motd"]
            .as_str()
            .expect("server_data fixture"),
    );
    assert_eq!(fixture[0], 8, "network NBT root is a String Component");
    assert_eq!(*fixture.last().unwrap(), 0, "optional icon is absent");

    let encoded = encode_server_data(&NbtTag::String("VanillaProbe".into()), None).unwrap();
    assert_eq!(encoded, fixture, "Server Data must match the official payload");

    // The icon branch is `Optional<byte[]>`: flag, VarInt length, PNG bytes.
    let icon = [0x89u8, b'P', b'N', b'G'];
    let with_icon = encode_server_data(&NbtTag::String("VanillaProbe".into()), Some(&icon)).unwrap();
    assert_eq!(&with_icon[..with_icon.len() - 6], &fixture[..fixture.len() - 1]);
    assert_eq!(&with_icon[with_icon.len() - 6..], &[1, 4, 0x89, b'P', b'N', b'G']);
}

struct Registry {
    id:      String,
    entries: Vec<String>,
}

fn parse_registry(payload: &[u8]) -> Registry {
    let mut offset = 0;
    let id_len = take_varint(payload, &mut offset) as usize;
    let id = std::str::from_utf8(&payload[offset..offset + id_len])
        .unwrap()
        .to_owned();
    offset += id_len;
    let count = take_varint(payload, &mut offset);
    let mut entries = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let len = take_varint(payload, &mut offset) as usize;
        entries.push(
            std::str::from_utf8(&payload[offset..offset + len])
                .unwrap()
                .to_owned(),
        );
        offset += len;
        let has_data = payload[offset];
        offset += 1;
        assert_eq!(has_data, 0, "known-pack registry entries carry no NBT");
    }
    assert_eq!(offset, payload.len(), "registry payload fully consumed");
    Registry { id, entries }
}

#[test]
fn configuration_tags_cover_item_tags_referenced_by_registries() {
    let payload = rustbukkit_protocol::v26_3::configuration_tags_packet();
    assert!(payload.len() > 50_000, "official 26.3 tag payload is ~58 KB");

    let mut offset = 0;
    let registry_count = take_varint(payload, &mut offset);
    let mut found = false;
    for _ in 0..registry_count {
        let id_len = take_varint(payload, &mut offset) as usize;
        let registry = std::str::from_utf8(&payload[offset..offset + id_len]).unwrap();
        offset += id_len;
        let tag_count = take_varint(payload, &mut offset);
        for _ in 0..tag_count {
            let tag_len = take_varint(payload, &mut offset) as usize;
            let tag = std::str::from_utf8(&payload[offset..offset + tag_len]).unwrap();
            offset += tag_len;
            let entry_count = take_varint(payload, &mut offset);
            for _ in 0..entry_count {
                take_varint(payload, &mut offset);
            }
            if registry == "minecraft:item" && tag == "minecraft:sulfur_cube_archetype/slow_sliding" {
                found = true;
            }
        }
    }
    assert_eq!(offset, payload.len(), "tag payload fully consumed");
    assert!(found, "tag required by the sulfur_cube_archetype registry is present");
}

#[test]
fn configuration_registry_snapshot_locks_biome_dimension_and_clock_ids() {
    let packets = rustbukkit_protocol::v26_3::configuration_registry_packets();
    assert_eq!(packets.len(), 32, "official 26.3 configuration registry count");

    let registries: Vec<Registry> = packets.iter().map(|payload| parse_registry(payload)).collect();
    let find = |id: &str| {
        registries
            .iter()
            .find(|registry| registry.id == id)
            .unwrap_or_else(|| panic!("missing registry {id}"))
    };

    assert_eq!(find("minecraft:worldgen/biome").entries.len(), 67);
    let plains = find("minecraft:worldgen/biome")
        .entries
        .iter()
        .position(|entry| entry == "minecraft:plains")
        .expect("plains in the biome registry");
    assert_eq!(plains as i32, PLAINS_BIOME_ID);

    let dimension_type = find("minecraft:dimension_type");
    assert_eq!(
        dimension_type
            .entries
            .iter()
            .position(|entry| entry == "minecraft:overworld"),
        Some(0)
    );
    let clock = find("minecraft:world_clock");
    assert_eq!(
        clock
            .entries
            .iter()
            .position(|entry| entry == "minecraft:overworld"),
        Some(0)
    );

    let ids = &golden()["registry_ids"];
    assert_eq!(ids["plains_biome"].as_i64(), Some(i64::from(PLAINS_BIOME_ID)));
    assert_eq!(ids["overworld_dimension_type"].as_i64(), Some(0));
    assert_eq!(ids["overworld_world_clock"].as_i64(), Some(0));
}
