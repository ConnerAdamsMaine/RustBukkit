use rustbukkit_decoding::WireError;
use thiserror::Error;

use crate::v1_19_2;

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
    UpdateTime,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayInbound {
    TeleportConfirm(i32),
    KeepAlive(i64),
    Position { x: f64, y: f64, z: f64 },
    Look,
    OnGround,
    Other(i32),
}

#[derive(Debug, Error)]
pub enum PlayDecodeError {
    #[error(transparent)]
    Wire(#[from] WireError),
    #[error("invalid play packet payload")]
    InvalidPayload,
}

pub trait VersionPack: Send + Sync {
    fn minecraft_version(&self) -> &'static str;
    fn protocol_version(&self) -> i32;
    fn login_transition(&self) -> LoginTransition;
    fn clientbound_id(&self, packet: ClientboundPacket) -> i32;
    fn parse_login_start(&self, payload: &[u8]) -> Result<String, LoginError>;
    fn join_game_payload(&self, max_players: u32, view_distance: u8, simulation_distance: u8) -> Vec<u8>;
    fn chunk_payload(&self, x: i32, z: i32) -> Vec<u8>;
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
    #[error(
        "Unsupported Minecraft version '{0}'. Supported versions begin at 1.19.2; currently implemented: 1.19.2."
    )]
    Unsupported(String),
}

struct V1_19_2;

impl VersionPack for V1_19_2 {
    fn minecraft_version(&self) -> &'static str {
        "1.19.2"
    }

    fn protocol_version(&self) -> i32 {
        760
    }

    fn login_transition(&self) -> LoginTransition {
        LoginTransition::Play
    }

    fn clientbound_id(&self, packet: ClientboundPacket) -> i32 {
        match packet {
            ClientboundPacket::StatusResponse => 0x00,
            ClientboundPacket::StatusPong => 0x01,
            ClientboundPacket::LoginDisconnect => 0x00,
            ClientboundPacket::LoginSuccess => 0x02,
            ClientboundPacket::JoinGame => 0x25,
            ClientboundPacket::KeepAlive => 0x20,
            ClientboundPacket::ChunkData => 0x21,
            ClientboundPacket::UnloadChunk => 0x1c,
            ClientboundPacket::PlayerPosition => 0x39,
            ClientboundPacket::ViewCenter => 0x4b,
            ClientboundPacket::SpawnPosition => 0x4d,
            ClientboundPacket::SimulationDistance => 0x5a,
            ClientboundPacket::ServerData => 0x42,
            ClientboundPacket::UpdateHealth => 0x55,
            ClientboundPacket::UpdateTime => 0x5c,
        }
    }

    fn parse_login_start(&self, payload: &[u8]) -> Result<String, LoginError> {
        v1_19_2::parse_login_start(payload)
    }

    fn join_game_payload(&self, max_players: u32, view_distance: u8, simulation_distance: u8) -> Vec<u8> {
        v1_19_2::join_game_payload(max_players, view_distance, simulation_distance)
    }

    fn chunk_payload(&self, x: i32, z: i32) -> Vec<u8> {
        v1_19_2::chunk_payload(x, z)
    }

    fn parse_play(&self, id: i32, payload: &[u8]) -> Result<PlayInbound, PlayDecodeError> {
        v1_19_2::parse_play(id, payload)
    }
}

static V1_19_2_PACK: V1_19_2 = V1_19_2;

impl VersionRegistry {
    pub fn select(version: &str) -> Result<&'static dyn VersionPack, VersionSelectionError> {
        match version {
            "1.19.2" => Ok(&V1_19_2_PACK),
            other => Err(VersionSelectionError::Unsupported(other.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LoginTransition, VersionRegistry};

    #[test]
    fn selects_exact_release_and_its_state_sequence() {
        let pack = VersionRegistry::select("1.19.2").unwrap();
        assert_eq!(pack.protocol_version(), 760);
        assert_eq!(pack.login_transition(), LoginTransition::Play);
        assert!(VersionRegistry::select("1.18.2").is_err());
        assert!(VersionRegistry::select("1.19.3").is_err());
    }
}
