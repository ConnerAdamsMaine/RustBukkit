#![allow(dead_code)]

use anyhow::Result;
use bytes::BytesMut;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::network::{ByteWritable, PacketWriter, write_varint};
use crate::terrain::{BlockType, Chunk};

/// Send a single chunk to the client using the Chunk Data packet
/// This is the primary packet for sending terrain data
pub async fn send_chunk_data_packet(socket: &mut TcpStream, chunk: &Chunk) -> Result<()> {
    let mut writer = PacketWriter::new();

    // Chunk X coordinate
    writer.write_int(chunk.pos.x);

    // Chunk Z coordinate
    writer.write_int(chunk.pos.z);

    // Heightmap data (NBT compound containing "MOTION_BLOCKING" and optionally "WORLD_SURFACE")
    let heightmap_nbt = create_heightmap_nbt();
    writer.write_bytes(&heightmap_nbt);

    // Data section
    // For 1.21.7, data is a single NBT compound containing chunk data
    // Simplified: send empty or minimal data for now
    let chunk_data_nbt = create_chunk_data_nbt(chunk);
    writer.write_bytes(&chunk_data_nbt);

    let packet_data = writer.finish();
    let packet_id = write_varint(0x20); // Chunk Data packet ID in Play state (0x20 or 0x27)
    let packet_length = (packet_id.len() + packet_data.len()) as i32;

    // Write packet: [length][id][data]
    let mut frame = vec![];
    frame.extend_from_slice(&write_varint(packet_length));
    frame.extend_from_slice(&packet_id);
    frame.extend_from_slice(&packet_data);

    #[cfg(feature = "dev-sdk")]
    let _ = &crate::LOGGER.log_server_packet(&frame);

    socket.write_all(&frame).await?;
    socket.flush().await?;

    tracing::debug!("[CHUNK] Sent chunk data packet for ({}, {})", chunk.pos.x, chunk.pos.z);
    Ok(())
}

/// Create a minimal heightmap NBT compound
/// Structure: TAG_Compound "" { TAG_LongArray "MOTION_BLOCKING": [...] }
fn create_heightmap_nbt() -> Vec<u8> {
    let mut bytes = vec![];

    // TAG_Compound
    bytes.push(0x0A);

    // Root name (empty)
    bytes.extend_from_slice(&(0i16).to_be_bytes());

    // TAG_LongArray for MOTION_BLOCKING
    bytes.push(0x0C); // TAG_LongArray

    // Name: "MOTION_BLOCKING"
    let name = b"MOTION_BLOCKING";
    bytes.extend_from_slice(&(name.len() as i16).to_be_bytes());
    bytes.extend_from_slice(name);

    // Array length (256 longs for 256 heightmap entries / 64 bits per long)
    bytes.extend_from_slice(&(36i32).to_be_bytes()); // 36 longs to cover 256 entries at 9 bits each

    // Array data (placeholder - all zeros)
    for _ in 0..36 {
        bytes.extend_from_slice(&(0i64).to_be_bytes());
    }

    // TAG_End
    bytes.push(0x00);

    bytes
}

/// Create minimal chunk data NBT
/// For now, return a minimal valid structure
fn create_chunk_data_nbt(_chunk: &Chunk) -> Vec<u8> {
    let mut bytes = vec![];

    // TAG_Compound (root)
    bytes.push(0x0A);

    // Root name (empty)
    bytes.extend_from_slice(&(0i16).to_be_bytes());

    // For 1.21.7, this would contain sections and other data
    // For now, return a minimal empty compound

    // TAG_End
    bytes.push(0x00);

    bytes
}

/// Serialize a chunk into Minecraft protocol format (legacy implementation)
/// This creates a basic chunk data packet that clients can render
pub fn serialize_chunk(chunk: &Chunk) -> BytesMut {
    let mut writer = PacketWriter::new();

    // Chunk X coordinate
    writer.write_int(chunk.pos.x);

    // Chunk Z coordinate
    writer.write_int(chunk.pos.z);

    // Heightmaps (simplified - send a flat heightmap)
    let heightmap_data = serialize_heightmap(chunk);
    writer.write_bytes(&heightmap_data);

    // Empty biome data
    writer.write_varint(0);

    // Data sections (empty for now - this is where block data goes)
    writer.write_varint(0); // 0 sections

    // Block entity count (empty)
    writer.write_varint(0);

    writer.finish()
}

/// Serialize a simple flat heightmap for the chunk
fn serialize_heightmap(_chunk: &Chunk) -> Vec<u8> {
    // Minecraft heightmap is 256 9-bit values packed into bits
    // For now, return a minimal heightmap
    vec![0; 36] // 36 bytes can hold 256 9-bit values
}

/// Check if a chunk section (16x16x16 blocks) contains any non-air blocks
fn has_section_data(chunk: &Chunk, section_y: usize) -> bool {
    let base_y = section_y * 16;
    for x in 0..16 {
        for y in base_y..base_y + 16 {
            for z in 0..16 {
                let Some(block) = chunk.get_block(x, y, z) else {
                    continue;
                };
                if block != BlockType::Air {
                    return true;
                }
            }
        }
    }
    false
}

/// Build a palette of block IDs present in this section
fn build_palette(chunk: &Chunk, section_y: usize) -> Vec<i32> {
    let base_y = section_y * 16;
    let mut palette = vec![0i32]; // Air is always at index 0
    let mut seen = std::collections::HashSet::new();
    seen.insert(0i32);

    for x in 0..16 {
        for y in base_y..base_y + 16 {
            for z in 0..16 {
                if let Some(block) = chunk.get_block(x, y, z) {
                    let block_id = block_type_to_id(block);
                    if !seen.contains(&block_id) && block_id != 0 {
                        palette.push(block_id);
                        seen.insert(block_id);
                    }
                }
            }
        }
    }

    palette
}

/// Convert block type to Minecraft block state ID
fn block_type_to_id(block: BlockType) -> i32 {
    // This maps our BlockType enum to Minecraft block state IDs
    match block {
        BlockType::Air => 0,
        BlockType::Stone => 1,
        BlockType::Grass => 3,
        BlockType::Dirt => 3,
        BlockType::Cobblestone => 4,
        BlockType::OakLog => 17,
        BlockType::OakLeaves => 18,
        BlockType::OakPlanks => 5,
        BlockType::Water => 9,
        BlockType::Lava => 10,
        BlockType::Sand => 12,
        BlockType::Gravel => 13,
    }
}
