use rustbukkit_chunks::{BlockKind, BlockPos, ChunkSnapshot};
use rustbukkit_decoding::WireError;
use rustbukkit_inventory::ItemStack;
use thiserror::Error;
use uuid::Uuid;

use crate::v26_3::protocol::V26_3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginTransition {
    Play,
    Configuration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientboundPacket {
    StatusResponse,
    StatusPong,
    LoginDisconnect,
    LoginSuccess,
    JoinGame,
    KeepAlive,
    ChunkData,
    UnloadChunk,
    PlayerPosition,
    ViewCenter,
    SpawnPosition,
    SimulationDistance,
    ServerData,
    UpdateHealth,
    HurtAnimation,
    EntityEvent,
    UpdateTime,
    PlayerInfo,
    SpawnEntity,
    EntityMetadata,
    EntityVelocity,
    EntityTeleport,
    EntityMove,
    EntityHeadRotation,
    RemoveEntities,
    Respawn,
    BlockUpdate,
    ContainerClose,
    ContainerSetContent,
    ContainerSetSlot,
    SetCursorItem,
    SetHeldSlot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DigAction {
    Start,
    Cancel,
    Finish,
    DropStack,
    DropOne,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockFace {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatVisibility {
    Full,
    System,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainHand {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParticleStatus {
    All,
    Decreased,
    Minimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerPose {
    Standing,
    FallFlying,
    Sleeping,
    Swimming,
    SpinAttack,
    Crouching,
    LongJump,
    Dying,
    Croaking,
    UsingTongue,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityVelocity {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemEntitySpawn {
    pub entity_id: i32,
    pub uuid:      Uuid,
    pub x:         f64,
    pub y:         f64,
    pub z:         f64,
    pub velocity:  EntityVelocity,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresenceChange<'a> {
    pub entity_id: i32,
    pub uuid:      Uuid,
    pub username:  &'a str,
    pub pose:      PlayerPose,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClientInformation {
    pub locale:                String,
    pub view_distance:         i8,
    pub chat_visibility:       ChatVisibility,
    pub chat_colors:           bool,
    pub skin_parts:            u8,
    pub main_hand:             MainHand,
    pub text_filtering:        bool,
    pub allows_server_listing: bool,
    pub particle_status:       ParticleStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfigurationInbound {
    ClientInformation(ClientInformation),
    CustomPayload,
    Finish,
    KeepAlive(i64),
    Pong(i32),
    KnownPacks { has_core: bool },
    AcceptCodeOfConduct,
}

#[derive(Debug, Error)]
pub enum ConfigurationError {
    #[error(transparent)]
    Wire(#[from] WireError),
    #[error("invalid configuration packet: {0}")]
    Invalid(&'static str),
    #[error("unexpected configuration packet {0}")]
    UnexpectedPacket(i32),
}
#[derive(Debug, Clone, PartialEq)]
pub enum PlayInbound {
    TeleportConfirm(i32),
    KeepAlive(i64),
    Position {
        x: f64,
        y: f64,
        z: f64,
    },
    PositionAndLook {
        x:         f64,
        y:         f64,
        z:         f64,
        yaw:       f32,
        pitch:     f32,
        on_ground: bool,
    },
    Look {
        yaw:       f32,
        pitch:     f32,
        on_ground: bool,
    },
    OnGround(bool),
    Dig {
        action:   DigAction,
        position: BlockPos,
        sequence: i32,
    },
    Place {
        target:    BlockPos,
        face:      BlockFace,
        main_hand: bool,
        sequence:  i32,
    },
    HeldSlot(u8),
    /// Full serverbound `container_click` data. The client's changed-slot list
    /// and carried item are predictions only; the server computes the
    /// authoritative result from `slot`, `button` and `mode`.
    ClickContainer {
        container_id: i32,
        state_id:     i32,
        slot:         i16,
        button:       i8,
        mode:         rustbukkit_inventory::ClickMode,
    },
    Attack {
        target_entity_id: i32,
        sneaking:         bool,
    },
    RespawnRequest,
    UseItem {
        hand:     crate::v26_3::play::Hand,
        sequence: i32,
        yaw:      f32,
        pitch:    f32,
    },
    PlayerAction {
        action:    crate::v26_3::play::PlayerAction,
        position:  (i32, i32, i32),
        direction: BlockFace,
        sequence:  i32,
    },
    ClientCommand(crate::v26_3::play::ClientCommand),
    MovementStatus(crate::v26_3::play::MovementFlags),
    Other(i32),
}

#[derive(Debug, Error)]
pub enum PlayDecodeError {
    #[error(transparent)]
    Wire(#[from] WireError),
    #[error("invalid play packet payload")]
    InvalidPayload,
}

pub trait LoginProtocol: Send + Sync {
    fn login_success_id(&self) -> i32;
    /// Serverbound packet that moves the connection from Login to Configuration.
    fn login_acknowledged_id(&self) -> i32;
    fn login_success_payload(&self, player_uuid: Uuid, username: &str, session_uuid: Uuid) -> Vec<u8>;
    fn parse_login_start(&self, payload: &[u8]) -> Result<String, LoginError>;
}

pub trait ConfigurationProtocol: Send + Sync {
    fn initial_packets(&self) -> Vec<(i32, Vec<u8>)>;
    /// Registry Data payloads in the order the client must receive them.
    fn registry_packets(&self) -> Vec<&'static [u8]>;
    /// Clientbound Registry Data packet identifier.
    fn registry_data_id(&self) -> i32;
    /// Update Tags payload sent after registry data, if the version uses tags.
    fn tags_packet(&self) -> Option<&'static [u8]>;
    /// Clientbound Update Tags packet identifier.
    fn update_tags_id(&self) -> i32;
    /// Clientbound packet that finishes the configuration phase.
    fn finish_configuration_id(&self) -> i32;
    fn parse_inbound(&self, id: i32, payload: &[u8]) -> Result<ConfigurationInbound, ConfigurationError>;
}

pub trait PlayProtocol: Send + Sync {
    fn parse_play(&self, id: i32, payload: &[u8]) -> Result<PlayInbound, PlayDecodeError>;
}

/// Version-owned packet construction.
///
/// Gameplay code must not hand-assemble packet payloads: packet identifiers and
/// field layouts are version facts and belong to the selected version pack.
pub trait VersionPack: Send + Sync {
    fn minecraft_version(&self) -> &'static str;
    fn protocol_version(&self) -> i32;
    fn login_transition(&self) -> LoginTransition;
    fn clientbound_id(&self, packet: ClientboundPacket) -> i32;
    fn status_response_id(&self) -> i32;
    fn status_pong_id(&self) -> i32;
    fn login_protocol(&self) -> Option<&dyn LoginProtocol>;
    fn configuration_protocol(&self) -> Option<&dyn ConfigurationProtocol>;
    fn play_protocol(&self) -> Option<&dyn PlayProtocol>;
    fn parse_login_start(&self, payload: &[u8]) -> Result<String, LoginError>;
    fn join_game_payload(
        &self,
        entity_id: i32,
        max_players: u32,
        view_distance: u8,
        simulation_distance: u8,
        seed: i64,
        sea_level: i32,
    ) -> Vec<u8>;
    fn chunk_payload(&self, chunk: &ChunkSnapshot, x: i32, z: i32) -> Vec<u8>;
    fn view_center_payload(&self, x: i32, z: i32) -> Vec<u8>;
    fn forget_chunk_payload(&self, x: i32, z: i32) -> Vec<u8>;
    fn block_update_payload(&self, position: BlockPos, block: BlockKind) -> Vec<u8>;
    /// `container_set_content`: full authoritative window sync.
    fn container_set_content_payload(
        &self,
        container_id: i32,
        state_id: i32,
        items: &[Option<ItemStack>],
        carried: Option<ItemStack>,
    ) -> Vec<u8>;
    /// `container_set_slot`: single authoritative slot correction.
    fn container_set_slot_payload(
        &self,
        container_id: i32,
        state_id: i32,
        slot: i16,
        item: Option<ItemStack>,
    ) -> Vec<u8>;
    /// `set_cursor_item`: the cursor (carried) stack.
    fn set_cursor_item_payload(&self, item: Option<ItemStack>) -> Vec<u8>;
    /// `set_held_slot`: the selected hotbar slot.
    fn set_held_slot_payload(&self, slot: u8) -> Vec<u8>;
    /// `container_close`: closes a container window on the client.
    fn container_close_payload(&self, container_id: i32) -> Vec<u8>;
    fn player_position_payload(&self, teleport_id: i32, position: [f64; 3], yaw: f32, pitch: f32) -> Vec<u8>;
    fn spawn_position_payload(&self, position: BlockPos, yaw: f32, pitch: f32) -> Vec<u8>;
    fn time_payload(&self, game_time: i64) -> Vec<u8>;
    /// `set_health`: victim health/food/saturation; health 0 triggers the
    /// client's death screen.
    fn update_health_payload(&self, health: f32, food: i32, saturation: f32) -> Vec<u8>;
    /// `hurt_animation`: hurt tilt broadcast for an entity, victim included.
    fn hurt_animation_payload(&self, entity_id: i32, yaw: f32) -> Vec<u8>;
    /// `entity_event`: version-owned entity event broadcast (combat states).
    fn entity_event_payload(&self, entity_id: i32, event: i8) -> Vec<u8>;
    /// `set_entity_motion`: authoritative velocity push (knockback).
    fn entity_velocity_payload(&self, entity_id: i32, velocity: [f64; 3]) -> Vec<u8>;
    /// `respawn`: dimension/state reset after death, before the position sync.
    fn respawn_payload(&self, data_to_keep: u8, seed: i64, sea_level: i32) -> Vec<u8>;
    /// 26.3 Server Data: a text Component followed by an optional icon PNG.
    fn server_data_payload(&self, motd: &str, icon: Option<&[u8]>) -> Vec<u8>;
    fn player_info_add_payload(&self, entries: &[(Uuid, &str)]) -> Vec<u8>;
    fn add_player_payload(
        &self,
        entity_id: i32,
        uuid: Uuid,
        position: [f64; 3],
        yaw: f32,
        pitch: f32,
    ) -> Vec<u8>;
    fn remove_entities_payload(&self, entity_ids: &[i32]) -> Vec<u8>;
    fn move_entity_payload(
        &self,
        entity_id: i32,
        delta: [i16; 3],
        yaw: f32,
        pitch: f32,
        on_ground: bool,
    ) -> Vec<u8>;
    fn parse_play(&self, id: i32, payload: &[u8]) -> Result<PlayInbound, PlayDecodeError>;
}

#[derive(Debug, Error)]
pub enum LoginError {
    #[error(transparent)]
    Wire(#[from] WireError),
    #[error("invalid username")]
    InvalidUsername,
    #[error("invalid Login Start data")]
    InvalidPayload,
}

pub struct VersionRegistry;

#[derive(Debug, Error)]
pub enum VersionSelectionError {
    #[error("Unsupported Minecraft version \"{0}\". Supported versions: 26.3.")]
    Unsupported(String),
}

static V26_3_PACK: V26_3 = V26_3;
// V26_3_LOGIN/CONFIGURATION/PLAY_PROTOCOL statics live in the v26_3 submodule.
// They are referenced by the V26_3 VersionPack impl's methods there.

impl VersionRegistry {
    pub fn select(version: &str) -> Result<&'static dyn VersionPack, VersionSelectionError> {
        match version {
            "26.3" => Ok(&V26_3_PACK),
            other => Err(VersionSelectionError::Unsupported(other.to_owned())),
        }
    }
}

pub fn validate_username(username: &str) -> Result<(), LoginError> {
    if username.is_empty() || username.len() > 16 {
        return Err(LoginError::InvalidUsername);
    }
    for c in username.chars() {
        if !c.is_ascii_alphanumeric() && c != '_' {
            return Err(LoginError::InvalidUsername);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{LoginTransition, VersionRegistry};

    #[test]
    fn selects_exact_release_and_its_state_sequence() {
        let pack = VersionRegistry::select("26.3").unwrap();
        assert_eq!(pack.protocol_version(), 777);
        assert_eq!(pack.login_transition(), LoginTransition::Configuration);
        assert!(VersionRegistry::select("1.19.2").is_err());
        assert!(VersionRegistry::select("1.18.2").is_err());
    }

    #[test]
    fn clientbound_ids_come_from_the_verified_encoder_constants() {
        use crate::v26_3::{chunk, clientbound};

        let pack = VersionRegistry::select("26.3").unwrap();
        assert_eq!(pack.clientbound_id(super::ClientboundPacket::JoinGame), clientbound::PLAY_LOGIN_ID);
        assert_eq!(
            pack.clientbound_id(super::ClientboundPacket::ChunkData),
            chunk::LEVEL_CHUNK_WITH_LIGHT_ID
        );
        assert_eq!(pack.clientbound_id(super::ClientboundPacket::UnloadChunk), chunk::FORGET_CHUNK_ID);
        assert_eq!(
            pack.clientbound_id(super::ClientboundPacket::ViewCenter),
            chunk::SET_CHUNK_CACHE_CENTER_ID
        );
        assert_eq!(
            pack.clientbound_id(super::ClientboundPacket::PlayerPosition),
            clientbound::PLAYER_POSITION_ID
        );
        assert_eq!(
            pack.clientbound_id(super::ClientboundPacket::SpawnPosition),
            clientbound::SPAWN_POSITION_ID
        );
        assert_eq!(pack.clientbound_id(super::ClientboundPacket::UpdateTime), clientbound::TIME_ID);
    }
}
