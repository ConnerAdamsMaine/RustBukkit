#![allow(dead_code)]
use std::ops::Neg;

use anyhow::Result;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::consts::{WORLD_MAX_CHUNKS, WORLD_REGION_SIZE};
use crate::terrain::{BlockType, Chunk, ChunkPos};

// const WORLD_REGION_SIZE: i32 = 32;
// const WORLD_MAX_CHUNKS: i32 = 10240;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegionPos {
    pub x: i32,
    pub z: i32,
}

impl From<ChunkPos> for RegionPos {
    fn from(chunk_pos: ChunkPos) -> Self {
        Self {
            x: chunk_pos.x >> 5,
            z: chunk_pos.z >> 5,
        }
    }
}

impl RegionPos {
    pub fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    pub fn from_chunk(chunk_x: i32, chunk_z: i32) -> Self {
        Self {
            x: chunk_x >> 5,
            z: chunk_z >> 5,
        }
    }

    pub fn min_chunk(&self) -> (i32, i32) {
        (self.x * WORLD_REGION_SIZE, self.z * WORLD_REGION_SIZE)
    }

    pub fn max_chunk(&self) -> (i32, i32) {
        ((self.x + 1) * WORLD_REGION_SIZE - 1, (self.z + 1) * WORLD_REGION_SIZE - 1)
    }

    pub fn chunk_offset(&self, chunk_x: i32, chunk_z: i32) -> Option<usize> {
        let (min_x, min_z) = self.min_chunk();
        let local_x = chunk_x - min_x;
        let local_z = chunk_z - min_z;

        // if local_x >= 0 && local_x < REGION_SIZE && local_z >= 0 && local_z < REGION_SIZE {
        if (0..WORLD_REGION_SIZE).contains(&local_x) && (0..WORLD_REGION_SIZE).contains(&local_z) {
            Some((local_z * WORLD_REGION_SIZE + local_x) as usize)
        } else {
            None
        }
    }

    pub fn filename(&self) -> String {
        let (min_x, min_z) = self.min_chunk();
        let (max_x, max_z) = self.max_chunk();
        format!("region_{}_{}_{}_{}.dat", min_x, min_z, max_x, max_z)
    }

    pub fn is_valid(&self) -> bool {
        // Allow negative coordinates - world is centered at origin
        let (min_x, min_z) = self.min_chunk();
        let (max_x, max_z) = self.max_chunk();

        let half_world = WORLD_MAX_CHUNKS / 2;
        let neg_bound: i32 = half_world.neg();
        // -(half_world as i32);
        let pos_bound: i32 = half_world;

        assert!(pos_bound.is_positive());
        assert!(neg_bound.is_negative());

        // The below are all semantically equivalent ways to check bounds
        // I'm going to leave these here for the time being duiring dev work,
        // if you want to you can pick one and delete them later.

        // Original
        // min_x >= neg_bound && min_z >= neg_bound && max_x < pos_bound && max_z < pos_bound

        // let valid = (neg_bound..pos_bound).contains(&min_x)
        //     && (neg_bound..pos_bound).contains(&min_z)
        //     && (neg_bound..pos_bound).contains(&max_x)
        //     && (neg_bound..pos_bound).contains(&max_z);

        // let min_corner = [min_x, min_z];
        // let max_corner = [max_x, max_z];
        // min_corner.iter().all(|&min_c| min_c >= neg_bound)
        //     && max_corner.iter().all(|&max_c| max_c < pos_bound)

        [min_x, min_z].par_iter().all(|&min| min >= neg_bound)
            && [max_x, max_z].par_iter().all(|&max| max < pos_bound)

        // let x_fits = min_x >= neg_bound && max_x < pos_bound;
        // let z_fits = min_z >= neg_bound && max_z < pos_bound;
        // x_fits && z_fits
    }
}

#[derive(Serialize, Deserialize)]
pub struct SerializedChunk {
    pub pos:    (i32, i32),
    pub blocks: Vec<u16>,
}

impl SerializedChunk {
    pub fn from_chunk(chunk: &Chunk) -> Self {
        let mut blocks = Vec::with_capacity(16 * 256 * 16);

        for y in 0..256 {
            for x in 0..16 {
                for z in 0..16 {
                    let block = chunk.get_block(x, y, z).map(|b| b as u16).unwrap_or(0);
                    blocks.push(block);
                }
            }
        }

        Self {
            pos: (chunk.pos.x, chunk.pos.z),
            blocks,
        }
    }

    pub fn to_chunk(&self) -> Result<Chunk> {
        let mut chunk = Chunk::new(ChunkPos::new(self.pos.0, self.pos.1));

        let mut idx = 0;
        for y in 0..256 {
            for x in 0..16 {
                for z in 0..16 {
                    if idx < self.blocks.len() {
                        if let Some(block_type) = BlockType::from_u16(self.blocks[idx]) {
                            chunk.set_block(x, y, z, block_type);
                        }
                        idx += 1;
                    }
                }
            }
        }

        Ok(chunk)
    }
}

pub struct Region {
    pos:      RegionPos,
    chunks:   Vec<Option<Chunk>>,
    modified: bool,
}

impl Region {
    pub fn new(pos: RegionPos) -> Self {
        Self {
            pos,
            chunks: vec![None; (WORLD_REGION_SIZE * WORLD_REGION_SIZE) as usize],
            modified: false,
        }
    }

    pub fn get(&self, chunk_x: i32, chunk_z: i32) -> Option<&Chunk> {
        self.pos
            .chunk_offset(chunk_x, chunk_z)
            .and_then(|idx| self.chunks[idx].as_ref())
    }

    pub fn insert(&mut self, chunk: Chunk) -> bool {
        if let Some(idx) = self.pos.chunk_offset(chunk.pos.x, chunk.pos.z) {
            self.chunks[idx] = Some(chunk);
            self.modified = true;
            true
        } else {
            false
        }
    }

    /// `std` library iterator,
    /// uses chunks.iter().filter_map(...)
    pub fn chunks_iter(&self) -> impl Iterator<Item = &Chunk> {
        // FilterMap<Iter<'_, Option<Chunk>>, impl Fn(&Option<Chunk>) -> Option<&Chunk>>
        self.chunks.iter().filter_map(|c| c.as_ref())
    }

    /// `rayon` parallel iterator,
    /// uses chunks.par_iter().filter_map(...)
    pub fn par_chunks_iter(&self) -> impl ParallelIterator<Item = &Chunk> {
        // FilterMap<Iter<'_, Option<Chunk>>, impl FnMut(&Option<Chunk>) -> Option<&Chunk>>
        self.chunks.par_iter().filter_map(|c| c.as_ref())
    }

    pub fn is_modified(&self) -> bool {
        self.modified
    }

    pub fn mark_clean(&mut self) {
        self.modified = false;
    }

    pub fn serialize(&self) -> Vec<u8> {
        let serialized: Vec<SerializedChunk> =
            self.par_chunks_iter().map(SerializedChunk::from_chunk).collect();
        bincode::serialize(&serialized).unwrap_or_default()
    }

    pub fn deserialize(data: &[u8]) -> Result<Self> {
        let serialized: Vec<SerializedChunk> = bincode::deserialize(data)?;
        let mut region = Self::new(RegionPos::new(0, 0)); // Will be set properly

        for ser_chunk in serialized {
            let chunk = ser_chunk.to_chunk()?;
            region.insert(chunk);
        }

        Ok(region)
    }
}
