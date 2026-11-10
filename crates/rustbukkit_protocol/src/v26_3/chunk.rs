use std::collections::HashSet;

use rustbukkit_chunks::{BlockKind, ChunkSnapshot, FlatWorld};
use rustbukkit_encoding::Writer;
use thiserror::Error;

pub const FORGET_CHUNK_ID: i32 = 38;
pub const LEVEL_CHUNK_WITH_LIGHT_ID: i32 = 46;
pub const LIGHT_UPDATE_ID: i32 = 49;
pub const SET_CHUNK_CACHE_CENTER_ID: i32 = 96;

const SECTION_COUNT: usize = FlatWorld::HEIGHT as usize / 16;
const LIGHT_SECTION_COUNT: usize = SECTION_COUNT + 2;
const VALID_LIGHT_BITS: u32 = (1 << LIGHT_SECTION_COUNT) - 1;
const HEIGHTMAP_WORDS: usize = 256_usize.div_ceil(7);

static FULL_SKY_LIGHT: [[u8; 2048]; LIGHT_SECTION_COUNT] = [[0xff; 2048]; LIGHT_SECTION_COUNT];

/// Skylight covering every light section at full brightness.
///
/// The overworld currently has no per-section light propagation, so reporting
/// full sky light keeps loaded terrain visible instead of black while every
/// section is still described by a valid mask/array pair.
pub fn full_skylight() -> LightData<'static> {
    LightData {
        sky_mask:         VALID_LIGHT_BITS,
        block_mask:       0,
        empty_sky_mask:   0,
        empty_block_mask: VALID_LIGHT_BITS,
        sky_arrays:       &FULL_SKY_LIGHT,
        block_arrays:     &[],
    }
}

// Authoritative 26.3 default block-state IDs, extracted from the official
// server's generated `blocks.json` report. Mirrored in
// `registry_data/26.3/block_states.json` and asserted by
// `tests/protocol_ids_26_3.rs`.
const BLOCK_STATES: BlockStateIds = BlockStateIds {
    air:         0,
    stone:       1,
    water:       89,
    bedrock:     88,
    dirt:        10,
    grass_block: 9,
    sand:        121,
    cobblestone: 14,
};

/// Block-state IDs shared by chunk data and block updates.
pub const fn default_block_state_ids() -> BlockStateIds {
    BLOCK_STATES
}

/// Block-state registry ID used by both chunk data and block updates.
pub fn block_state_id(block: BlockKind) -> i32 {
    BLOCK_STATES.for_block(block)
}

#[derive(Debug, Clone, Copy)]
pub struct BlockStateIds {
    pub air:         i32,
    pub stone:       i32,
    pub water:       i32,
    pub bedrock:     i32,
    pub dirt:        i32,
    pub grass_block: i32,
    pub sand:        i32,
    pub cobblestone: i32,
}

impl BlockStateIds {
    fn validate(self) -> Result<(), ChunkError> {
        let ids = [
            self.air,
            self.stone,
            self.water,
            self.bedrock,
            self.dirt,
            self.grass_block,
            self.sand,
            self.cobblestone,
        ];
        let mut unique = HashSet::with_capacity(ids.len());
        for id in ids {
            if id < 0 {
                return Err(ChunkError::InvalidBlockStateId(id));
            }
            if !unique.insert(id) {
                return Err(ChunkError::DuplicateBlockStateId(id));
            }
        }
        Ok(())
    }

