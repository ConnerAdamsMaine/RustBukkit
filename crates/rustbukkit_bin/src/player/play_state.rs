#![allow(dead_code)]

use anyhow::Result;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::network::{ByteWritable, PacketWriter, write_varint};
use crate::player::{Vec2, Vec3};

pub struct PlayStateHandler;

impl PlayStateHandler {
    /// Send Confirm Teleport/Position packet (0x00 in Play state)
    /// This acknowledges to the client that their position has been confirmed by the server
    pub async fn send_confirm_teleport(stream: &mut TcpStream, teleport_id: i32) -> Result<()> {
        let mut writer = PacketWriter::new();

        // Write the teleport ID (used to match with the client's request)
        writer.write_varint(teleport_id);

        let packet_data = writer.finish();
        let packet_id = write_varint(0x00); // Confirm Teleport packet ID in Play state
        let packet_length = (packet_id.len() + packet_data.len()) as i32;

        // Write packet: [length][id][data]
        let mut frame = Vec::new();
        frame.extend_from_slice(&write_varint(packet_length));
        frame.extend_from_slice(&packet_id);
        frame.extend_from_slice(&packet_data);

        #[cfg(feature = "dev-sdk")]
        let _ = &crate::LOGGER.log_server_packet(&frame);

        stream.write_all(&frame).await?;
        stream.flush().await?;

        Ok(())
    }

    /// Send Set Default Spawn Position packet (0x4E in Play state)
    /// Tells the client where to respawn when they die
    pub async fn send_set_default_spawn_position<N: Into<i32>>(
        stream: &mut TcpStream,
        x: N,
        y: N,
        z: N,
        angle: f32,
    ) -> Result<()> {
        let mut writer = PacketWriter::new();

        // Position (as a combined int: x << 38 | (z & 0x3FFFFFF) << 12 | (y & 0xFFF))
        // For 1.21.7, this is sent as X (i32), Y (i32), Z (i32)
        writer.write_int(x);
        writer.write_int(y);
        writer.write_int(z);

        // Angle (rotation in degrees, 0-360, as a float)
        writer.write_float(angle);

        let packet_data = writer.finish();
        let packet_id = write_varint(0x4E); // Set Default Spawn Position packet ID
        let packet_length = (packet_id.len() + packet_data.len()) as i32;

        // Write packet: [length][id][data]
        let mut frame = Vec::new();
        frame.extend_from_slice(&write_varint(packet_length));
        frame.extend_from_slice(&packet_id);
        frame.extend_from_slice(&packet_data);

        #[cfg(feature = "dev-sdk")]
        let _ = &crate::LOGGER.log_server_packet(&frame);

        stream.write_all(&frame).await?;
        stream.flush().await?;

        Ok(())
    }

    /// Send Player Position And Look packet (0x28 in Play state, server → client)
    /// This packet tells the client where they should be and how they should look
    pub async fn send_player_position_and_look<N64: Into<f64>, N32: Into<f32>>(
        stream: &mut TcpStream,
        vec_3: Vec3<N64>,
        // x: N64,
        // y: N64,
        // z: N64,
        vec_2: Vec2<N32>,
        // yaw: N32,
        // pitch: N32,
        relative_arguments: u8,
        teleport_id: i32,
    ) -> Result<()> {
        let mut writer = PacketWriter::new();

        // Position
        writer.write_double(vec_3.x);
        writer.write_double(vec_3.y);
        writer.write_double(vec_3.z);

        // Rotation (yaw and pitch)
        writer.write_float(vec_2.yaw);
        writer.write_float(vec_2.pitch);

        // Relative arguments (bitfield)
        // Bit 0 (0x01): X is relative
        // Bit 1 (0x02): Y is relative
        // Bit 2 (0x04): Z is relative
        // Bit 3 (0x08): Yaw is relative
        // Bit 4 (0x10): Pitch is relative
        writer.write_byte(relative_arguments);

        // Teleport ID (used in Confirm Teleport packet)
        writer.write_varint(teleport_id);

        let packet_data = writer.finish();
        let packet_id = write_varint(0x28); // Player Position And Look packet ID
        let packet_length = (packet_id.len() + packet_data.len()) as i32;

        // Write packet: [length][id][data]
        let mut frame = Vec::new();
        frame.extend_from_slice(&write_varint(packet_length));
        frame.extend_from_slice(&packet_id);
        frame.extend_from_slice(&packet_data);

        #[cfg(feature = "dev-sdk")]
        let _ = &crate::LOGGER.log_server_packet(&frame);

        stream.write_all(&frame).await?;
        stream.flush().await?;

        Ok(())
    }

    /// Send Entity Status packet (0x01 in Play state)
    /// Used to send various entity events
    pub async fn send_entity_status(stream: &mut TcpStream, entity_id: i32, status: u8) -> Result<()> {
        let mut writer = PacketWriter::new();

        // Entity ID
        writer.write_int(entity_id);

        // Status code
        writer.write_byte(status);

        let packet_data = writer.finish();
        let packet_id = write_varint(0x01); // Entity Status packet ID
        let packet_length = (packet_id.len() + packet_data.len()) as i32;

        // Write packet: [length][id][data]
        let mut frame = Vec::new();
        frame.extend_from_slice(&write_varint(packet_length));
        frame.extend_from_slice(&packet_id);
        frame.extend_from_slice(&packet_data);

        #[cfg(feature = "dev-sdk")]
        let _ = &crate::LOGGER.log_server_packet(&frame);

        stream.write_all(&frame).await?;
        stream.flush().await?;

        Ok(())
    }

    /// Send Synchronize Player Position packet (0x31 in Play state)
    /// Alternative to Player Position And Look, used for synchronization
    pub async fn send_synchronize_player_position<N64: Into<f64>, N32: Into<f32>>(
        stream: &mut TcpStream,
        vec_3: Vec3<N64>,
        // x: f64,
        // y: f64,
        // z: f64,
        vec_2: Vec2<N32>,
        // yaw: f32,
        // pitch: f32,
        teleport_id: i32,
    ) -> Result<()> {
        let mut writer = PacketWriter::new();

        // Position
        writer.write_double(vec_3.x);
        writer.write_double(vec_3.y);
        writer.write_double(vec_3.z);

        // Rotation
        writer.write_float(vec_2.yaw);
        writer.write_float(vec_2.pitch);

        // Teleport ID
        writer.write_varint(teleport_id);

        let packet_data = writer.finish();
        let packet_id = write_varint(0x31); // Synchronize Player Position packet ID
        let packet_length = (packet_id.len() + packet_data.len()) as i32;

        // Write packet: [length][id][data]
        let mut frame = Vec::new();
        frame.extend_from_slice(&write_varint(packet_length));
        frame.extend_from_slice(&packet_id);
        frame.extend_from_slice(&packet_data);

        #[cfg(feature = "dev-sdk")]
        let _ = &crate::LOGGER.log_server_packet(&frame);

        stream.write_all(&frame).await?;
        stream.flush().await?;

        Ok(())
    }
}
