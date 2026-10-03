#![allow(dead_code)]

use std::io::{Cursor, Read};
use std::ops::{AddAssign, BitOrAssign};

use anyhow::{Result, anyhow};
use bytes::{BufMut, Bytes, BytesMut};
use uuid::Uuid;

use crate::network::ByteWritable;

/// Validate a Minecraft identifier (resource location)
/// Ensures the identifier contains no null bytes and only valid characters
fn validate_identifier(id: &str) -> Result<()> {
    if id.contains('\0') {
        return Err(anyhow!("Identifier contains null byte: {:?}", id));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | ':'))
    {
        return Err(anyhow!("Invalid identifier characters: {}", id));
    }
    Ok(())
}

/// Minecraft protocol packet structure
pub struct Packet {
    pub id:   i32,
    pub data: Bytes,
}

impl Packet {
    pub fn new(id: i32, data: Bytes) -> Self {
        Self { id, data }
    }
}

/// Read a Minecraft varint from bytes
pub fn read_varint(cursor: &mut Cursor<&[u8]>) -> std::io::Result<i32> {
    let mut result: i32 = 0;
    let mut bytes_read: i32 = 0;
    let mut byte: [u8; 1] = [0u8; 1];

    while let Ok(()) = cursor.read_exact(&mut byte) {
        //
        let b = byte[0];
        result.bitor_assign(((b & 0x7F) as i32) << (7 * bytes_read));
        if (b & 0x80) == 0 {
            break;
        }
        bytes_read.add_assign(1);
        if bytes_read >= 5 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "VarInt is too big"));
        }
    }

    // loop {
    //     cursor.read_exact(&mut byte)?;
    //     let b = byte[0];
    //     result |= ((b & 0x7F) as i32) << (7 * bytes_read);
    //
    //     if (b & 0x80) == 0 {
    //         break;
    //     }
    //
    //     bytes_read += 1;
    //     if bytes_read >= 5 {
    //         return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "VarInt is too big"));
    //     }
    // }

    tracing::debug!("[PACKET] Packet length bytes read: {}", bytes_read);
    tracing::debug!("[PACKET] Read VarInt: {}", result);

    Ok(result)
}

/// Write a Minecraft varint to bytes
pub fn write_varint(value: i32) -> Vec<u8> {
    let mut result: Vec<u8> = Vec::new();
    let mut v: u32 = value as u32;

    loop {
        let mut temp: u8 = (v & 0x7F) as u8;
        v >>= 7; // u32
        if v != 0 {
            temp |= 0x80;
        }
        result.push(temp);
        if v == 0 {
            // v: u32
            break;
        }
    }

    result
}

pub struct PacketWriter {
    data: BytesMut,
}

impl Default for PacketWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl ByteWritable for PacketWriter {
    fn write_varint<N: Into<i32>>(&mut self, value: N) {
        self.data.extend_from_slice(&write_varint(value.into()))
    }

    fn write_string<S: AsRef<str>>(&mut self, s: S) {
        let bytes = s.as_ref().as_bytes();
        self.write_varint(bytes.len() as i32);
        self.data.extend_from_slice(bytes);
    }

    fn write_byte<N: Into<u8>>(&mut self, value: N) {
        self.data.put_u8(value.into());
    }

    fn write_short<N: Into<i16>>(&mut self, value: N) {
        self.data.extend_from_slice(&value.into().to_be_bytes());
    }

    fn write_int<N: Into<i32>>(&mut self, value: N) {
        self.data.extend_from_slice(&value.into().to_be_bytes());
    }

    fn write_long<N: Into<i64>>(&mut self, value: N) {
        self.data.put_i64_ne(value.into());
    }

    fn write_float<N: Into<f32>>(&mut self, value: N) {
        self.data.extend_from_slice(&value.into().to_be_bytes());
    }

    fn write_double<N: Into<f64>>(&mut self, value: N) {
        self.data.put_f64_ne(value.into());
    }

    fn write_bool<B: Into<bool>>(&mut self, value: B) {
        self.data.put_u8(if value.into() { 1 } else { 0 });
    }

    fn write_uuid<U: AsRef<Uuid>>(&mut self, uuid: U) {
        self.data.extend_from_slice(uuid.as_ref().as_bytes());
    }

    fn write_bytes<A: AsRef<[u8]>>(&mut self, bytes: A) {
        self.data.extend_from_slice(bytes.as_ref());
    }

    fn finish(self) -> BytesMut {
        self.data
    }
}

impl PacketWriter {
    pub fn new() -> Self {
        Self {
            data: BytesMut::new(),
        }
    }
}

pub struct PacketReader<'a> {
    cursor: Cursor<&'a [u8]>,
}

