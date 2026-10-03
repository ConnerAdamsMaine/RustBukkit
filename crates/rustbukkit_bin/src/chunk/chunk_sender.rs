#![allow(dead_code)]

use anyhow::Result;
use tokio::net::TcpStream;
use tracing::debug;

use crate::chunk::{ChunkStorage, send_chunk_data_packet};
use crate::terrain::{Chunk, ChunkPos};

/// Send a single chunk to a player using the Chunk Data packet
pub async fn send_chunk(socket: &mut TcpStream, chunk: &Chunk) -> Result<()> {
    send_chunk_data_packet(socket, chunk).await?;
    debug!("[CHUNK] Sent chunk {} to player", chunk.pos);
    Ok(())
}

/// Send multiple chunks to a player
pub async fn send_chunks(socket: &mut TcpStream, chunks: &[Chunk]) -> Result<()> {
    for chunk in chunks {
        send_chunk(socket, chunk).await?;
    }
    Ok(())
}

/// Send chunks in a spiral pattern around player position
pub async fn send_chunks_around_player(
    socket: &mut TcpStream,
    chunk_storage: &ChunkStorage,
    chunk_x: i32,
    chunk_z: i32,
    radius: i32,
) -> Result<()> {
    // Spiral outward from player position
    for distance in 0..=radius {
        for dx in -distance..=distance {
            for dz in -distance..=distance {
                // Only process the current ring
                if dx.abs() != distance && dz.abs() != distance {
                    continue;
                }

                let pos = ChunkPos::new(chunk_x + dx, chunk_z + dz);
                match chunk_storage.get_chunk(pos) {
                    Ok(chunk) => {
                        send_chunk(socket, &chunk).await?;
                    }
                    Err(e) => {
                        debug!("[CHUNK] Failed to load chunk {}: {}", pos, e);
                    }
                }
            }
        }
    }

    Ok(())
}
