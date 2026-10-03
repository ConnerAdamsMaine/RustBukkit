use std::sync::Arc;

use anyhow::{Result, anyhow};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tracing::debug;

use crate::network::{
    ByteWritable,
    DamageTypeCompound,
    DimensionCompound,
    NBTBuilder,
    PacketReader,
    PacketWriter,
    read_varint,
    write_varint,
};

pub enum ConfigurationAckPacket {
    ClientInformation = 0x00,
    ServerboundPluginMessage = 0x01,
    ServerboundKnownPacks = 0x02,
    AcknowledgeFinishConfiguration = 0x03,
}

impl From<i32> for ConfigurationAckPacket {
    fn from(value: i32) -> Self {
        match value {
            0x00 => ConfigurationAckPacket::ClientInformation,
            0x01 => ConfigurationAckPacket::ServerboundPluginMessage,
            0x02 => ConfigurationAckPacket::ServerboundKnownPacks,
            0x03 => ConfigurationAckPacket::AcknowledgeFinishConfiguration,
            _ => panic!("Invalid ConfigurationAckPacket value: {}", value),
        }
    }
}

pub struct ConfigurationHandler;

impl ConfigurationHandler {
    /// Handle the Configuration phase after login
    /// Sends required registry data and finish configuration packet
    pub async fn handle_configuration(stream: &mut TcpStream) -> Result<()> {
        debug!("[CONFIG] Starting configuration phase");

        let stream_c = Arc::new(Mutex::new(stream));

        // Send required Registry Data packets
        // These define the game registries that client and server must agree on
        // tokio::try_join!(
        //     Self::send_registry_data(Arc::clone(&stream_c)),
        //     Self::send_finish_configuration(Arc::clone(&stream_c)),
        //     Self::read_acknowledge_finish_configuration(Arc::clone(&stream_c)),
        // )?;

        Self::send_registry_data(Arc::clone(&stream_c)).await?;
        Self::send_finish_configuration(Arc::clone(&stream_c)).await?;
        Self::read_acknowledge_finish_configuration(Arc::clone(&stream_c)).await?;

        debug!("[CONFIG] Configuration phase complete");
        Ok(())
    }

    /// Send Registry Data packets for critical registries
    /// Registry Data packet structure (Protocol ID: 0x07 in Configuration state):
    /// - Registry ID (Identifier): The registry name (e.g., "minecraft:dimension_type")
    /// - Entries (Prefixed Array):
    ///   - Entry ID (Identifier): The entry name (e.g., "minecraft:overworld")
    ///   - Data (Prefixed Optional NBT): Entry data in NBT format (or null if from known packs)
    async fn send_registry_data(stream: Arc<Mutex<&mut TcpStream>>) -> Result<()> {
        // Send minimal required registries for basic functionality
        // For a full server, you'd need to send ALL synchronized registries
        let registries = vec![
            ("minecraft:dimension_type", Self::get_dimension_type_registry()),
            ("minecraft:damage_type", Self::get_damage_type_registry()),
        ];

        for (registry_id, entries) in registries {
            Self::send_single_registry(Arc::clone(&stream), registry_id, &entries).await?;
        }

        debug!("[CONFIG] Registry Data packets sent");
        Ok(())
    }