    pub(crate) fn for_block(self, block: BlockKind) -> i32 {
        match block {
            BlockKind::Air => self.air,
            BlockKind::Stone => self.stone,
            BlockKind::Water => self.water,
            BlockKind::Bedrock => self.bedrock,
            BlockKind::Dirt => self.dirt,
            BlockKind::GrassBlock => self.grass_block,
            BlockKind::Sand => self.sand,
            BlockKind::Cobblestone => self.cobblestone,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LightData<'a> {
    pub sky_mask:         u32,
    pub block_mask:       u32,
    pub empty_sky_mask:   u32,
    pub empty_block_mask: u32,
    pub sky_arrays:       &'a [[u8; 2048]],
    pub block_arrays:     &'a [[u8; 2048]],
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ChunkError {
    #[error("block-state registry ID must be nonnegative: {0}")]
    InvalidBlockStateId(i32),
    #[error("block-state registry ID is used by multiple block kinds: {0}")]
    DuplicateBlockStateId(i32),
    #[error("biome registry ID must be nonnegative: {0}")]
    InvalidBiomeId(i32),
    #[error("light masks and arrays are inconsistent for this dimension")]
    InvalidLightData,
}

impl LightData<'_> {
    fn validate(&self) -> Result<(), ChunkError> {
        if (self.sky_mask | self.block_mask | self.empty_sky_mask | self.empty_block_mask) & !VALID_LIGHT_BITS
            != 0
            || self.sky_mask & self.empty_sky_mask != 0
            || self.block_mask & self.empty_block_mask != 0
            || self.sky_mask.count_ones() as usize != self.sky_arrays.len()
            || self.block_mask.count_ones() as usize != self.block_arrays.len()
        {
            return Err(ChunkError::InvalidLightData);
        }
        Ok(())
    }

    fn write(&self, writer: &mut Writer) {
        for mask in [
            self.sky_mask,
            self.block_mask,
            self.empty_sky_mask,
            self.empty_block_mask,
        ] {
            if mask == 0 {
                writer.varint(0);
            } else {
                writer.varint(1);
                writer.i64(i64::from(mask));
            }
        }
        for arrays in [self.sky_arrays, self.block_arrays] {
            writer.varint(arrays.len() as i32);
            for array in arrays {
                writer.varint(array.len() as i32);
                writer.raw(array);
            }
        }
    }
}

pub fn encode_forget_chunk(x: i32, z: i32) -> Vec<u8> {
    let mut writer = Writer::new();
    let packed = u64::from(x as u32) | (u64::from(z as u32) << 32);
    writer.i64(packed as i64);
    writer.finish()
}

pub fn encode_set_chunk_cache_center(x: i32, z: i32) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.varint(x);
    writer.varint(z);
    writer.finish()
}

pub fn encode_light_update(x: i32, z: i32, light: &LightData<'_>) -> Result<Vec<u8>, ChunkError> {
    light.validate()?;
    let mut writer = Writer::new();
    writer.varint(x);
    writer.varint(z);
    light.write(&mut writer);
    Ok(writer.finish())
}

pub fn encode_chunk_with_light(
    x: i32,
    z: i32,
    chunk: &ChunkSnapshot,
    block_states: BlockStateIds,
    biome_id: i32,
    light: &LightData<'_>,
) -> Result<Vec<u8>, ChunkError> {
    block_states.validate()?;
    if biome_id < 0 {
        return Err(ChunkError::InvalidBiomeId(biome_id));
    }
    light.validate()?;

    let mut sections = Writer::new();
    let mut heights = [0u16; 256];
    for section_index in 0..SECTION_COUNT {
        let mut palette = Vec::with_capacity(8);
        let mut words = [0u64; 256];
        let mut non_air = 0i16;
        let mut fluids = 0i16;
        let base_y = FlatWorld::MIN_Y + section_index as i32 * 16;

        for local_y in 0..16 {
            let y = base_y + local_y;
            for local_z in 0..16u8 {
                for local_x in 0..16u8 {
                    let block = chunk.block_at(local_x, y, local_z);
                    if block != BlockKind::Air {
                        non_air += 1;
                        heights[usize::from(local_z) * 16 + usize::from(local_x)] =
                            (y + 1 - FlatWorld::MIN_Y) as u16;
                    }
                    if block == BlockKind::Water {
                        fluids += 1;
                    }
                    let id = block_states.for_block(block);
                    let palette_index = match palette.iter().position(|&entry| entry == id) {
                        Some(index) => index,
                        None => {
                            palette.push(id);
                            palette.len() - 1
                        }
                    };
                    let index = local_y as usize * 256 + usize::from(local_z) * 16 + usize::from(local_x);
                    words[index / 16] |= (palette_index as u64) << ((index % 16) * 4);
                }
            }
        }

        sections.i16(non_air);
        sections.i16(fluids);
        if palette.len() == 1 {
            sections.u8(0);
            sections.varint(palette[0]);
        } else {
            sections.u8(4);
            sections.varint(palette.len() as i32);
            for id in palette {
                sections.varint(id);
            }
            for word in words {
                sections.i64(word as i64);
            }
        }
        sections.u8(0);
        sections.varint(biome_id);
    }

    let mut heightmap = [0u64; HEIGHTMAP_WORDS];
    for (index, height) in heights.into_iter().enumerate() {
        heightmap[index / 7] |= u64::from(height) << ((index % 7) * 9);
    }

    let mut writer = Writer::new();
    writer.i32(x);
    writer.i32(z);
    writer.varint(3);
    for heightmap_type in [1, 4, 5] {
        writer.varint(heightmap_type);
        writer.varint(HEIGHTMAP_WORDS as i32);
        for word in heightmap {
            writer.i64(word as i64);
        }
    }
    let sections = sections.finish();
    writer.varint(sections.len() as i32);
    writer.raw(&sections);
    writer.varint(0);
    light.write(&mut writer);
    Ok(writer.finish())
}
