use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WireError {
    #[error("truncated wire value")]
    Truncated,
    #[error("malformed VarInt")]
    InvalidVarInt,
    #[error("negative or oversized string length")]
    InvalidStringLength,
    #[error("invalid UTF-8 string")]
    InvalidUtf8,
    #[error("invalid boolean value {0}")]
    InvalidBool(u8),
}

pub(crate) fn partial_varint(bytes: &[u8]) -> Result<Option<(i32, usize)>, WireError> {
    let mut value = 0u32;
    for (index, &byte) in bytes.iter().take(5).enumerate() {
        if index == 4 && byte & 0xf0 != 0 {
            return Err(WireError::InvalidVarInt);
        }
        value |= u32::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            return Ok(Some((value as i32, index + 1)));
        }
    }
    if bytes.len() >= 5 {
        Err(WireError::InvalidVarInt)
    } else {
        Ok(None)
    }
}

pub struct Reader<'a> {
    bytes:  &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }

    pub fn raw(&mut self, length: usize) -> Result<&'a [u8], WireError> {
        if length > self.remaining() {
            return Err(WireError::Truncated);
        }
        let start = self.offset;
        self.offset += length;
        Ok(&self.bytes[start..self.offset])
    }

    pub fn varint(&mut self) -> Result<i32, WireError> {
        let (value, count) = partial_varint(&self.bytes[self.offset..])?.ok_or(WireError::Truncated)?;
        self.offset += count;
        Ok(value)
    }

    pub fn u8(&mut self) -> Result<u8, WireError> {
        Ok(self.raw(1)?[0])
    }

    pub fn i8(&mut self) -> Result<i8, WireError> {
        Ok(self.u8()? as i8)
    }

    pub fn i16(&mut self) -> Result<i16, WireError> {
        Ok(i16::from_be_bytes(self.raw(2)?.try_into().map_err(|_| WireError::Truncated)?))
    }

    pub fn i32(&mut self) -> Result<i32, WireError> {
        Ok(i32::from_be_bytes(self.raw(4)?.try_into().map_err(|_| WireError::Truncated)?))
    }

    pub fn i64(&mut self) -> Result<i64, WireError> {
        Ok(i64::from_be_bytes(self.raw(8)?.try_into().map_err(|_| WireError::Truncated)?))
    }

    pub fn f32(&mut self) -> Result<f32, WireError> {
        Ok(f32::from_be_bytes(self.raw(4)?.try_into().map_err(|_| WireError::Truncated)?))
    }

    pub fn f64(&mut self) -> Result<f64, WireError> {
        Ok(f64::from_be_bytes(self.raw(8)?.try_into().map_err(|_| WireError::Truncated)?))
    }

    pub fn bool(&mut self) -> Result<bool, WireError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(WireError::InvalidBool(other)),
        }
    }

    pub fn uuid(&mut self) -> Result<Uuid, WireError> {
        Ok(Uuid::from_bytes(self.raw(16)?.try_into().map_err(|_| WireError::Truncated)?))
    }

    pub fn string(&mut self, max_bytes: usize) -> Result<&'a str, WireError> {
        let length = self.varint()?;
        if length < 0 || length as usize > max_bytes {
            return Err(WireError::InvalidStringLength);
        }
        std::str::from_utf8(self.raw(length as usize)?).map_err(|_| WireError::InvalidUtf8)
    }

    pub fn identifier(&mut self, max_bytes: usize) -> Result<&'a str, WireError> {
        let value = self.string(max_bytes)?;
        if value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'/' | b'_' | b'-' | b'.'))
        {
            Ok(value)
        } else {
            Err(WireError::InvalidUtf8)
        }
    }

    pub fn position(&mut self) -> Result<(i32, i32, i32), WireError> {
        let packed = self.i64()?;
        Ok(((packed >> 38) as i32, (packed << 52 >> 52) as i32, (packed << 26 >> 38) as i32))
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{Reader, WireError};

    #[test]
    fn decodes_known_network_order_values() {
        let bytes = [
            &[0xac, 0x02, 0x12, 0x34, 0x12, 0x34, 0x56, 0x78][..],
            &[1, 2, 3, 4, 5, 6, 7, 8],
            &[0x3f, 0x80, 0, 0],
            &[0x3f, 0xf0, 0, 0, 0, 0, 0, 0],
            &[
                0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
            ],
            &[2, b'h', b'i'],
        ]
        .concat();
        let mut reader = Reader::new(&bytes);
        assert_eq!(reader.varint().unwrap(), 300);
        assert_eq!(reader.i16().unwrap(), 0x1234);
        assert_eq!(reader.i32().unwrap(), 0x1234_5678);
        assert_eq!(reader.i64().unwrap(), 0x0102_0304_0506_0708);
        assert_eq!(reader.f32().unwrap(), 1.0);
        assert_eq!(reader.f64().unwrap(), 1.0);
        assert_eq!(reader.uuid().unwrap(), Uuid::from_u128(0x0011_2233_4455_6677_8899_aabb_ccdd_eeff));
        assert_eq!(reader.string(16).unwrap(), "hi");
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn rejects_truncated_and_invalid_values() {
        assert_eq!(Reader::new(&[0x80]).varint(), Err(WireError::Truncated));
        assert_eq!(Reader::new(&[0xff, 0xff, 0xff, 0xff, 0x10]).varint(), Err(WireError::InvalidVarInt));
        assert_eq!(
            Reader::new(&[0xff, 0xff, 0xff, 0xff, 0x0f]).string(32),
            Err(WireError::InvalidStringLength)
        );
    }
}
