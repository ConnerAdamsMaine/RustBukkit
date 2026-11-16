use rustbukkit_chunks::{BlockKind, BlockPos, ChunkSnapshot};
use rustbukkit_inventory::ItemStack;
use uuid::Uuid;

use crate::version::{ConfigurationError, ConfigurationInbound, LoginError, PlayDecodeError, PlayInbound};

const MINECRAFT_OVERWORLD: &str = "minecraft:overworld";
const MINECRAFT_DIMENSIONS: &[&str] = &["minecraft:overworld", "minecraft:the_end", "minecraft:the_nether"];

pub struct V26_3LoginProtocol;

impl V26_3LoginProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Default for V26_3LoginProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::version::LoginProtocol for V26_3LoginProtocol {
    fn login_success_id(&self) -> i32 {
        0x02
    }

    fn login_acknowledged_id(&self) -> i32 {
        0x03
    }

    /// 26.3 `ClientboundLoginFinishedPacket` = `GameProfile` (UUID, name,
    /// properties) followed by the login session UUID.
    ///
    /// Captured from the official 26.3 server (`registry_data/26.3/
    /// golden_packets.json`): `uuid | name | varint(0 properties) | session
    /// uuid`. The previous encoder omitted the property count, which shifted
    /// every following byte and made unmodified clients reject login.
    fn login_success_payload(&self, player_uuid: Uuid, username: &str, session_uuid: Uuid) -> Vec<u8> {
        let mut writer = rustbukkit_encoding::Writer::new();
        writer.uuid(player_uuid);
        writer.string(username);
        writer.varint(0);
        writer.uuid(session_uuid);
        writer.finish()
    }

    fn parse_login_start(&self, payload: &[u8]) -> Result<String, LoginError> {
        super::parse_login_start(payload)
    }
}

pub struct V26_3ConfigurationProtocol;

impl V26_3ConfigurationProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Default for V26_3ConfigurationProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::version::ConfigurationProtocol for V26_3ConfigurationProtocol {
    fn initial_packets(&self) -> Vec<(i32, Vec<u8>)> {
        super::initial_configuration_packets()
    }

    fn registry_packets(&self) -> Vec<&'static [u8]> {
        super::configuration_registry_packets()
    }

    fn registry_data_id(&self) -> i32 {
        super::CLIENTBOUND_REGISTRY_DATA_ID
    }

    fn tags_packet(&self) -> Option<&'static [u8]> {
        Some(super::configuration_tags_packet())
    }

    fn update_tags_id(&self) -> i32 {
        super::CLIENTBOUND_UPDATE_TAGS_ID
    }

    fn finish_configuration_id(&self) -> i32 {
        super::CLIENTBOUND_FINISH_CONFIGURATION_ID
    }

    fn parse_inbound(&self, id: i32, payload: &[u8]) -> Result<ConfigurationInbound, ConfigurationError> {
        super::parse_configuration_packet(id, payload)
    }
}

pub struct V26_3PlayProtocol;

impl V26_3PlayProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Default for V26_3PlayProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::version::PlayProtocol for V26_3PlayProtocol {
    fn parse_play(&self, id: i32, payload: &[u8]) -> Result<PlayInbound, PlayDecodeError> {
        super::play::parse_play_packet(id, payload)
            .map_err(|e| {
                match e {
                    super::play::PlayPacketError::Wire(e) => PlayDecodeError::Wire(e),
                    _ => PlayDecodeError::InvalidPayload,
                }
            })
            .map(super::play::ServerboundPlay::into)
    }
}

pub struct V26_3;

