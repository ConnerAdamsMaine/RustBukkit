use anyhow::{Result, anyhow};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{info, warn};
use uuid::Uuid;

use crate::network::ByteWritable;
use crate::network::protocol::{PacketReader, PacketWriter, read_varint, write_varint};

#[derive(Debug, Clone)]
pub struct PlayerLogin {
    pub username: String,
    pub uuid:     Uuid,
}

pub struct LoginHandler {
    stream:           TcpStream,
    protocol_version: i32,
}

use crate::consts::NETWORK_VALID_PROTOCOL_VERSION;

impl From<TcpStream> for LoginHandler {
    fn from(stream: TcpStream) -> Self {
        Self {
            stream,
            protocol_version: 0,
        }
    }
}

impl LoginHandler {
    // pub fn new(stream: TcpStream) -> Self {
    //     Self {
    //         stream,
    //         protocol_version: 0,
    //     }
    // }

    pub async fn handle_login(&mut self) -> Result<PlayerLogin> {
        tracing::debug!("[LOGIN] Starting login flow");

        // Read Handshake packet
        tracing::debug!("[LOGIN] Waiting for Handshake packet...");
        if let Err(e) = self.read_handshake().await {
            warn!("[LOGIN] Handshake failed: {}", e);
            self.send_disconnect("Invalid handshake").await.ok();
            return Err(e);
        }
        tracing::debug!("[LOGIN] Handshake received, protocol version: {}", self.protocol_version);

        // Validate protocol version
        if self.protocol_version != NETWORK_VALID_PROTOCOL_VERSION {
            warn!(
                "[LOGIN] Invalid protocol version: {} (expected {})",
                self.protocol_version, NETWORK_VALID_PROTOCOL_VERSION
            );
            self.send_disconnect("Outdated server! Please use 1.21.7")
                .await
                .ok();
            return Err(anyhow!(
                "Protocol version mismatch: {} vs {}",
                self.protocol_version,
                NETWORK_VALID_PROTOCOL_VERSION
            ));
        }
        tracing::debug!("[LOGIN] Protocol version validated");

        // Read Login Start packet
        tracing::debug!("[LOGIN] Waiting for Login Start packet...");
        let username = match self.read_login_start().await {
            Ok(name) => {
                tracing::debug!("[LOGIN] Login Start received, username: {}", name);
                name
            }
            Err(e) => {
                warn!("[LOGIN] Login start failed: {}", e);
                self.send_disconnect("Invalid username").await.ok();
                return Err(e);
            }
        };

        // Validate username
        if !Self::is_valid_username(&username) {
            warn!("[LOGIN] Invalid username: {}", username);
            self.send_disconnect("Invalid username").await.ok();
            return Err(anyhow!("Invalid username: {}", username));
        }
        tracing::debug!("[LOGIN] Username validated: {}", username);

        // Generate UUID for offline mode
        let uuid = Self::generate_offline_uuid(&username);
        tracing::debug!("[LOGIN] Generated UUID: {}", uuid);

        // Send Login Success packet
        tracing::debug!("[LOGIN] Sending Login Success packet...");
        if let Err(e) = self.send_login_success(&username, &uuid).await {
            warn!("[LOGIN] Failed to send login success: {}", e);
            return Err(e);
        }
        tracing::debug!("[LOGIN] Login Success sent");

        info!("[LOGIN] Player '{}' (UUID: {}) logged in successfully", username, uuid);

        // Wait for Login Acknowledged packet (required for 1.20.2+)
        tracing::debug!("[LOGIN] Waiting for Login Acknowledged packet...");
        if let Err(e) = self.read_login_acknowledged().await {
            warn!("[LOGIN] Failed to receive Login Acknowledged: {}", e);
            return Err(e);
        }
        tracing::info!("[LOGIN] Login Acknowledged received");

        Ok(PlayerLogin { username, uuid })
    }

