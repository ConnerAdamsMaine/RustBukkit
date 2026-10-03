#![allow(dead_code)]
use serde::{Deserialize, Serialize};

// const CHUNK_SIZE: usize = 16;
// const CHUNK_HEIGHT: usize = 256;
use crate::consts::{TERRAIN_CHUNK_HEIGHT, TERRAIN_CHUNK_SIZE};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ChunkPos {
    pub x: i32,
    pub z: i32,
}

impl std::fmt::Display for ChunkPos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}:{})", self.x, self.z)
    }
}

impl ChunkPos {
    pub fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    pub fn from_block_pos(x: i32, z: i32) -> Self {
        Self { x: x >> 4, z: z >> 4 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u16)]
pub enum BlockType {
    Air = 0,
    Stone = 1,
    Grass = 2,
    Dirt = 3,
    Cobblestone = 4,
    OakLog = 5,
    OakLeaves = 6,
    Water = 9,
    Lava = 10,
    Sand = 12,
    Gravel = 13,
    OakPlanks = 7,
}

impl BlockType {
    pub fn from_u16(value: u16) -> Option<Self> {
        match value {
            0 => Some(BlockType::Air),
            1 => Some(BlockType::Stone),
            2 => Some(BlockType::Grass),
            3 => Some(BlockType::Dirt),
            4 => Some(BlockType::Cobblestone),
            5 => Some(BlockType::OakLog),
            6 => Some(BlockType::OakLeaves),
            7 => Some(BlockType::OakPlanks),
            9 => Some(BlockType::Water),
            10 => Some(BlockType::Lava),
            12 => Some(BlockType::Sand),
            13 => Some(BlockType::Gravel),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub pos:      ChunkPos,
    blocks:       Vec<Vec<Vec<BlockType>>>, // [y][x][z]
    pub modified: bool,
}

impl Chunk {
    pub fn new(pos: ChunkPos) -> Self {
        Self {
            pos,
            blocks: vec![
                vec![vec![BlockType::Air; TERRAIN_CHUNK_SIZE]; TERRAIN_CHUNK_SIZE];
                TERRAIN_CHUNK_HEIGHT
            ],
            modified: true,
        }
    }

    pub fn get_block(&self, x: usize, y: usize, z: usize) -> Option<BlockType> {
        if x < TERRAIN_CHUNK_SIZE && y < TERRAIN_CHUNK_HEIGHT && z < TERRAIN_CHUNK_SIZE {
            Some(self.blocks[y][x][z])
        } else {
            None
        }
    }

    pub fn set_block(&mut self, x: usize, y: usize, z: usize, block: BlockType) -> bool {
        if x < TERRAIN_CHUNK_SIZE && y < TERRAIN_CHUNK_HEIGHT && z < TERRAIN_CHUNK_SIZE {
            self.blocks[y][x][z] = block;
            self.modified = true;
            true
        } else {
            false
        }
    }

    pub fn is_modified(&self) -> bool {
        self.modified
    }

    pub fn mark_clean(&mut self) {
        self.modified = false;
    }
}