    /// Send a single Registry Data packet
    /// Packet Structure (1.21.7):
    /// - Registry ID (String): e.g., "minecraft:dimension_type"
    /// - Entries (VarInt count, then array):
    ///   - Entry ID (String): e.g., "minecraft:overworld"
    ///   - Data (Optional NBT - Prefixed by length):
    ///     - Length (-1 for null, otherwise byte count)
    ///     - NBT Data: The serialized NBT data
    async fn send_single_registry(
        stream: Arc<Mutex<&mut TcpStream>>,
        registry_id: &str,
        entries: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<()> {
        let mut writer = PacketWriter::new();

        tracing::debug!("[CONFIG] Preparing Registry Data for: {}", registry_id);
        tracing::debug!("[CONFIG] Number of entries: {}", entries.len());

        // Write Registry ID (as a String identifier)
        writer.write_string(registry_id);

        // Write number of entries
        writer.write_varint(entries.len() as i32);

        // Write each entry
        for (entry_id, nbt_data) in entries {
            // Convert entry_id bytes to string if needed
            let id_str = String::from_utf8_lossy(entry_id).to_string();

            // Write Entry ID (as a String identifier)
            writer.write_string(&id_str);

            // Write Data (Prefixed Optional NBT)
            // Length of NBT data followed by the data itself
            if nbt_data.is_empty() {
                // If no data, write -1 to indicate null
                writer.write_varint(-1);
            } else {
                writer.write_varint(nbt_data.len() as i32);
                writer.write_bytes(nbt_data);
            }
        }

        let packet_data = writer.finish();
        let packet_id = write_varint(0x07); // Registry Data packet ID

        // Write packet: [length][id][data]
        let mut frame = Vec::new();
        frame.extend_from_slice(&write_varint((packet_id.len() + packet_data.len()) as i32));
        frame.extend_from_slice(&packet_id);
        frame.extend_from_slice(&packet_data);

        let stream = &mut *stream.lock().await;
        stream.write_all(&frame).await?;
        stream.flush().await?;
        debug!("[CONFIG] Sent registry data for: {} ({} entries)", registry_id, entries.len());

        Ok(())
    }

    /// Get the dimension_type registry entries with proper NBT data
    #[rustfmt::skip]
    fn get_dimension_type_registry() -> Vec<(Vec<u8>, Vec<u8>)> {
        let overworld_comp =    DimensionCompound::new("overworld", 384, -64, true, false, false, true, 1.0);
        let the_nether_comp =   DimensionCompound::new("the_nether", 256, 0, false, true, true, false, 8.0);
        let the_end_comp =      DimensionCompound::new("the_end", 256, 0, false, false, false, false, 1.0);

        vec![
            ("minecraft:overworld".into(),  NBTBuilder::dimension_compound(overworld_comp)),
            ("minecraft:the_nether".into(), NBTBuilder::dimension_compound(the_nether_comp)),
            ("minecraft:the_end".into(),    NBTBuilder::dimension_compound(the_end_comp)),
        ]
    }

    /// Get the damage_type registry entries with proper NBT data
    #[rustfmt::skip]
    fn get_damage_type_registry() -> Vec<(Vec<u8>, Vec<u8>)> {
        let generic_comp =          DamageTypeCompound::new("generic", "when_caused_by_living_non_player", 0.0);
        let player_attack_comp =    DamageTypeCompound::new("player_attack", "when_caused_by_living_non_player", 0.1);
        let player_knockback_comp = DamageTypeCompound::new("player_knockback", "when_caused_by_living_non_player", 0.1);
        let world_border_comp =     DamageTypeCompound::new("world_border", "always", 0.0);
        let falling_comp =          DamageTypeCompound::new("falling", "when_caused_by_living_non_player", 0.1);
        let suffocation_comp =      DamageTypeCompound::new("suffocation", "always", 0.0);
        let drowning_comp =         DamageTypeCompound::new("drowning", "always", 0.0);
        let starving_comp =         DamageTypeCompound::new("starving", "always", 0.0);
        let falling_anvil_comp =    DamageTypeCompound::new("falling_anvil", "when_caused_by_living_non_player", 0.1);

        vec![
            ("minecraft:generic".into(),            NBTBuilder::damage_type_compound(generic_comp)),
            ("minecraft:player_attack".into(),      NBTBuilder::damage_type_compound(player_attack_comp)),
            ("minecraft:player_knockback".into(),   NBTBuilder::damage_type_compound(player_knockback_comp)),
            ("minecraft:world_border".into(),       NBTBuilder::damage_type_compound(world_border_comp)),
            ("minecraft:falling".into(),            NBTBuilder::damage_type_compound(falling_comp)),
            ("minecraft:suffocation".into(),        NBTBuilder::damage_type_compound(suffocation_comp)),
            ("minecraft:drowning".into(),           NBTBuilder::damage_type_compound(drowning_comp)),
            ("minecraft:starving".into(),           NBTBuilder::damage_type_compound(starving_comp)),
            ("minecraft:falling_anvil".into(),      NBTBuilder::damage_type_compound(falling_anvil_comp)),
        ]
    }

    async fn send_finish_configuration(stream: Arc<Mutex<&mut TcpStream>>) -> Result<()> {
        debug!("[CONFIG] Sending Finish Configuration");
        // Finish Configuration packet (0x03 in Configuration state)
        let packet_id = write_varint(0x03);

        // This packet has no payload, just packet ID
        let mut frame = Vec::new();
        frame.extend_from_slice(&write_varint(packet_id.len() as i32));
        frame.extend_from_slice(&packet_id);

        let mut stream = stream.lock().await;
        stream.write_all(&frame).await?;
        stream.flush().await?;

        debug!("[CONFIG] Finish Configuration sent");
        Ok(())
    }

    async fn read_acknowledge_finish_configuration(stream: Arc<Mutex<&mut TcpStream>>) -> Result<()> {
        debug!("[CONFIG] Waiting for Acknowledge Finish Configuration");
        // Client may send optional packets before Acknowledge Finish Configuration
        // Valid packets in Configuration state (serverbound):
        // 0x00 = Client Information
        // 0x01 = Serverbound Plugin Message
        // 0x02 = Serverbound Known Packs
        // 0x03 = Acknowledge Finish Configuration

        loop {
            let mut length_buf = [0u8; 5];

            // Read packet length
            let mut bytes_read = 0;
            loop {
                // let stream = &mut *stream.lock().unwrap();
                let mut stream = stream.lock().await;
                let n = stream.read(&mut length_buf[bytes_read..bytes_read + 1]).await?;
                tracing::debug!("[CONFIG] Read {} bytes for packet length", n);
                if n == 0 {
                    return Err(anyhow!("Connection closed during acknowledge finish configuration"));
                }

                let maybe = length_buf[bytes_read] & 0x80 == 0;

                // 2026-01-04T07:56:01.636839Z DEBUG 234: [CONFIG] Packet length byte: 00001111
                tracing::debug!("[CONFIG] Packet length byte: {:08b}", length_buf[bytes_read]);
                if maybe {
                    bytes_read += 1;
                    break;
                }
                bytes_read += 1;
                if bytes_read >= 5 {
                    return Err(anyhow!("Packet length too long"));
                }
            }

            // hmmmmmmmmmmmmmmmmmmmmmmmm
            // 2026-01-04T07:51:32.950695Z DEBUG 228: [CONFIG] Read 1 bytes for packet length
            // 2026-01-04T07:51:32.950700Z DEBUG 243: [CONFIG] Packet length bytes read: 1

            tracing::debug!("[CONFIG] Packet length bytes read: {}", bytes_read);

            let packet_length = read_varint(&mut std::io::Cursor::new(&length_buf[..bytes_read]))? as usize;

            tracing::debug!("[CONFIG] Packet length: {}", packet_length);

            // Read packet data
            let mut packet_data = vec![0u8; packet_length];
            let mut stream = stream.lock().await;
            stream.read_exact(&mut packet_data).await?;

            let mut reader = PacketReader::new(&packet_data);
            let packet_id = reader.read_varint()?;

            tracing::debug!("[CONFIG] Received packet ID: 0x{:02X}", packet_id);

            let packet_id_enum: ConfigurationAckPacket = packet_id.into();

            match packet_id_enum {
                ConfigurationAckPacket::ClientInformation => {
                    // Client Information - optional, skip it
                    debug!("[CONFIG] Received Client Information (0x00)");
                }
                ConfigurationAckPacket::ServerboundPluginMessage => {
                    // Serverbound Plugin Message - optional, skip it
                    debug!("[CONFIG] Received Serverbound Plugin Message (0x01)");
                }
                ConfigurationAckPacket::ServerboundKnownPacks => {
                    // Serverbound Known Packs - optional, skip it
                    debug!("[CONFIG] Received Serverbound Known Packs (0x02)");
                }
                ConfigurationAckPacket::AcknowledgeFinishConfiguration => {
                    // Acknowledge Finish Configuration - this is what we're waiting for
                    debug!("[CONFIG] Acknowledge Finish Configuration received");
                    return Ok(());
                }
            }
        } // end loop
    }
}
