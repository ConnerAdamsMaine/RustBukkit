use thiserror::Error;

use crate::Writer;

pub const MAX_PACKET_LENGTH: usize = (1 << 21) - 1;

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
    let mut frame = Writer::with_capacity(3 + length);
    frame.varint(length as i32);
    frame.raw(&id_bytes);
    frame.raw(payload);
    Ok(frame.finish())
}

#[cfg(test)]
mod tests {
    use super::{FrameEncodeError, encode_frame};

    #[test]
    fn frame_length_includes_id_and_payload() {
        assert_eq!(encode_frame(0x80, &[0xaa, 0xbb]).unwrap(), [4, 0x80, 1, 0xaa, 0xbb]);
    }

    #[test]
    fn encodes_the_largest_three_byte_frame_and_rejects_the_next_length() {
        let mut payload = vec![0xab; 2_097_150];
        let frame = encode_frame(1, &payload).unwrap();
        assert_eq!(&frame[..4], &[0xff, 0xff, 0x7f, 0x01]);
        assert_eq!(frame.len(), 3 + 2_097_151);

        payload.push(0xab);
        assert!(matches!(encode_frame(1, &payload), Err(FrameEncodeError::TooLarge(2_097_152))));
    }
}
