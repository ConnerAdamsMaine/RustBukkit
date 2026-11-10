//! Locks protocol constants to the extracted 26.3 reports in `registry_data/26.3`.
//!
//! These assertions are the reason packet IDs and block states are not guessed:
//! regenerating the report (see `registry_data/26.3/README.md`) and changing the
//! Rust constants must happen together.

use std::path::PathBuf;

use rustbukkit_chunks::BlockKind;
use rustbukkit_protocol::v26_3::chunk::block_state_id;
use rustbukkit_protocol::v26_3::play::{
    ACCEPT_TELEPORTATION_ID,
    ATTACK_ID,
    CLIENT_COMMAND_ID,
    CONTAINER_CLICK_ID,
    KEEP_ALIVE_ID as PLAY_KEEP_ALIVE_ID,
    MOVE_PLAYER_POS_ID,
    MOVE_PLAYER_POS_ROT_ID,
    MOVE_PLAYER_ROT_ID,
    MOVE_PLAYER_STATUS_ID,
    PLAYER_ACTION_ID,
    SET_CARRIED_ITEM_ID,
    USE_ITEM_ID,
    USE_ITEM_ON_ID,
};
use rustbukkit_protocol::v26_3::{
    CLIENTBOUND_CUSTOM_PAYLOAD_ID,
    CLIENTBOUND_FINISH_CONFIGURATION_ID,
    CLIENTBOUND_SELECT_KNOWN_PACKS_ID,
    CLIENTBOUND_UPDATE_ENABLED_FEATURES_ID,
    SERVERBOUND_ACCEPT_CODE_OF_CONDUCT_ID,
    SERVERBOUND_CLIENT_INFORMATION_ID,
    SERVERBOUND_CUSTOM_PAYLOAD_ID,
    SERVERBOUND_FINISH_CONFIGURATION_ID,
    SERVERBOUND_KEEP_ALIVE_ID,
    SERVERBOUND_PONG_ID,
    SERVERBOUND_SELECT_KNOWN_PACKS_ID,
};
use rustbukkit_protocol::{ClientboundPacket, VersionRegistry};

fn report(name: &str) -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../registry_data/26.3")
        .join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
    serde_json::from_slice(&bytes).unwrap_or_else(|error| panic!("parsing {}: {error}", path.display()))
}

fn id(report: &serde_json::Value, path: &[&str]) -> i32 {
    let mut value = report;
    for key in path {
        value = value
            .get(*key)
            .unwrap_or_else(|| panic!("missing {} in protocol report", path.join(".")));
    }
    value
        .as_i64()
        .unwrap_or_else(|| panic!("{} is not an integer", path.join("."))) as i32
}

#[test]
fn clientbound_ids_match_the_generated_26_3_report() {
    let packet_ids = report("packet_ids.json");
    let pack = VersionRegistry::select("26.3").unwrap();
    let cases: &[(ClientboundPacket, &str)] = &[
        (ClientboundPacket::JoinGame, "login"),
        (ClientboundPacket::KeepAlive, "keep_alive"),
        (ClientboundPacket::ChunkData, "level_chunk_with_light"),
        (ClientboundPacket::UnloadChunk, "forget_level_chunk"),
        (ClientboundPacket::PlayerPosition, "player_position"),
        (ClientboundPacket::ViewCenter, "set_chunk_cache_center"),
        (ClientboundPacket::SpawnPosition, "set_default_spawn_position"),
        (ClientboundPacket::SimulationDistance, "set_simulation_distance"),
        (ClientboundPacket::ServerData, "server_data"),
        (ClientboundPacket::UpdateHealth, "set_health"),
        (ClientboundPacket::UpdateTime, "set_time"),
        (ClientboundPacket::PlayerInfo, "player_info_update"),
        (ClientboundPacket::SpawnEntity, "add_entity"),
        (ClientboundPacket::EntityMetadata, "set_entity_data"),
        (ClientboundPacket::EntityVelocity, "set_entity_motion"),
        (ClientboundPacket::EntityTeleport, "teleport_entity"),
        (ClientboundPacket::EntityHeadRotation, "rotate_head"),
        (ClientboundPacket::RemoveEntities, "remove_entities"),
        (ClientboundPacket::Respawn, "respawn"),
        (ClientboundPacket::BlockUpdate, "block_update"),
    ];
    for (packet, name) in cases {
        assert_eq!(
            pack.clientbound_id(*packet),
            id(&packet_ids, &["play", "clientbound", name]),
            "clientbound ID for {name}"
        );
    }
}

