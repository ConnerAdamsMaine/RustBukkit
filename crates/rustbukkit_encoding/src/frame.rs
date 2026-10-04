use thiserror::Error;

use crate::Writer;

pub const MAX_PACKET_LENGTH: usize = 2 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum FrameEncodeError {
    #[error("packet length {0} exceeds the {MAX_PACKET_LENGTH} byte limit")]
    TooLarge(usize),
    #[error("packet ID must be nonnegative")]
    InvalidId,
}

pub fn encode_frame(id: i32, payload: &[u8]) -> Result<Vec<u8>, FrameEncodeError> {
    if id < 0 {
        return Err(FrameEncodeError::InvalidId);
    }
    let mut id_writer = Writer::new();
    id_writer.varint(id);
    let id_bytes = id_writer.finish();
    let length = id_bytes.len() + payload.len();
    if length > MAX_PACKET_LENGTH {
        return Err(FrameEncodeError::TooLarge(length));
    }
    let mut frame = Writer::with_capacity(5 + length);
    frame.varint(length as i32);
    frame.raw(&id_bytes);
    frame.raw(payload);
    Ok(frame.finish())
}

#[cfg(test)]
mod tests {
    use super::encode_frame;

    #[test]
    fn frame_length_includes_id_and_payload() {
        assert_eq!(encode_frame(0x80, &[0xaa, 0xbb]).unwrap(), [4, 0x80, 1, 0xaa, 0xbb]);
    }
}
