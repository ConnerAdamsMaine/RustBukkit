use bytes::{Buf, Bytes, BytesMut};
use thiserror::Error;

use crate::wire::{WireError, partial_varint};

pub const MAX_PACKET_LENGTH: usize = 2 * 1024 * 1024;
const MAX_RECEIVE_BUFFER: usize = 2 * MAX_PACKET_LENGTH + 10;

#[derive(Debug, Error)]
pub enum FrameError {
    #[error(transparent)]
    Wire(#[from] WireError),
    #[error("invalid packet length {0}")]
    InvalidLength(i32),
    #[error("invalid packet ID {0}")]
    InvalidId(i32),
    #[error("receive buffer exceeds packet limit")]
    BufferTooLarge,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Frame {
    pub id:      i32,
    pub payload: Bytes,
}

#[derive(Default)]
pub struct FrameDecoder {
    buffer: BytesMut,
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Result<(), FrameError> {
        if bytes.len() > MAX_RECEIVE_BUFFER.saturating_sub(self.buffer.len()) {
            return Err(FrameError::BufferTooLarge);
        }
        self.buffer.extend_from_slice(bytes);
        Ok(())
    }

    pub fn next_frame(&mut self) -> Result<Option<Frame>, FrameError> {
        let Some((length, prefix_len)) = partial_varint(&self.buffer)? else {
            return Ok(None);
        };
        if length <= 0 || length as usize > MAX_PACKET_LENGTH {
            return Err(FrameError::InvalidLength(length));
        }
        let length = length as usize;
        if self.buffer.len() < prefix_len + length {
            return Ok(None);
        }
        let mut packet = self.buffer.split_to(prefix_len + length);
        packet.advance(prefix_len);
        let (id, id_len) = partial_varint(&packet)?.ok_or(WireError::Truncated)?;
        if id < 0 {
            return Err(FrameError::InvalidId(id));
        }
        packet.advance(id_len);
        Ok(Some(Frame {
            id,
            payload: packet.freeze(),
        }))
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{FrameDecoder, FrameError};

    fn decode_in_chunks(input: &[u8], width: usize) -> Vec<(i32, Vec<u8>)> {
        let mut decoder = FrameDecoder::new();
        let mut frames = Vec::new();
        for chunk in input.chunks(width) {
            decoder.feed(chunk).unwrap();
            while let Some(frame) = decoder.next_frame().unwrap() {
                frames.push((frame.id, frame.payload.to_vec()));
            }
        }
        assert!(decoder.is_empty());
        frames
    }

    #[test]
    fn fragmentation_and_coalescing_decode_identically() {
        let mut input = vec![0x82, 0x01, 0x02];
        input.extend(std::iter::repeat_n(0xab, 129));
        let one = (2, vec![0xab; 129]);
        for width in [1, 2, input.len()] {
            assert_eq!(decode_in_chunks(&input, width), vec![one.clone()]);
        }
        let mut combined = input.clone();
        combined.extend_from_slice(&[2, 1, 0xcc]);
        combined.extend_from_slice(&[1, 3]);
        for width in [1, 2, combined.len()] {
            assert_eq!(decode_in_chunks(&combined, width), vec![one.clone(), (1, vec![0xcc]), (3, vec![])]);
        }
    }

    #[test]
    fn rejects_invalid_lengths_before_allocating() {
        let mut decoder = FrameDecoder::new();
        decoder.feed(&[0]).unwrap();
        assert!(matches!(decoder.next_frame(), Err(FrameError::InvalidLength(0))));
        let mut decoder = FrameDecoder::new();
        decoder.feed(&[0xff, 0xff, 0xff, 0xff, 0x0f]).unwrap();
        assert!(matches!(decoder.next_frame(), Err(FrameError::InvalidLength(-1))));
    }
}