impl crate::version::VersionPack for V26_3 {
    fn minecraft_version(&self) -> &'static str {
        "26.3"
    }

    fn protocol_version(&self) -> i32 {
        777
    }

    fn login_transition(&self) -> crate::version::LoginTransition {
        crate::version::LoginTransition::Configuration
    }

    /// Identifiers come from the 26.3 server's generated `packets.json` report
    /// (`registry_data/26.3/packet_ids.json`) and are asserted by
    /// `tests/protocol_ids_26_3.rs`; do not hand-edit from memory.
    fn clientbound_id(&self, packet: crate::version::ClientboundPacket) -> i32 {
        use crate::v26_3::{chunk, clientbound};
        use crate::version::ClientboundPacket;

        match packet {
            ClientboundPacket::StatusResponse => 0x00,
            ClientboundPacket::StatusPong => 0x01,
            ClientboundPacket::LoginDisconnect => 0x00,
            ClientboundPacket::LoginSuccess => 0x02,
            ClientboundPacket::JoinGame => clientbound::PLAY_LOGIN_ID,
            ClientboundPacket::KeepAlive => 45,
            ClientboundPacket::ChunkData => chunk::LEVEL_CHUNK_WITH_LIGHT_ID,
            ClientboundPacket::UnloadChunk => chunk::FORGET_CHUNK_ID,
            ClientboundPacket::PlayerPosition => clientbound::PLAYER_POSITION_ID,
            ClientboundPacket::ViewCenter => chunk::SET_CHUNK_CACHE_CENTER_ID,
            ClientboundPacket::SpawnPosition => clientbound::SPAWN_POSITION_ID,
            ClientboundPacket::SimulationDistance => 113,
            ClientboundPacket::ServerData => 88,
            ClientboundPacket::UpdateHealth => clientbound::SET_HEALTH_ID,
            ClientboundPacket::HurtAnimation => clientbound::HURT_ANIMATION_ID,
            ClientboundPacket::EntityEvent => clientbound::ENTITY_EVENT_ID,
            ClientboundPacket::UpdateTime => clientbound::TIME_ID,
            ClientboundPacket::PlayerInfo => 71,
            ClientboundPacket::SpawnEntity => 1,
            ClientboundPacket::EntityMetadata => 101,
            ClientboundPacket::EntityVelocity => 103,
            ClientboundPacket::EntityTeleport => 128,
            ClientboundPacket::EntityMove => 55,
            ClientboundPacket::EntityHeadRotation => 85,
            ClientboundPacket::RemoveEntities => 78,
            ClientboundPacket::Respawn => clientbound::RESPAWN_ID,
            ClientboundPacket::BlockUpdate => crate::v26_3::clientbound::BLOCK_UPDATE_ID,
            ClientboundPacket::ContainerClose => clientbound::CONTAINER_CLOSE_ID,
            ClientboundPacket::ContainerSetContent => clientbound::CONTAINER_SET_CONTENT_ID,
            ClientboundPacket::ContainerSetSlot => clientbound::CONTAINER_SET_SLOT_ID,
            ClientboundPacket::SetCursorItem => clientbound::SET_CURSOR_ITEM_ID,
            ClientboundPacket::SetHeldSlot => clientbound::SET_HELD_SLOT_ID,
        }
    }

    fn status_response_id(&self) -> i32 {
        0x00
    }

    fn status_pong_id(&self) -> i32 {
        0x01
    }

    fn login_protocol(&self) -> Option<&dyn crate::version::LoginProtocol> {
        Some(&V26_3_LOGIN_PROTOCOL)
    }

    fn configuration_protocol(&self) -> Option<&dyn crate::version::ConfigurationProtocol> {
        Some(&V26_3_CONFIGURATION_PROTOCOL)
    }

    fn play_protocol(&self) -> Option<&dyn crate::version::PlayProtocol> {
        Some(&V26_3_PLAY_PROTOCOL)
    }

    fn parse_login_start(&self, payload: &[u8]) -> Result<String, LoginError> {
        super::parse_login_start(payload)
    }

    fn join_game_payload(
        &self,
        entity_id: i32,
        max_players: u32,
        view_distance: u8,
        simulation_distance: u8,
        seed: i64,
        sea_level: i32,
    ) -> Vec<u8> {
        super::clientbound::encode_play_login(super::clientbound::PlayLogin {
            entity_id,
            hardcore: false,
            // The official 26.3 server advertises the three vanilla
            // dimensions in this order; dimension type 0 is `overworld`
            // (verified in `registry_data/26.3/golden_packets.json`).
            dimensions: MINECRAFT_DIMENSIONS,
            max_players: max_players as i32,
            view_distance: view_distance as i32,
            simulation_distance: simulation_distance as i32,
            reduced_debug_info: false,
            show_death_screen: true,
            limited_crafting: false,
            spawn: super::clientbound::CommonSpawnInfo {
                dimension_type_id: 0,
                dimension: MINECRAFT_OVERWORLD,
                seed,
                game_mode: super::clientbound::GameMode::Survival,
                previous_game_mode: None,
                is_debug: false,
                is_flat: false,
                last_death_location: None,
                portal_cooldown: 0,
                sea_level,
            },
            online_mode: false,
            enforces_secure_chat: false,
        })
        .expect("static dimension list and validated join parameters")
    }

    fn chunk_payload(&self, chunk: &ChunkSnapshot, x: i32, z: i32) -> Vec<u8> {
        super::chunk::encode_chunk_with_light(
            x,
            z,
            chunk,
            super::chunk::default_block_state_ids(),
            super::chunk::PLAINS_BIOME_ID,
            &super::chunk::full_skylight(),
        )
        .expect("16x16 chunk snapshot with bounded palette and light encodes within the packet limit")
    }

    fn view_center_payload(&self, x: i32, z: i32) -> Vec<u8> {
        super::chunk::encode_set_chunk_cache_center(x, z)
    }

    fn forget_chunk_payload(&self, x: i32, z: i32) -> Vec<u8> {
        super::chunk::encode_forget_chunk(x, z)
    }

    fn block_update_payload(&self, position: BlockPos, block: BlockKind) -> Vec<u8> {
        super::clientbound::encode_block_update(position, super::chunk::block_state_id(block))
    }

    fn container_set_content_payload(
        &self,
        container_id: i32,
        state_id: i32,
        items: &[Option<ItemStack>],
        carried: Option<ItemStack>,
    ) -> Vec<u8> {
        super::clientbound::encode_container_set_content(container_id, state_id, items, carried)
            .expect("a 46-slot player inventory with bounded stacks encodes within the packet limit")
    }

    fn container_set_slot_payload(
        &self,
        container_id: i32,
        state_id: i32,
        slot: i16,
        item: Option<ItemStack>,
    ) -> Vec<u8> {
        super::clientbound::encode_container_set_slot(container_id, state_id, slot, item)
    }

    fn set_cursor_item_payload(&self, item: Option<ItemStack>) -> Vec<u8> {
        super::clientbound::encode_set_cursor_item(item)
    }

    fn set_held_slot_payload(&self, slot: u8) -> Vec<u8> {
        super::clientbound::encode_set_held_slot(slot)
    }

    fn container_close_payload(&self, container_id: i32) -> Vec<u8> {
        super::clientbound::encode_container_close(container_id)
    }

    fn player_position_payload(&self, teleport_id: i32, position: [f64; 3], yaw: f32, pitch: f32) -> Vec<u8> {
        super::clientbound::encode_player_position(super::clientbound::PlayerPosition {
            teleport_id,
            position,
            delta: [0.0; 3],
            yaw,
            pitch,
            relative_flags: 0,
        })
    }

    fn spawn_position_payload(&self, position: BlockPos, yaw: f32, pitch: f32) -> Vec<u8> {
        super::clientbound::encode_spawn_position(MINECRAFT_OVERWORLD, position, yaw, pitch)
            .expect("static dimension identifier and world-bounded spawn position")
    }

    fn server_data_payload(&self, motd: &str, icon: Option<&[u8]>) -> Vec<u8> {
        super::clientbound::encode_server_data(&super::registry::NbtTag::String(motd.to_owned()), icon)
            .expect("a MOTD string and icon bounded by the packet limit encode successfully")
    }

    fn player_info_add_payload(&self, entries: &[(Uuid, &str)]) -> Vec<u8> {
        super::clientbound::encode_player_info_add(entries)
    }

    fn add_player_payload(
        &self,
        entity_id: i32,
        uuid: Uuid,
        position: [f64; 3],
        yaw: f32,
        pitch: f32,
    ) -> Vec<u8> {
        super::clientbound::encode_add_player(entity_id, uuid, position, yaw, pitch)
    }

    fn remove_entities_payload(&self, entity_ids: &[i32]) -> Vec<u8> {
        super::clientbound::encode_remove_entities(entity_ids)
    }

    fn move_entity_payload(
        &self,
        entity_id: i32,
        delta: [i16; 3],
        yaw: f32,
        pitch: f32,
        on_ground: bool,
    ) -> Vec<u8> {
        super::clientbound::encode_move_entity(entity_id, delta, yaw, pitch, on_ground)
    }

    fn time_payload(&self, game_time: i64) -> Vec<u8> {
        super::clientbound::encode_time(
            game_time,
            &[super::clientbound::ClockUpdate {
                registry_id:  0,
                ticks:        game_time,
                partial_tick: 0.0,
                rate:         1.0,
            }],
        )
        .expect("single clock entry is bounded")
    }

    fn update_health_payload(&self, health: f32, food: i32, saturation: f32) -> Vec<u8> {
        super::clientbound::encode_set_health(health, food, saturation)
            .expect("survival health/food/saturation are finite, non-negative values")
    }

    fn hurt_animation_payload(&self, entity_id: i32, yaw: f32) -> Vec<u8> {
        super::clientbound::encode_hurt_animation(entity_id, yaw)
    }

    fn entity_event_payload(&self, entity_id: i32, event: i8) -> Vec<u8> {
        super::clientbound::encode_entity_event(entity_id, event)
    }

    fn entity_velocity_payload(&self, entity_id: i32, velocity: [f64; 3]) -> Vec<u8> {
        super::clientbound::encode_set_entity_motion(entity_id, velocity)
    }

    fn respawn_payload(&self, data_to_keep: u8, seed: i64, sea_level: i32) -> Vec<u8> {
        super::clientbound::encode_respawn(
            super::clientbound::CommonSpawnInfo {
                // Matches the join payload's static overworld spawn context.
                dimension_type_id: 0,
                dimension: MINECRAFT_OVERWORLD,
                seed,
                game_mode: super::clientbound::GameMode::Survival,
                previous_game_mode: None,
                is_debug: false,
                is_flat: false,
                last_death_location: None,
                portal_cooldown: 0,
                sea_level,
            },
            data_to_keep,
        )
        .expect("static dimension list and world-bounded respawn parameters")
    }

    fn parse_play(&self, id: i32, payload: &[u8]) -> Result<PlayInbound, PlayDecodeError> {
        super::play::parse_play_packet(id, payload)
            .map_err(|e| {
                match e {
                    super::play::PlayPacketError::Wire(e) => PlayDecodeError::Wire(e),
                    _ => PlayDecodeError::InvalidPayload,
                }
            })
            .map(super::play::ServerboundPlay::into)
    }
}

static V26_3_LOGIN_PROTOCOL: V26_3LoginProtocol = V26_3LoginProtocol;
static V26_3_CONFIGURATION_PROTOCOL: V26_3ConfigurationProtocol = V26_3ConfigurationProtocol;
static V26_3_PLAY_PROTOCOL: V26_3PlayProtocol = V26_3PlayProtocol;