#[test]
fn connection_state_ids_match_the_generated_26_3_report() {
    let packet_ids = report("packet_ids.json");
    let pack = VersionRegistry::select("26.3").unwrap();
    let login = pack.login_protocol().unwrap();
    let configuration = pack.configuration_protocol().unwrap();

    assert_eq!(pack.status_response_id(), id(&packet_ids, &["status", "clientbound", "status_response"]));
    assert_eq!(pack.status_pong_id(), id(&packet_ids, &["status", "clientbound", "pong_response"]));
    assert_eq!(login.login_success_id(), id(&packet_ids, &["login", "clientbound", "login_finished"]));
    assert_eq!(
        login.login_acknowledged_id(),
        id(&packet_ids, &["login", "serverbound", "login_acknowledged"])
    );
    assert_eq!(
        configuration.finish_configuration_id(),
        id(&packet_ids, &["configuration", "clientbound", "finish_configuration"])
    );
    assert_eq!(configuration.finish_configuration_id(), CLIENTBOUND_FINISH_CONFIGURATION_ID);

    for (constant, name) in [
        (CLIENTBOUND_CUSTOM_PAYLOAD_ID, "custom_payload"),
        (CLIENTBOUND_UPDATE_ENABLED_FEATURES_ID, "update_enabled_features"),
        (CLIENTBOUND_SELECT_KNOWN_PACKS_ID, "select_known_packs"),
    ] {
        assert_eq!(
            constant,
            id(&packet_ids, &["configuration", "clientbound", name]),
            "configuration clientbound {name}"
        );
    }
    for (constant, name) in [
        (SERVERBOUND_CLIENT_INFORMATION_ID, "client_information"),
        (SERVERBOUND_CUSTOM_PAYLOAD_ID, "custom_payload"),
        (SERVERBOUND_FINISH_CONFIGURATION_ID, "finish_configuration"),
        (SERVERBOUND_KEEP_ALIVE_ID, "keep_alive"),
        (SERVERBOUND_PONG_ID, "pong"),
        (SERVERBOUND_SELECT_KNOWN_PACKS_ID, "select_known_packs"),
        (SERVERBOUND_ACCEPT_CODE_OF_CONDUCT_ID, "accept_code_of_conduct"),
    ] {
        assert_eq!(
            constant,
            id(&packet_ids, &["configuration", "serverbound", name]),
            "configuration serverbound {name}"
        );
    }

    let expected = [
        CLIENTBOUND_CUSTOM_PAYLOAD_ID,
        CLIENTBOUND_UPDATE_ENABLED_FEATURES_ID,
        CLIENTBOUND_SELECT_KNOWN_PACKS_ID,
    ];
    let actual: Vec<i32> = configuration
        .initial_packets()
        .iter()
        .map(|(packet_id, _)| *packet_id)
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn serverbound_play_ids_match_the_generated_26_3_report() {
    let packet_ids = report("packet_ids.json");
    for (constant, name) in [
        (ACCEPT_TELEPORTATION_ID, "accept_teleportation"),
        (ATTACK_ID, "attack"),
        (CLIENT_COMMAND_ID, "client_command"),
        (CONTAINER_CLICK_ID, "container_click"),
        (PLAY_KEEP_ALIVE_ID, "keep_alive"),
        (MOVE_PLAYER_POS_ID, "move_player_pos"),
        (MOVE_PLAYER_POS_ROT_ID, "move_player_pos_rot"),
        (MOVE_PLAYER_ROT_ID, "move_player_rot"),
        (MOVE_PLAYER_STATUS_ID, "move_player_status_only"),
        (PLAYER_ACTION_ID, "player_action"),
        (SET_CARRIED_ITEM_ID, "set_carried_item"),
        (USE_ITEM_ON_ID, "use_item_on"),
        (USE_ITEM_ID, "use_item"),
    ] {
        assert_eq!(constant, id(&packet_ids, &["play", "serverbound", name]), "serverbound play {name}");
    }
}

#[test]
fn block_state_ids_match_the_generated_26_3_blocks_report() {
    let states = report("block_states.json");
    for (block, name) in [
        (BlockKind::Air, "air"),
        (BlockKind::Stone, "stone"),
        (BlockKind::GrassBlock, "grass_block"),
        (BlockKind::Dirt, "dirt"),
        (BlockKind::Bedrock, "bedrock"),
        (BlockKind::Water, "water"),
        (BlockKind::Sand, "sand"),
        (BlockKind::Cobblestone, "cobblestone"),
    ] {
        assert_eq!(
            block_state_id(block),
            states[name].as_i64().unwrap_or_else(|| panic!("missing {name}")) as i32,
            "block state for {name}"
        );
    }
}

#[test]
fn default_block_state_ids_are_unique_and_nonnegative() {
    let ids = rustbukkit_protocol::v26_3::chunk::default_block_state_ids();
    let values = [
        ids.air,
        ids.stone,
        ids.water,
        ids.bedrock,
        ids.dirt,
        ids.grass_block,
        ids.sand,
        ids.cobblestone,
    ];
    let unique: std::collections::HashSet<_> = values.iter().copied().collect();
    assert_eq!(unique.len(), values.len());
    assert!(values.iter().all(|id| *id >= 0));
}