impl<'a> PacketReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            cursor: Cursor::new(data),
        }
    }

    pub fn read_varint(&mut self) -> std::io::Result<i32> {
        read_varint(&mut self.cursor)
    }

    pub fn read_string(&mut self) -> std::io::Result<String> {
        let len = self.read_varint()? as usize;
        let mut buf = vec![0u8; len];
        self.cursor.read_exact(&mut buf)?;
        Ok(String::from_utf8_lossy(&buf).to_string())
    }

    pub fn read_byte(&mut self) -> std::io::Result<u8> {
        let mut buf = [0u8; 1];
        self.cursor.read_exact(&mut buf)?;
        Ok(buf[0])
    }

    pub fn read_short(&mut self) -> std::io::Result<i16> {
        let mut buf = [0u8; 2];
        self.cursor.read_exact(&mut buf)?;
        Ok(i16::from_ne_bytes(buf))
    }

    pub fn read_int(&mut self) -> std::io::Result<i32> {
        let mut buf = [0u8; 4];
        self.cursor.read_exact(&mut buf)?;
        Ok(i32::from_ne_bytes(buf))
    }

    pub fn read_long(&mut self) -> std::io::Result<i64> {
        let mut buf = [0u8; 8];
        self.cursor.read_exact(&mut buf)?;
        Ok(i64::from_ne_bytes(buf))
    }

    pub fn read_float(&mut self) -> std::io::Result<f32> {
        let mut buf = [0u8; 4];
        self.cursor.read_exact(&mut buf)?;
        Ok(f32::from_ne_bytes(buf))
    }

    pub fn read_double(&mut self) -> std::io::Result<f64> {
        let mut buf = [0u8; 8];
        self.cursor.read_exact(&mut buf)?;
        Ok(f64::from_ne_bytes(buf))
    }

    pub fn read_bool(&mut self) -> std::io::Result<bool> {
        Ok(self.read_byte()? != 0)
    }

    pub fn read_uuid(&mut self) -> std::io::Result<Uuid> {
        let mut buf = [0u8; 16];
        self.cursor.read_exact(&mut buf)?;
        Ok(Uuid::from_bytes(buf))
    }

    pub fn read_bytes(&mut self, len: usize) -> std::io::Result<Vec<u8>> {
        let mut buf = vec![0u8; len];
        self.cursor.read_exact(&mut buf)?;
        Ok(buf)
    }

    pub fn remaining(&self) -> usize {
        let pos = self.cursor.position() as usize;
        self.cursor.get_ref().len() - pos
    }
}

// Helper functions for Prefixed Optional encoding
pub fn write_optional_bytes<A: AsRef<[u8]>>(writer: &mut PacketWriter, data: Option<A>) {
    match data {
        Some(bytes) => {
            let bytes_ref = bytes.as_ref();
            writer.write_varint(bytes_ref.len() as i32);
            writer.write_bytes(bytes_ref);
        }
        None => {
            writer.write_varint(-1); // -1 indicates no data
        }
    }
}

#[derive(Debug)]
pub struct DimensionCompound {
    name:             &'static str,
    height:           i32,
    min_y:            i32,
    has_skylight:     bool,
    has_ceiling:      bool,
    ultrawarm:        bool,
    natural:          bool,
    coordinate_scale: f32,
}

impl DimensionCompound {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: &'static str,
        height: i32,
        min_y: i32,
        has_skylight: bool,
        has_ceiling: bool,
        ultrawarm: bool,
        natural: bool,
        coordinate_scale: f32,
    ) -> Self {
        Self {
            name,
            height,
            min_y,
            has_skylight,
            has_ceiling,
            ultrawarm,
            natural,
            coordinate_scale,
        }
    }
}

pub struct DamageTypeCompound {
    message_id: &'static str,
    scaling:    &'static str,
    exhaustion: f32,
}

impl DamageTypeCompound {
    pub fn new<S>(message_id: &'static S, scaling: &'static S, exhaustion: f32) -> Self
    where
        S: AsRef<str> + 'static + ?Sized,
    {
        Self {
            message_id: message_id.as_ref(),
            scaling: scaling.as_ref(),
            exhaustion,
        }
    }
}

// Simple NBT encoder for registry data
#[derive(Debug)]
pub struct NBTBuilder {
    data: BytesMut,
}

impl Default for NBTBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl NBTBuilder {
    pub fn new() -> Self {
        Self {
            data: BytesMut::new(),
        }
    }

    /// Create an empty compound (root compound with no tags)
    pub fn empty_compound() -> Vec<u8> {
        vec![0x0A, 0x00, 0x00, 0x00] // TAG_Compound, empty name, TAG_End
    }

