#![allow(dead_code)]

use std::io::Cursor;
use std::sync::Arc;

use anyhow::Result;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use uuid::Uuid;

use crate::chunk::ChunkStorage;
use crate::core::{ChunkGenThreadPool, HandlerData};
use crate::error_tracker::{ErrorKey, ErrorTracker};
use crate::network::{LoginHandler, read_varint};
use crate::player::configuration::ConfigurationHandler;
use crate::player::join_game::JoinGameHandler;
use crate::player::{CrossAssign, Vec2, Vec3, movement_handler};
use crate::terrain::ChunkPos;

pub struct PlayerData<N64: Into<f64> = f64> {
    pub uuid:         Uuid,
    pub username:     String,
    pub socket:       TcpStream,
    pub state:        PlayerState,
    // pub x:            f64,
    // pub y:            f64,
    // pub z:            f64,
    pub cooridinates: Vec3<N64>,
    pub last_chunk_x: i32,
    pub last_chunk_z: i32,
    loaded_chunks:    std::collections::HashSet<ChunkPos>,
}

impl CrossAssign for PlayerData<f64> {
    fn cross_assign(&mut self, rhs: Self) {
        self.cooridinates.cross_assign(rhs.cooridinates);
    }
}

impl CrossAssign for PlayerData<f32> {
    fn cross_assign(&mut self, rhs: Self) {
        self.cooridinates.cross_assign(rhs.cooridinates);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerState {
    Handshake,
    Login,
    Play,
    Idle,
}

impl PlayerData {
    pub async fn new(socket: TcpStream) -> Result<Self> {
        Ok(Self {
            uuid: Uuid::new_v4(),
            username: String::new(),
            socket,
            state: PlayerState::Handshake,
            cooridinates: Vec3::from((0.0, 64.0, 0.0)),
            last_chunk_x: 0,
            last_chunk_z: 0,
            loaded_chunks: std::collections::HashSet::new(),
        })
    }

    pub async fn handle(mut self, hd: HandlerData) -> Result<()> {
        tracing::debug!("[PLAYER] Player handler starting");

        // Wait for world initialization to complete (in blocking task to not block async runtime)
        tracing::debug!("[PLAYER] Waiting for world initialization...");
        let chunk_gen_pool_clone = Arc::clone(&hd.chunk_gen_pool);
        tokio::task::spawn_blocking(move || {
            chunk_gen_pool_clone.wait_for_init_complete();
            tracing::info!("[PLAYER] World initialization complete, accepting players");
        })
        .await?;

        // Handle login flow
        tracing::debug!("[PLAYER] Creating LoginHandler");
        let mut login_handler = LoginHandler::from(self.socket); // new(self.socket);

        tracing::debug!("[PLAYER] Starting login flow");
        let player_login = match login_handler.handle_login().await {
            Ok(login) => {
                tracing::debug!("[PLAYER] Login successful");
                login
            }
            Err(e) => {
                tracing::error!("[LOGIN] Authentication failed: {}", e);
                let key = ErrorKey::new("LOGIN", format!("auth_failed: {}", e));
                hd.error_tracker.record_error(key);
                return Err(e);
            }
        };

        tracing::debug!("[PLAYER] Extracting login info");
        self.uuid = player_login.uuid;
        self.username = player_login.username.clone();
        self.socket = login_handler.get_stream();
        self.state = PlayerState::Login;
        tracing::debug!("[PLAYER] Player state set to Login (awaiting configuration)");

        tracing::info!("[PLAYER] '{}' ({}) joined at {}", self.username, self.uuid, self.cooridinates);

        // Handle Configuration phase
        tracing::debug!("[PLAYER] Starting configuration phase");
        if let Err(e) = ConfigurationHandler::handle_configuration(&mut self.socket).await {
            tracing::error!("[PLAYER] Configuration phase failed for {}: {}", self.username, e);
            let key = ErrorKey::new("CONFIG", format!("config_failed: {}", e));
            hd.error_tracker.record_error(key);
            return Err(e);
        }
        tracing::debug!("[PLAYER] Configuration phase complete");

        // Transition to Play state
        self.state = PlayerState::Play;
        tracing::debug!("[PLAYER] Player state set to Play");

        // Send join game packet
        tracing::debug!("[PLAYER] Sending Join Game packet");
        if let Err(e) = JoinGameHandler::send_join_game(&mut self.socket, 1, &self.username).await {
            tracing::error!("[PLAYER] Failed to send join game packet to {}: {}", self.username, e);
            let key = ErrorKey::new("JOIN_GAME", "send_failed");
            hd.error_tracker.record_error(key);
            return Err(e);
        }
        tracing::debug!("[PLAYER] Join Game sent");

        // Send player info add packet
        tracing::debug!("[PLAYER] Sending Player Info Add packet");
        if let Err(e) =
            JoinGameHandler::send_player_info_add(&mut self.socket, self.uuid, &self.username).await
        {
            tracing::error!("[PLAYER] Failed to send player info to {}: {}", self.username, e);
            let key = ErrorKey::new("PLAYER_INFO", "send_failed");
            hd.error_tracker.record_error(key);
            return Err(e);
        }
        tracing::debug!("[PLAYER] Player Info Add sent");

        // Send spawn position packet
        tracing::debug!("[PLAYER] Sending Spawn Position packet");
        let spawn = Vec3::from((0, 64, 0));
        if let Err(e) = JoinGameHandler::send_spawn_position(&mut self.socket, spawn, 0.0).await {
            tracing::error!("[PLAYER] Failed to send spawn position: {}", e);
            let key = ErrorKey::new("SPAWN_POS", "send_failed");
            hd.error_tracker.record_error(key);
            return Err(e);
        }
        tracing::debug!("[PLAYER] Spawn Position sent");

        // Send synchronize player position to initialize client position
        tracing::debug!("[PLAYER] Sending initial player position sync");
        if let Err(e) = crate::player::PlayStateHandler::send_synchronize_player_position(
            &mut self.socket,
            self.cooridinates,
            Vec2::from((0.0, 0.0)),
            0, // teleport_id
        )
        .await
        {
            tracing::error!("[PLAYER] Failed to send player position sync: {}", e);
            let key = ErrorKey::new("POSITION_SYNC", "send_failed");
            hd.error_tracker.record_error(key);
            return Err(e);
        }
        tracing::debug!("[PLAYER] Player position sync sent");

        // Load initial chunks around player and send to client
        {
            let socket = &mut self.socket;
            if let Err(e) = Self::send_chunks_around_static(
                socket,
                &mut self.cooridinates,
                // self.x,
                // self.y,
                // self.z,
                &hd.chunk_storage,
                &mut self.loaded_chunks,
            )
            .await
            {
                tracing::error!("[CHUNK] Failed to load initial chunks for {}: {}", self.username, e);
                let key = ErrorKey::new("CHUNK", "load_failed");
                hd.error_tracker.record_error(key);
                return Err(e);
            }
        }

        tracing::info!("[PLAYER] {} ready to play at {}", self.username, self.cooridinates);
        tracing::debug!("[PLAYER] Starting main game loop");

        // Main game loop for this player
        loop {
            // Try to read incoming packets from client
            {
                let socket = &mut self.socket;
                // let logger = &self.packet_logger;
                match Self::handle_incoming_packets_static(
                    //
                    socket,
                    &mut self.cooridinates,
                    // &mut self.x,
                    // &mut self.y,
                    // &mut self.z,
                )
                .await
                {
                    Ok(_) => {}
                    Err(e) => {
                        tracing::error!("[PLAYER] {} packet read error: {}", self.username, e);
                        return Err(e);
                    }
                }
            }

            // Update loaded chunks based on player position
            if self.check_chunk_changed(&hd.chunk_storage).await? {
                // Player moved to a different chunk - send new chunks
                let socket = &mut self.socket;
                if let Err(e) = Self::send_chunks_around_static(
                    socket,
                    &mut self.cooridinates,
                    &hd.chunk_storage,
                    &mut self.loaded_chunks,
                )
                .await
                {
                    tracing::warn!("[PLAYER] Failed to send chunks to {}: {}", self.username, e);
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
    }

    async fn check_chunk_changed(&mut self, _chunk_storage: &ChunkStorage) -> Result<bool> {
        // Calculate current chunk position
        let current_chunk_x = (self.cooridinates.x / 16.0) as i32;
        let current_chunk_z = (self.cooridinates.z / 16.0) as i32;

        // Check if player moved to a different chunk
        if current_chunk_x != self.last_chunk_x || current_chunk_z != self.last_chunk_z {
            self.last_chunk_x = current_chunk_x;
            self.last_chunk_z = current_chunk_z;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn send_chunks_around_static<N64>(
        socket: &mut TcpStream,
        vec_3: &mut Vec3<N64>,
        chunk_storage: &ChunkStorage,
        loaded_chunks: &mut std::collections::HashSet<ChunkPos>,
    ) -> Result<()>
    where
        N64: Into<f64>,
        N64: Copy,
    {
        let chunk_x = (vec_3.x.into() / 16.0) as i32;
        let chunk_z = (vec_3.z.into() / 16.0) as i32;

        // Load a 5x5 chunk radius around player
        for cx in (chunk_x - 2)..=(chunk_x + 2) {
            for cz in (chunk_z - 2)..=(chunk_z + 2) {
                let pos = ChunkPos::new(cx, cz);

                if !loaded_chunks.contains(&pos) {
                    match chunk_storage.get_chunk(pos) {
                        Ok(chunk) => {
                            // Send chunk to client
                            if let Err(e) = &crate::chunk::send_chunk(socket, &chunk).await {
                                tracing::warn!("[CHUNK] Failed to send chunk {}: {}", pos, e);
                            } else {
                                loaded_chunks.insert(pos);
                                tracing::debug!("[CHUNK] Sent chunk {}", pos);
                            }
                        }
                        Err(e) => {
                            tracing::warn!("[CHUNK] Failed to load chunk {}: {}", pos, e);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn handle_incoming_packets_static(socket: &mut TcpStream, vec_3: &mut Vec3<f64>) -> Result<()> {
        // Read packet length
        let mut length_bytes = [0u8; 5];
        let n = socket.read(&mut length_bytes).await?;

        if n == 0 {
            // Client disconnected
            tracing::warn!("[PACKET] Client disconnected (read 0 bytes)");
            return Err(anyhow::anyhow!("Client disconnected"));
        }

        tracing::trace!("[PACKET] Read {} bytes for packet header", n);

        // Parse varint length
        let mut cursor = Cursor::new(&length_bytes[..n]);
        let packet_length = match read_varint(&mut cursor) {
            Ok(len) => {
                tracing::trace!("[PACKET] Packet length: {}", len);
                len as usize
            }
            Err(e) => {
                tracing::trace!("[PACKET] Could not parse varint: {}, trying again later", e);
                return Ok(()); // Incomplete packet, try again later
            }
        };

        // Read packet data
        let mut packet_data = vec![0u8; packet_length];
        match socket.read_exact(&mut packet_data).await {
            Ok(_) => {
                tracing::trace!("[PACKET] Read packet data ({} bytes)", packet_length);

                // Log the full packet (length + data)
                let mut full_packet = length_bytes[..n].to_vec();
                full_packet.extend_from_slice(&packet_data);
                #[cfg(feature = "dev-sdk")]
                let _ = &crate::LOGGER.log_client_packet(&full_packet);

                // Parse packet ID
                let mut cursor = Cursor::new(&packet_data[..]);
                if let Ok(packet_id) = read_varint(&mut cursor) {
                    let pos = cursor.position() as usize;
                    let payload = &packet_data[pos..];

                    tracing::trace!(
                        "[PACKET] Packet ID: 0x{:02x}, payload: {} bytes",
                        packet_id,
                        payload.len()
                    );

                    // Handle movement packets
                    if let Ok(Some(movement)) = movement_handler::parse_movement_packet(packet_id, payload) {
                        match movement {
                            movement_handler::MovementPacket::Position(pos) => {
                                let pos: Vec3<f64> =
                                    Vec3::from((pos.coordinates.x, pos.coordinates.y, pos.coordinates.z));

                                let mut v3: Vec3<f64> = Into::into(*vec_3);
                                CrossAssign::cross_assign(&mut v3, pos);

                                tracing::debug!("[PLAYER] moved to {}", pos);
                            }
                            movement_handler::MovementPacket::PositionAndLook(pos) => {
                                let pos_and_look =
                                    Vec3::from((pos.coordinates.x, pos.coordinates.y, pos.coordinates.z));

                                let mut v3: Vec3<f64> = Into::into(*vec_3);
                                CrossAssign::cross_assign(&mut v3, pos_and_look);

                                // where x, y, z are now vec_3.x, vec_3.y, vec_3.z
                                // *x = pos.x;
                                // *y = pos.y;
                                // *z = pos.z;
                                tracing::debug!("[PLAYER] moved to {}", pos_and_look);
                            }
                            movement_handler::MovementPacket::Look(_) => {
                                // Handle rotation only - no position update
                            }
                        }
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No data available, try again later
            }
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                // Client disconnected gracefully
                tracing::debug!("[PACKET] Client disconnected (unexpected EOF)");
                return Err(anyhow::anyhow!("Client disconnected"));
            }
            Err(e) => return Err(e.into()),
        }

        Ok(())
    }
}
