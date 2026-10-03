#![allow(dead_code)]

use bytes::BytesMut;

use crate::network::{ByteWritable, PacketWriter};
use crate::terrain::{BlockType, Chunk};

/// Serialize a chunk into Minecraft protocol format (chunk data packet)
/// This creates a basic chunk data packet that clients can render
pub fn serialize_chunk(chunk: &Chunk) -> BytesMut {
    let mut writer = PacketWriter::new();

    // Packet ID for Chunk Data packet (0x20 in Play state)
    writer.write_varint(0x20);

    // Chunk X coordinate
    writer.write_int(chunk.pos.x);

    // Chunk Z coordinate
    writer.write_int(chunk.pos.z);

    // Full chunk flag (true = full chunk with all sections)
    writer.write_bool(true);

    // Primary Bit Mask - which sections are included (16 sections, bottom to top)
    // For now, send all sections that have data
    let mut bitmask = 0u16;
    for y in 0..16 {
        if has_section_data(chunk, y) {
            bitmask |= 1 << y;
        }
    }
    writer.write_varint(bitmask as i32);

    // Heightmaps (simplified - send a flat heightmap)
    let heightmap_data = serialize_heightmap(chunk);
    writer.write_varint(heightmap_data.len() as i32);
    writer.write_bytes(&heightmap_data);

    // Biome data (empty for now)
    writer.write_varint(0); // 0 biomes

    // Data section count (number of chunk sections with data)
    let section_count = (0..16).filter(|&y| has_section_data(chunk, y)).count();
    writer.write_varint(section_count as i32);

    // Serialize each section that has data
    for section_y in 0..16 {
        if has_section_data(chunk, section_y) {
            serialize_section(&mut writer, chunk, section_y);
        }
    }

    writer.finish()
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

/// Serialize a 16x16x16 section of blocks using Minecraft's block state palette format
fn serialize_section(writer: &mut PacketWriter, chunk: &Chunk, section_y: usize) {
    let base_y = section_y * 16;

    // Block count (number of non-air blocks) - simplified
    let mut block_count = 0i16;
    for x in 0..16 {
        for y in base_y..base_y + 16 {
            for z in 0..16 {
                let Some(block) = chunk.get_block(x, y, z) else {
                    continue;
                };
                if block != BlockType::Air {
                    block_count += 1;
                }
            }
        }
    }
    writer.write_short(block_count);

    // Palette (block state mapping)
    // Minecraft uses a palette system where block IDs are mapped to indices
    // For simplicity, we use a direct mapping
    let palette = build_palette(chunk, section_y);
    writer.write_varint(palette.len() as i32);
    for block_id in &palette {
        writer.write_varint(*block_id);
    }

    // Data array (which palette index for each block)
    let data = encode_block_data(chunk, section_y, &palette);
    writer.write_varint((data.len() / 8) as i32); // Size in longs
    writer.write_bytes(&data);
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

/// Encode block data as a byte array with palette indices
fn encode_block_data(chunk: &Chunk, section_y: usize, palette: &[i32]) -> Vec<u8> {
    let base_y = section_y * 16;
    let mut blocks = Vec::new();

    // Collect all blocks in the section
    for y in base_y..base_y + 16 {
        for z in 0..16 {
            for x in 0..16 {
                let block = chunk.get_block(x, y, z).unwrap_or(BlockType::Air);
                let block_id = block_type_to_id(block);

                // Find index in palette
                let palette_idx = palette.iter().position(|&id| id == block_id).unwrap_or(0);
                blocks.push(palette_idx as u8);
            }
        }
    }

    // Pack blocks into 64-bit longs (Minecraft uses variable bit width based on palette size)
    // For simplicity, use 1 byte per block (8 bits per block supports up to 256 block types)
    blocks
}

/// Convert block type to Minecraft block state ID
fn block_type_to_id(block: BlockType) -> i32 {
    // This maps our BlockType enum to Minecraft block state IDs
    // Format: blockid << 4 | metadata (for 1.12.x compatibility)
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

/// Serialize a simple flat heightmap for the chunk
fn serialize_heightmap(_chunk: &Chunk) -> Vec<u8> {
    // Minecraft heightmap is 256 9-bit values packed into bits
    // For now, return a minimal heightmap
    vec![0; 36] // 36 bytes can hold 256 9-bit values (256 * 9 / 8 = 288 bits = 36 bytes)
}