    /// Create a dimension type compound with minimal properties
    pub fn dimension_compound(dim_comp: DimensionCompound) -> Vec<u8> {
        let mut bytes = BytesMut::new();

        // TAG_Compound
        bytes.put_u8(0x0A);

        // Root compound name (empty)
        bytes.extend_from_slice(&(0i16).to_be_bytes());

        // Helper macro to write NBT tags
        macro_rules! write_nbt_byte {
            ($name:expr, $value:expr) => {
                bytes.put_u8(0x01); // TAG_Byte
                let name_bytes = $name.as_bytes();
                bytes.extend_from_slice(&(name_bytes.len() as i16).to_be_bytes());
                bytes.extend_from_slice(name_bytes);
                bytes.put_u8($value);
            };
        }

        macro_rules! write_nbt_int {
            ($name:expr, $value:expr) => {
                bytes.put_u8(0x03); // TAG_Int
                let name_bytes = $name.as_bytes();
                bytes.extend_from_slice(&(name_bytes.len() as i16).to_be_bytes());
                bytes.extend_from_slice(name_bytes);
                bytes.extend_from_slice(&($value as i32).to_be_bytes());
            };
        }

        macro_rules! write_nbt_float {
            ($name:expr, $value:expr) => {
                bytes.put_u8(0x05); // TAG_Float
                let name_bytes = $name.as_bytes();
                bytes.extend_from_slice(&(name_bytes.len() as i16).to_be_bytes());
                bytes.extend_from_slice(name_bytes);
                bytes.extend_from_slice(&($value as f32).to_be_bytes());
            };
        }

        // Write all fields
        write_nbt_byte!(
            "bed_works",
            if dim_comp.name.contains("nether") || dim_comp.name.contains("end") {
                0
            } else {
                1
            }
        );
        write_nbt_byte!("has_ceiling", if dim_comp.has_ceiling { 1 } else { 0 });
        write_nbt_byte!("has_skylight", if dim_comp.has_skylight { 1 } else { 0 });
        write_nbt_byte!("has_raids", if dim_comp.name.contains("end") { 0 } else { 1 });
        write_nbt_int!("height", dim_comp.height);
        write_nbt_int!("logical_height", dim_comp.height);
        write_nbt_int!("min_y", dim_comp.min_y);
        write_nbt_byte!("ultrawarm", if dim_comp.ultrawarm { 1 } else { 0 });
        write_nbt_byte!("natural", if dim_comp.natural { 1 } else { 0 });
        write_nbt_float!("coordinate_scale", dim_comp.coordinate_scale);
        write_nbt_byte!("piglin_safe", 0);
        write_nbt_byte!("respawn_anchor_works", if dim_comp.name.contains("nether") { 1 } else { 0 });

        // TAG_End
        bytes.put_u8(0x00);

        bytes.to_vec()
    }

    /// Create a damage type compound
    pub fn damage_type_compound(
        // message_id: &str, scaling: &str, exhaustion: f32
        dmg_comp: DamageTypeCompound,
    ) -> Vec<u8> {
        let mut bytes = BytesMut::new();

        bytes.put_u8(0x0A); // TAG_Compound
        bytes.extend_from_slice(&(0i16).to_be_bytes()); // empty root name

        // exhaustion: TAG_Float
        bytes.put_u8(0x05);
        let name_bytes = b"exhaustion";
        bytes.extend_from_slice(&(name_bytes.len() as i16).to_be_bytes());
        bytes.extend_from_slice(name_bytes);
        bytes.extend_from_slice(&dmg_comp.exhaustion.to_be_bytes());

        // message_id: TAG_String
        bytes.put_u8(0x08);
        let name_bytes = b"message_id";
        bytes.extend_from_slice(&(name_bytes.len() as i16).to_be_bytes());
        bytes.extend_from_slice(name_bytes);
        let value_bytes = dmg_comp.message_id.as_bytes();
        bytes.extend_from_slice(&(value_bytes.len() as i16).to_be_bytes());
        bytes.extend_from_slice(value_bytes);

        // scaling: TAG_String
        bytes.put_u8(0x08);
        let name_bytes = b"scaling";
        bytes.extend_from_slice(&(name_bytes.len() as i16).to_be_bytes());
        bytes.extend_from_slice(name_bytes);
        let value_bytes = dmg_comp.scaling.as_bytes();
        bytes.extend_from_slice(&(value_bytes.len() as i16).to_be_bytes());
        bytes.extend_from_slice(value_bytes);

        // TAG_End
        bytes.put_u8(0x00);

        bytes.to_vec()
    }
}