    async fn read_handshake(&mut self) -> Result<()> {
        let mut length_buf = [0u8; 5];

        // Read packet length
        let mut bytes_read = 0;

        while let Ok(n_bytes) = self
            .stream
            .read(&mut length_buf[bytes_read..bytes_read + 1])
            .await
        {
            if n_bytes == 0 {
                // early return on closed connection
                return Err(anyhow!("Connection closed during handshake"));
            }
            let maybe = length_buf[bytes_read] & 0x80 == 0;
            tracing::debug!("Maybe value: {:08b}", length_buf[bytes_read]);
            if maybe {
                bytes_read += 1;
                break;
            }
            bytes_read += 1;
            if bytes_read >= 5 {
                return Err(anyhow!("Packet length too long"));
            }
        }

        // loop {
        //     let n = self
        //         .stream
        //         .read(&mut length_buf[bytes_read..bytes_read + 1])
        //         .await?;
        //     if n == 0 {
        //         return Err(anyhow!("Connection closed during handshake"));
        //     }
        //
        //     let maybe = length_buf[bytes_read] & 0x80 == 0;
        //
        //     tracing::debug!("Maybe value: {:08b}", length_buf[bytes_read]);
        //
        //     if maybe {
        //         bytes_read += 1;
        //         break;
        //     }
        //     bytes_read += 1;
        //     if bytes_read >= 5 {
        //         return Err(anyhow!("Packet length too long"));
        //     }
        // }

        let packet_length = read_varint(&mut std::io::Cursor::new(&length_buf[..bytes_read]))? as usize;

        // Read packet data
        let mut packet_data = vec![0u8; packet_length];
        self.stream.read_exact(&mut packet_data).await?;

        let mut reader = PacketReader::new(&packet_data);
        let packet_id: i32 = reader.read_varint()?;

        if packet_id != 0x00 {
            return Err(anyhow!("Expected Handshake packet (0x00), got {:#x}", packet_id));
        }

        self.protocol_version = reader.read_varint()?;
        let _server_addr = reader.read_string()?;
        let _server_port = reader.read_short()?;
        let next_state = reader.read_varint()?;

        // Accept both Status (1) and Login (2) states
        // Client may ping first, then connect for login
        if next_state != 1 && next_state != 2 {
            return Err(anyhow!("Expected Status (1) or Login (2) state, got {}", next_state));
        }

        Ok(())
    }

    async fn read_login_acknowledged(&mut self) -> Result<()> {
        let mut length_buf = [0u8; 5];

        // Read packet length
        let mut bytes_read = 0;
        while let Ok(n_bytes) = self
            .stream
            .read(&mut length_buf[bytes_read..bytes_read + 1])
            .await
        {
            if n_bytes == 0 {
                // early return on closed connection
                return Err(anyhow!("Connection closed during login acknowledged"));
            }

            let maybe = length_buf[bytes_read] & 0x80 == 0;
            tracing::debug!("Maybe value: {:08b}", length_buf[bytes_read]);
            if maybe {
                bytes_read += 1;
                break;
            }
            bytes_read += 1;
            if bytes_read >= 5 {
                return Err(anyhow!("Packet length too long"));
            }
        }

        // loop {
        //     let n = self
        //         .stream
        //         .read(&mut length_buf[bytes_read..bytes_read + 1])
        //         .await?;
        //     if n == 0 {
        //         return Err(anyhow!("Connection closed during login acknowledged"));
        //     }
        //
        //     if length_buf[bytes_read] & 0x80 == 0 {
        //         bytes_read += 1;
        //         break;
        //     }
        //     bytes_read += 1;
        //     if bytes_read >= 5 {
        //         return Err(anyhow!("Packet length too long"));
        //     }
        // }

        tracing::debug!("[LOGIN] Reading Login Acknowledged packet, length bytes read: {}", bytes_read);

        let packet_length: usize =
            read_varint(&mut std::io::Cursor::new(&length_buf[..bytes_read]))? as usize;

        // Read packet data
        let mut packet_data: Vec<u8> = vec![0u8; packet_length];
        self.stream.read_exact(&mut packet_data).await?;

        let mut reader = PacketReader::new(&packet_data);
        let packet_id: i32 = reader.read_varint()?;

        if packet_id != 0x03 {
            return Err(anyhow!("Expected Login Acknowledged packet (0x03), got {:#x}", packet_id));
        }

        // Login Acknowledged has no payload
        Ok(())
    }

