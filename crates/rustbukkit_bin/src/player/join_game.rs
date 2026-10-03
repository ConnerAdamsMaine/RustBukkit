use anyhow::Result;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tracing::warn;
use uuid::Uuid;

// use crate::packet_logger::PacketLogger;
use crate::{
    network::{ByteWritable, PacketWriter, write_varint},
    player::Vec3,
};

pub struct JoinGameHandler;

#[allow(dead_code)]
impl JoinGameHandler {
    /// Send Spawn Position packet (0x4D in Play state)
    /// Sets the spawn point and angle
    pub async fn send_spawn_position<N32: Into<i32>>(
        stream: &mut TcpStream,
        vec_3: Vec3<N32>,
        // x: i32,
        // y: i32,
        // z: i32,
        angle: f32,
    ) -> Result<()> {
        let mut writer = PacketWriter::new();

        // Position (combined location)
        // For 1.21.7, position is sent as three separate ints
        writer.write_int(vec_3.x);
        writer.write_int(vec_3.y);
        writer.write_int(vec_3.z);

        // Angle (rotation in degrees, 0-360)
        writer.write_float(angle);

        let packet_data = writer.finish();
        let packet_id = write_varint(0x4D); // Spawn Position packet ID in Play state
        let packet_length = (packet_id.len() + packet_data.len()) as i32;

        // Write packet: [length][id][data]
        let mut frame = Vec::new();
        frame.extend_from_slice(&write_varint(packet_length));
        frame.extend_from_slice(&packet_id);
        frame.extend_from_slice(&packet_data);

        #[cfg(feature = "dev-sdk")]
        let _ = crate::LOGGER.log_server_packet(&frame);

        stream.write_all(&frame).await?;
        stream.flush().await?;

        Ok(())
    }

    pub async fn send_configuration_finish(stream: &mut TcpStream) -> Result<()> {
        // Configuration Finish packet (0x02) - transitions from Configuration to Play state
        // This packet has no data, just the ID
        let packet_id = write_varint(0x02);
        let packet_length = packet_id.len() as i32;

        let mut frame = Vec::new();
        frame.extend_from_slice(&write_varint(packet_length));
        frame.extend_from_slice(&packet_id);

        #[cfg(feature = "dev-sdk")]
        let _ = crate::LOGGER.log_server_packet(&frame);

        stream.write_all(&frame).await?;
        stream.flush().await?;

        Ok(())
    }

    pub async fn send_disconnect(stream: &mut TcpStream, reason: &str) -> Result<()> {
        let mut writer = PacketWriter::new();

        // Write JSON chat message
        let json_message = format!(r#"{{"text":"{}"}}"#, reason.replace('"', "\\\""));
        writer.write_string(&json_message);

        let packet_data = writer.finish();
        let packet_id = write_varint(0x19); // Disconnect packet ID in Play state
        let packet_length = (packet_id.len() + packet_data.len()) as i32;

        // Write packet: [length][id][data]
        let mut frame = Vec::new();
        frame.extend_from_slice(&write_varint(packet_length));
        frame.extend_from_slice(&packet_id);
        frame.extend_from_slice(&packet_data);

        #[cfg(feature = "dev-sdk")]
        let _ = &crate::LOGGER.log_server_packet(&frame);

        if let Err(e) = stream.write_all(&frame).await {
            warn!("Failed to send disconnect: {}", e);
        } else {
            let _ = stream.flush().await;
        }

        Ok(())
    }

    pub async fn send_join_game(
        stream: &mut TcpStream,
        entity_id: i32,
        _username: &str,
        // packet_logger: &PacketLogger,
    ) -> Result<()> {
        let mut writer = PacketWriter::new();

        // Entity ID
        writer.write_int(entity_id);

        // Hardcore Flag
        writer.write_bool(false);

        // Gamemode (0 = Survival)
        writer.write_byte(0);

        // Previous Gamemode (0xFF = none)
        writer.write_byte(0xFF);

        // World Count
        writer.write_varint(1);

        // World Names - single world: "minecraft:overworld"
        writer.write_string("minecraft:overworld");

        // Dimension Codec (simplified NBT) - empty for now
        writer.write_bytes(Self::get_dimension_codec_nbt());

        // Dimension (simplified NBT) - overworld
        writer.write_bytes(Self::get_dimension_nbt());

        // World Name
        writer.write_string("minecraft:overworld");

        // Hashed Seed
        writer.write_long(12345);

        // Max Players
        writer.write_varint(20);

        // View Distance
        writer.write_varint(10);

        // Reduced Debug Info
        writer.write_bool(false);

        // Enable Respawn Screen
        writer.write_bool(true);

        // Is Debug
        writer.write_bool(false);

        // Is Flat
        writer.write_bool(false);

        let packet_data = writer.finish();
        let packet_id = write_varint(0x28);
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

    pub async fn send_player_info_add(stream: &mut TcpStream, uuid: Uuid, username: &str) -> Result<()> {
        let mut writer = PacketWriter::new();

        // Action: 0 = Add Player
        writer.write_varint(0);

        // Number of entries
        writer.write_varint(1);

        // Entry UUID
        writer.write_uuid(uuid);

        // Player name
        writer.write_string(username);

        // Properties count (0 for now)
        writer.write_varint(0);

        // Gamemode (0 = Survival)
        writer.write_varint(0);

        // Ping (milliseconds)
        writer.write_varint(0);

        // Has display name
        writer.write_bool(false);

        let packet_data = writer.finish();
        let packet_id = write_varint(0x53);
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

    fn get_dimension_codec_nbt() -> Vec<u8> {
        // Minimal NBT for dimension codec
        // TAG_Compound "": {
        //   TAG_List "minecraft:dimension_type": [
        //     TAG_Compound {
        //       TAG_String "name": "minecraft:overworld"
        //       TAG_Int "id": 0
        //       TAG_Compound "element": { ... }
        //     }
        //   ]
        //   TAG_List "minecraft:worldgen/biome": [...]
        // }
        // For simplicity, using a minimal structure

        vec![
            0x0A, // TAG_Compound
            0x00, 0x00, // Name length (0)
            // TAG_List for dimension_type
            0x09, // TAG_List
            0x00, 0x1A, // Name: "minecraft:dimension_type"
            0x0A, // TAG_Compound (list type)
            0x00, 0x00, 0x00, 0x00, // Count: 0
            // End
            0x00, // TAG_End
        ]
    }

    fn get_dimension_nbt() -> Vec<u8> {
        // Minimal NBT for overworld dimension
        // TAG_Compound "": {
        //   TAG_Byte "piglin_safe": 0
        //   TAG_Byte "natural": 1
        //   ...
        // }

        vec![
            0x0A, // TAG_Compound
            0x00, 0x00, // Name length (0)
            // Minimal required fields
            0x01, 0x00, 0x0B, 0x70, 0x69, 0x67, 0x6C, 0x69, 0x6E, 0x5F, 0x73, 0x61, 0x66, 0x65,
            0x00, // piglin_safe: 0
            0x01, 0x00, 0x07, 0x6E, 0x61, 0x74, 0x75, 0x72, 0x61, 0x6C, 0x01, // natural: 1
            0x00, // TAG_End
        ]
    }
}
