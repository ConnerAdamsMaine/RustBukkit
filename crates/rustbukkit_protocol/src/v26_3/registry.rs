use std::collections::HashSet;

use rustbukkit_encoding::MAX_PACKET_LENGTH;
use thiserror::Error;

use super::parse_identifier;

const MAX_NBT_DEPTH: usize = 512;
const MAX_IDENTIFIER_LEN: usize = 32_767;

#[derive(Clone, Debug, PartialEq)]
pub struct RegistryEntry {
    pub id:   String,
    pub data: Option<NbtTag>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RegistryData {
    pub id:      String,
    pub entries: Vec<RegistryEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TagEntry {
    pub id:      String,
    pub entries: Vec<i32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TagRegistry {
    pub id:   String,
    pub tags: Vec<TagEntry>,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum RegistryPacketError {
    #[error("registry packet payload budget must leave room for the packet ID")]
    InvalidBudget,
    #[error("registry packet exceeds its payload budget")]
    TooLarge,
    #[error("registry packet identifier is invalid")]
    InvalidIdentifier,
    #[error("registry packet identifier exceeds the protocol length limit")]
    IdentifierTooLong,
    #[error("registry packet contains a duplicate identifier")]
    DuplicateIdentifier,
    #[error("registry tag contains a negative entry ID")]
    InvalidEntryId,
    #[error("registry packet collection exceeds the signed 32-bit length limit")]
    CollectionTooLong,
    #[error("invalid registry NBT: {0}")]
    Nbt(NbtEncodeError),
}

impl From<NbtEncodeError> for RegistryPacketError {
    fn from(error: NbtEncodeError) -> Self {
        match error {
            NbtEncodeError::InvalidBudget => Self::InvalidBudget,
            NbtEncodeError::TooLarge => Self::TooLarge,
            NbtEncodeError::CollectionTooLong => Self::CollectionTooLong,
            other => Self::Nbt(other),
        }
    }
}

pub fn encode_registry_data(
    registry: &RegistryData,
    max_payload_bytes: usize,
) -> Result<Vec<u8>, RegistryPacketError> {
    let mut writer = packet_writer(max_payload_bytes)?;
    writer.identifier(&registry.id)?;
    writer.packet_count(registry.entries.len())?;
    let mut names = HashSet::new();
    for entry in &registry.entries {
        let name = parse_identifier(&entry.id).ok_or(RegistryPacketError::InvalidIdentifier)?;
        if !names.insert(name) {
            return Err(RegistryPacketError::DuplicateIdentifier);
        }
        writer.identifier(&entry.id)?;
        match &entry.data {
            None => writer.byte(0)?,
            Some(data) => {
                writer.byte(1)?;
                writer.byte(data.kind())?;
                write_payload(&mut writer, data, 0)?;
            }
        }
    }
    Ok(writer.bytes)
}

pub fn encode_update_tags(
    registries: &[TagRegistry],
    max_payload_bytes: usize,
) -> Result<Vec<u8>, RegistryPacketError> {
    let mut writer = packet_writer(max_payload_bytes)?;
    writer.packet_count(registries.len())?;
    let mut registry_names = HashSet::new();
    for registry in registries {
        let name = parse_identifier(&registry.id).ok_or(RegistryPacketError::InvalidIdentifier)?;
        if !registry_names.insert(name) {
            return Err(RegistryPacketError::DuplicateIdentifier);
        }
        writer.identifier(&registry.id)?;
        writer.packet_count(registry.tags.len())?;
        let mut tag_names = HashSet::new();
        for tag in &registry.tags {
            let name = parse_identifier(&tag.id).ok_or(RegistryPacketError::InvalidIdentifier)?;
            if !tag_names.insert(name) {
                return Err(RegistryPacketError::DuplicateIdentifier);
            }
            writer.identifier(&tag.id)?;
            writer.packet_count(tag.entries.len())?;
            for entry in &tag.entries {
                if *entry < 0 {
                    return Err(RegistryPacketError::InvalidEntryId);
                }
                writer.varint(*entry)?;
            }
        }
    }
    Ok(writer.bytes)
}

fn packet_writer(max_payload_bytes: usize) -> Result<NbtWriter, RegistryPacketError> {
    if max_payload_bytes >= MAX_PACKET_LENGTH {
        return Err(RegistryPacketError::InvalidBudget);
    }
    Ok(NbtWriter::new(max_payload_bytes)?)
}

#[derive(Clone, Debug, PartialEq)]
pub enum NbtTag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(String),
    List(Vec<NbtTag>),
    Compound(Vec<(String, NbtTag)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum NbtEncodeError {
    #[error("NBT output budget exceeds the maximum packet length")]
    InvalidBudget,
    #[error("NBT exceeds its output budget")]
    TooLarge,
    #[error("NBT string exceeds the Java modified UTF-8 length limit")]
    StringTooLong,
    #[error("NBT collection exceeds the signed 32-bit length limit")]
    CollectionTooLong,
    #[error("NBT container nesting exceeds 512")]
    DepthExceeded,
    #[error("NBT compound contains duplicate field names")]
    DuplicateField,
}

pub fn encode_network_nbt(tag: &NbtTag, max_bytes: usize) -> Result<Vec<u8>, NbtEncodeError> {
    let mut writer = NbtWriter::new(max_bytes)?;
    writer.byte(tag.kind())?;
    write_payload(&mut writer, tag, 0)?;
    Ok(writer.bytes)
}

impl NbtTag {
    fn kind(&self) -> u8 {
        match self {
            Self::Byte(_) => 1,
            Self::Short(_) => 2,
            Self::Int(_) => 3,
            Self::Long(_) => 4,
            Self::Float(_) => 5,
            Self::Double(_) => 6,
            Self::ByteArray(_) => 7,
            Self::String(_) => 8,
            Self::List(_) => 9,
            Self::Compound(_) => 10,
            Self::IntArray(_) => 11,
            Self::LongArray(_) => 12,
        }
    }
}

struct NbtWriter {
    bytes:     Vec<u8>,
    max_bytes: usize,
}

impl NbtWriter {
    fn new(max_bytes: usize) -> Result<Self, NbtEncodeError> {
        if max_bytes > MAX_PACKET_LENGTH {
            return Err(NbtEncodeError::InvalidBudget);
        }
        Ok(Self {
            bytes: Vec::new(),
            max_bytes,
        })
    }

    fn reserve(&self, len: usize) -> Result<(), NbtEncodeError> {
        if len > self.max_bytes.saturating_sub(self.bytes.len()) {
            return Err(NbtEncodeError::TooLarge);
        }
        Ok(())
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), NbtEncodeError> {
        self.reserve(bytes.len())?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn byte(&mut self, value: u8) -> Result<(), NbtEncodeError> {
        self.write(&[value])
    }

    fn collection_len(&mut self, len: usize) -> Result<(), NbtEncodeError> {
        let len = i32::try_from(len).map_err(|_| NbtEncodeError::CollectionTooLong)?;
        self.write(&len.to_be_bytes())
    }

    fn varint(&mut self, value: i32) -> Result<(), NbtEncodeError> {
        let mut value = value as u32;
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            self.byte(if value == 0 { byte } else { byte | 0x80 })?;
            if value == 0 {
                return Ok(());
            }
        }
    }

    fn packet_count(&mut self, count: usize) -> Result<(), RegistryPacketError> {
        let count = i32::try_from(count).map_err(|_| RegistryPacketError::CollectionTooLong)?;
        self.varint(count)?;
        Ok(())
    }

    fn identifier(&mut self, value: &str) -> Result<(), RegistryPacketError> {
        let (namespace, path) = parse_identifier(value).ok_or(RegistryPacketError::InvalidIdentifier)?;
        let len = namespace.len() + 1 + path.len();
        if len > MAX_IDENTIFIER_LEN {
            return Err(RegistryPacketError::IdentifierTooLong);
        }
        self.varint(len as i32)?;
        self.write(namespace.as_bytes())?;
        self.byte(b':')?;
        self.write(path.as_bytes())?;
        Ok(())
    }

    fn modified_utf8(&mut self, value: &str) -> Result<(), NbtEncodeError> {
        let mut len = 0usize;
        for unit in value.encode_utf16() {
            len += match unit {
                1..=0x7f => 1,
                0..=0x7ff => 2,
                _ => 3,
            };
            if len > u16::MAX as usize {
                return Err(NbtEncodeError::StringTooLong);
            }
        }
        self.reserve(2 + len)?;
        self.bytes.extend_from_slice(&(len as u16).to_be_bytes());
        for unit in value.encode_utf16() {
            match unit {
                1..=0x7f => self.bytes.push(unit as u8),
                0..=0x7ff => {
                    self.bytes.push((0xc0 | (unit >> 6)) as u8);
                    self.bytes.push((0x80 | (unit & 0x3f)) as u8);
                }
                _ => {
                    self.bytes.push((0xe0 | (unit >> 12)) as u8);
                    self.bytes.push((0x80 | ((unit >> 6) & 0x3f)) as u8);
                    self.bytes.push((0x80 | (unit & 0x3f)) as u8);
                }
            }
        }
        Ok(())
    }
}

fn container_depth(depth: usize) -> Result<usize, NbtEncodeError> {
    if depth >= MAX_NBT_DEPTH {
        return Err(NbtEncodeError::DepthExceeded);
    }
    Ok(depth + 1)
}

fn write_payload(writer: &mut NbtWriter, tag: &NbtTag, depth: usize) -> Result<(), NbtEncodeError> {
    match tag {
        NbtTag::Byte(value) => writer.write(&value.to_be_bytes()),
        NbtTag::Short(value) => writer.write(&value.to_be_bytes()),
        NbtTag::Int(value) => writer.write(&value.to_be_bytes()),
        NbtTag::Long(value) => writer.write(&value.to_be_bytes()),
        NbtTag::Float(value) => {
            let bits = if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits()
            };
            writer.write(&bits.to_be_bytes())
        }
        NbtTag::Double(value) => {
            let bits = if value.is_nan() {
                0x7ff8_0000_0000_0000
            } else {
                value.to_bits()
            };
            writer.write(&bits.to_be_bytes())
        }
        NbtTag::ByteArray(values) => {
            writer.collection_len(values.len())?;
            writer.reserve(values.len())?;
            for value in values {
                writer.bytes.push(*value as u8);
            }
            Ok(())
        }
        NbtTag::String(value) => writer.modified_utf8(value),
        NbtTag::List(values) => {
            let depth = container_depth(depth)?;
            let element_kind = match values.first() {
                None => 0,
                Some(first) if values.iter().all(|value| value.kind() == first.kind()) => first.kind(),
                Some(_) => 10,
            };
            writer.byte(element_kind)?;
            writer.collection_len(values.len())?;
            for value in values {
                let needs_wrapper = element_kind == 10
                    && !matches!(value, NbtTag::Compound(fields) if fields.len() != 1 || !fields[0].0.is_empty());
                if needs_wrapper {
                    let wrapper_depth = container_depth(depth)?;
                    writer.byte(value.kind())?;
                    writer.modified_utf8("")?;
                    write_payload(writer, value, wrapper_depth)?;
                    writer.byte(0)?;
                } else {
                    write_payload(writer, value, depth)?;
                }
            }
            Ok(())
        }
        NbtTag::Compound(fields) => {
            let depth = container_depth(depth)?;
            let mut names = HashSet::new();
            for (name, value) in fields {
                if !names.insert(name.as_str()) {
                    return Err(NbtEncodeError::DuplicateField);
                }
                writer.byte(value.kind())?;
                writer.modified_utf8(name)?;
                write_payload(writer, value, depth)?;
            }
            writer.byte(0)
        }
        NbtTag::IntArray(values) => {
            writer.collection_len(values.len())?;
            for value in values {
                writer.write(&value.to_be_bytes())?;
            }
            Ok(())
        }
        NbtTag::LongArray(values) => {
            writer.collection_len(values.len())?;
            for value in values {
                writer.write(&value.to_be_bytes())?;
            }
            Ok(())
        }
    }
}
