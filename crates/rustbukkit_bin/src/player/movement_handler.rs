use std::fmt::Display;

use anyhow::Result;

use crate::network::PacketReader;
use crate::player::{Vec2, Vec3};

/// Player movement packet types
#[derive(Debug, Clone, Copy)]
pub struct PlayerPosition<N64: From<f64> + Display> {
    pub coordinates: Vec3<N64>,
    // pub x:      f64,
    // pub y:      f64,
    // pub z:      f64,
    pub ground:      bool,
}

#[derive(Debug, Clone, Copy)]
pub struct PlayerLook<N32: From<f32> + Display> {
    pub rotation: Vec2<N32>,
    // pub yaw:    f32,
    // pub pitch:  f32,
    pub ground:   bool,
}

#[derive(Debug, Clone, Copy)]
pub struct PlayerPositionAndLook<N64, N32>
where
    N64: From<f64> + Display,
    N32: From<f32> + Display,
{
    // pub x:      f64,
    // pub y:      f64,
    // pub z:      f64,
    pub coordinates: Vec3<N64>,
    pub rotation:    Vec2<N32>,
    // pub yaw:         f32,
    // pub pitch:       f32,
    pub ground:      bool,
}

/// Parse movement packets from client
pub fn parse_movement_packet(packet_id: i32, data: &[u8]) -> Result<Option<MovementPacket>> {
    match packet_id {
        0x04 => {
            // Player Position packet
            let mut reader = PacketReader::new(data);
            let coordinates =
                Vec3::from((reader.read_double()?, reader.read_double()?, reader.read_double()?));
            let ground = reader.read_bool()?;
            Ok(Some(MovementPacket::Position(PlayerPosition { coordinates, ground })))
        }
        0x05 => {
            // Player Look packet
            let mut reader = PacketReader::new(data);
            let ground = reader.read_bool()?;
            let rotation = Vec2::from((reader.read_float()?, reader.read_float()?));
            Ok(Some(MovementPacket::Look(PlayerLook { rotation, ground })))
        }
        0x06 => {
            // Player Position and Look packet
            let mut reader = PacketReader::new(data);
            let coordinates =
                Vec3::from((reader.read_double()?, reader.read_double()?, reader.read_double()?));
            let rotation = Vec2::from((reader.read_float()?, reader.read_float()?));
            let ground = reader.read_bool()?;

            Ok(Some(MovementPacket::PositionAndLook(PlayerPositionAndLook {
                coordinates,
                rotation,
                ground,
            })))
        }
        _ => {
            // Other packets we don't handle yet
            Ok(None)
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum MovementPacket<N64 = f64, N32 = f32>
where
    N64: From<f64> + Display + Copy,
    N32: From<f32> + Display + Copy,
{
    Position(PlayerPosition<N64>),
    Look(PlayerLook<N32>),
    PositionAndLook(PlayerPositionAndLook<N64, N32>),
}

impl<N64, N32> MovementPacket<N64, N32>
where
    N64: From<f64> + Display + Copy,
    N32: From<f32> + Display + Copy,
{
    pub fn from_xyz(x: N64, y: N64, z: N64, ground: bool) -> Self {
        MovementPacket::Position(PlayerPosition {
            coordinates: Vec3::new(x, y, z),
            ground,
        })
    }
}

impl<N64, N32> MovementPacket<N64, N32>
where
    N64: From<f64> + Display + Copy,
    N32: From<f32> + Display + Copy,
{
    pub fn from_yaw_pitch(yaw: N32, pitch: N32, ground: bool) -> Self {
        MovementPacket::Look(PlayerLook {
            rotation: Vec2::new(yaw, pitch),
            ground,
        })
    }
}

impl<N64, N32> MovementPacket<N64, N32>
where
    N64: From<f64> + Display + Copy,
    N32: From<f32> + Display + Copy,
{
    pub fn from_xyz_yaw_pitch(x: N64, y: N64, z: N64, yaw: N32, pitch: N32, ground: bool) -> Self {
        MovementPacket::PositionAndLook(PlayerPositionAndLook {
            coordinates: Vec3::new(x, y, z),
            rotation: Vec2::new(yaw, pitch),
            ground,
        })
    }
}

impl<N64, N32> MovementPacket<N64, N32>
where
    N64: From<f64> + Display + Copy,
    N32: From<f32> + Display + Copy,
{
    pub fn is_on_ground(&self) -> bool {
        match self {
            MovementPacket::Position(pos) => pos.ground,
            MovementPacket::Look(look) => look.ground,
            MovementPacket::PositionAndLook(pos_look) => pos_look.ground,
        }
    }
}

impl MovementPacket {
    pub fn new_position<N64: From<f64> + Into<f64>>(
        //
        x: N64,
        y: N64,
        z: N64,
        ground: bool,
    ) -> Self {
        MovementPacket::Position(PlayerPosition {
            coordinates: Vec3::from((x.into(), y.into(), z.into())),
            ground,
        })
    }

    pub fn new_look<N32: From<f32> + Into<f32>>(
        //
        yaw: N32,
        pitch: N32,
        ground: bool,
    ) -> Self {
        MovementPacket::Look(PlayerLook {
            rotation: Vec2::from((yaw.into(), pitch.into())),
            ground,
        })
    }

    pub fn new_position_and_look<N64: From<f64> + Into<f64>, N32: From<f32> + Into<f32>>(
        x: N64,
        y: N64,
        z: N64,
        yaw: N32,
        pitch: N32,
        ground: bool,
    ) -> Self {
        MovementPacket::PositionAndLook(PlayerPositionAndLook {
            coordinates: Vec3::from((x.into(), y.into(), z.into())),
            rotation: Vec2::from((yaw.into(), pitch.into())),
            ground,
        })
    }
}

impl<N> From<(N, N, N)> for MovementPacket
where
    N: From<f64> + Into<f64>,
{
    fn from(tuple: (N, N, N)) -> Self {
        MovementPacket::new_position(tuple.0.into(), tuple.1.into(), tuple.2.into(), false)
    }
}

impl<N> From<(N, N)> for MovementPacket
where
    N: From<f32> + Into<f32>,
{
    fn from(tuple: (N, N)) -> Self {
        MovementPacket::new_look(tuple.0.into(), tuple.1.into(), false)
    }
}

impl From<MovementPacket> for Vec3<f64> {
    fn from(packet: MovementPacket) -> Self {
        match packet {
            MovementPacket::Position(pos) => pos.coordinates,
            MovementPacket::PositionAndLook(pos_look) => pos_look.coordinates,
            _ => Vec3::new(0.0, 0.0, 0.0), // Default value for non-position packets
        }
    }
}

impl From<MovementPacket> for Vec2<f32> {
    fn from(packet: MovementPacket) -> Self {
        match packet {
            MovementPacket::Look(look) => look.rotation,
            MovementPacket::PositionAndLook(pos_look) => pos_look.rotation,
            _ => Vec2::new(0.0, 0.0), // Default value for non-look packets
        }
    }
}

impl<N> From<Vec3<N>> for MovementPacket
where
    N: From<f64> + Into<f64>,
{
    fn from(vec: Vec3<N>) -> Self {
        MovementPacket::new_position(vec.x.into(), vec.y.into(), vec.z.into(), false)
    }
}

impl<N> From<Vec2<N>> for MovementPacket
where
    N: From<f32> + Into<f32>,
{
    fn from(vec: Vec2<N>) -> Self {
        MovementPacket::new_look(vec.yaw.into(), vec.pitch.into(), false)
    }
}
