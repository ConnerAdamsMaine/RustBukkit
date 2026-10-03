use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::Result;
use tracing::{debug, info};

use crate::network::protocol::{PacketReader, read_varint};

pub struct PacketLogger {
    packet_dir: PathBuf,
    counter:    AtomicUsize,
}

impl PacketLogger {
    pub fn new() -> Result<Self> {
        let packet_dir = PathBuf::from("packets");

        // Create packets directory if it doesn't exist
        if !packet_dir.exists() {
            fs::create_dir_all(&packet_dir)?;
        }

        // Clear old packets on startup
        if packet_dir.exists() {
            for entry in fs::read_dir(&packet_dir)?.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().is_some_and(|ext| ext == "bin") {
                    let _ = fs::remove_file(path);
                }
            }
        }

        info!("[PACKET_LOG] Logger initialized, packets directory ready");

        Ok(Self {
            packet_dir,
            counter: AtomicUsize::new(0),
        })
    }

    pub fn log_client_packet(&self, data: &[u8]) -> Result<()> {
        let count = self.counter.fetch_add(1, Ordering::SeqCst);
        let filename = format!("{:06}_client.bin", count);
        let path = self.packet_dir.join(&filename);

        fs::write(&path, data)?;

        // Parse and display packet info
        self.log_packet_details("CLIENT", count, data);

        Ok(())
    }

    pub fn log_server_packet(&self, data: &[u8]) -> Result<()> {
        let count = self.counter.fetch_add(1, Ordering::SeqCst);
        let filename = format!("{:06}_server.bin", count);
        let path = self.packet_dir.join(&filename);

        fs::write(&path, data)?;

        // Parse and display packet info
        self.log_packet_details("SERVER", count, data);

        Ok(())
    }

    fn log_packet_details(&self, direction: &str, count: usize, data: &[u8]) {
        if data.is_empty() {
            debug!("[PACKET_LOG:{}] #{:06} Empty packet ({} bytes)", direction, count, data.len());
            return;
        }

        // Try to parse packet length and ID
        match self.parse_packet(data) {
            Some((packet_id, payload_len)) => {
                info!(
                    "[PACKET_LOG:{}] #{:06} Packet ID: 0x{:02x} | Total bytes: {} | Payload: {} bytes | Hex: {}",
                    direction,
                    count,
                    packet_id,
                    data.len(),
                    payload_len,
                    Self::hex_preview(data, 64)
                );
                debug!("[PACKET_LOG:{}] #{:06} Full hex: {}", direction, count, Self::hex_full(data));
            }
            None => {
                info!(
                    "[PACKET_LOG:{}] #{:06} Could not parse packet ({} bytes) | Hex: {}",
                    direction,
                    count,
                    data.len(),
                    Self::hex_preview(data, 64)
                );
            }
        }
    }

    fn parse_packet(&self, data: &[u8]) -> Option<(i32, usize)> {
        if data.is_empty() {
            return None;
        }

        let mut cursor = Cursor::new(data);

        // Read packet length (varint)
        let packet_length = match read_varint(&mut cursor) {
            Ok(len) => len as usize,
            Err(_) => return None,
        };

        let length_bytes = cursor.position() as usize;

        // Read packet ID (varint)
        let packet_id = match read_varint(&mut cursor) {
            Ok(id) => id,
            Err(_) => return None,
        };

        let id_bytes = cursor.position() as usize - length_bytes;
        let payload_len = packet_length.saturating_sub(id_bytes);

        Some((packet_id, payload_len))
    }

    fn hex_preview(data: &[u8], max_chars: usize) -> String {
        let hex = Self::bytes_to_hex(data);
        if hex.len() > max_chars {
            format!("{}...", &hex[..max_chars])
        } else {
            hex
        }
    }

    fn hex_full(data: &[u8]) -> String {
        Self::bytes_to_hex(data)
    }

    fn bytes_to_hex(bytes: &[u8]) -> String {
        bytes
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl AsRef<PacketLogger> for PacketLogger {
    fn as_ref(&self) -> &PacketLogger {
        self
    }
}

impl Clone for PacketLogger {
    fn clone(&self) -> Self {
        let counter = AtomicUsize::new(self.counter.load(Ordering::SeqCst));
        Self {
            packet_dir: self.packet_dir.clone(),
            counter,
        }
    }
}

impl Default for PacketLogger {
    fn default() -> Self {
        Self::new().expect("Failed to initialize PacketLogger")
    }
}