    async fn read_login_start(&mut self) -> Result<String> {
        let mut length_buf = [0u8; 5];

        // Read packet length
        let mut bytes_read = 0;

        while let Ok(n_bytes) = self
            .stream
            .read(&mut length_buf[bytes_read..bytes_read + 1])
            .await
        {
            if n_bytes == 0 {
                // early return on closed connection
                return Err(anyhow!("Connection closed during login start"));
            }

            let maybe = length_buf[bytes_read] & 0x80 == 0;
            tracing::debug!("Maybe value: {:08b}", length_buf[bytes_read]);
            if maybe {
                bytes_read += 1;
                break;
            }
            bytes_read += 1;
            if bytes_read >= 5 {
                return Err(anyhow!("Packet length too long"));
            }
        }

        // loop {
        //     let n = self
        //         .stream
        //         .read(&mut length_buf[bytes_read..bytes_read + 1])
        //         .await?;
        //     if n == 0 {
        //         return Err(anyhow!("Connection closed during login start"));
        //     }
        //
        //     if length_buf[bytes_read] & 0x80 == 0 {
        //         bytes_read += 1;
        //         break;
        //     }
        //     bytes_read += 1;
        //     if bytes_read >= 5 {
        //         return Err(anyhow!("Packet length too long"));
        //     }
        // }

        let packet_length = read_varint(&mut std::io::Cursor::new(&length_buf[..bytes_read]))? as usize;

        // Read packet data
        let mut packet_data = vec![0u8; packet_length];
        self.stream.read_exact(&mut packet_data).await?;

        let mut reader = PacketReader::new(&packet_data);
        let packet_id = reader.read_varint()?;

        if packet_id != 0x00 {
            return Err(anyhow!("Expected Login Start packet (0x00), got {:#x}", packet_id));
        }

        let username = reader.read_string()?;

        if username.is_empty() || username.len() > 16 {
            return Err(anyhow!("Invalid username length"));
        }

        Ok(username)
    }

    async fn send_login_success(&mut self, username: &str, uuid: &Uuid) -> Result<()> {
        let mut writer = PacketWriter::new();

        // Game Profile structure:
        // - UUID
        // - Username
        // - Properties (array of {name, value, signature})

        // Write UUID
        writer.write_uuid(uuid);

        // Write username
        writer.write_string(username);

        // Write properties count (empty array)
        writer.write_varint(0);

        let packet_data = writer.finish();
        let packet_id = write_varint(0x02);

        // Write packet: [length][id][data]
        let mut frame = Vec::new();
        frame.extend_from_slice(&write_varint((packet_id.len() + packet_data.len()) as i32));
        frame.extend_from_slice(&packet_id);
        frame.extend_from_slice(&packet_data);

        self.stream.write_all(&frame).await?;
        self.stream.flush().await?;

        Ok(())
    }

    fn generate_offline_uuid(username: &str) -> Uuid {
        // Create UUID v3 from username (offline mode)
        // UUID v3 uses MD5 hash of namespace + name
        let namespace = Uuid::NAMESPACE_DNS;
        let offline_name = format!("OfflinePlayer:{}", username);
        Uuid::new_v3(&namespace, offline_name.as_bytes())
    }

    fn is_valid_username(username: &str) -> bool {
        // Minecraft username must be 3-16 characters, alphanumeric + underscore
        if username.is_empty() || username.len() > 16 {
            return false;
        }

        username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    }

    async fn send_disconnect(&mut self, reason: &str) -> Result<()> {
        let mut writer = PacketWriter::new();

        // Write JSON text component
        // Escape JSON properly
        let escaped_reason = reason
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t");

        let json_message = format!(r#"{{"text":"{}"}}"#, escaped_reason);
        tracing::debug!("[LOGIN] Disconnect JSON: {}", json_message);
        writer.write_string(&json_message);

        let packet_data = writer.finish();
        let packet_id = write_varint(0x00); // Disconnect packet ID in Login state

        // Write packet: [length][id][data]
        let mut frame = Vec::new();
        frame.extend_from_slice(&write_varint((packet_id.len() + packet_data.len()) as i32));
        frame.extend_from_slice(&packet_id);
        frame.extend_from_slice(&packet_data);

        tracing::debug!("[LOGIN] Sending disconnect packet ({} bytes)", frame.len());
        self.stream.write_all(&frame).await?;
        self.stream.flush().await?;

        Ok(())
    }

    pub fn get_stream(self) -> TcpStream {
        self.stream
    }
}
