use std::net::SocketAddr;

use rustbukkit_decoding::{Frame, FrameDecoder, FrameError};
use rustbukkit_encoding::{FrameEncodeError, encode_frame};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, trace};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolState {
    Handshake,
    Status,
    Login,
    Configuration,
    Play,
    Closed,
}

#[derive(Debug, Error)]
pub enum ConnectionError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Frame(#[from] FrameError),
    #[error("outgoing packet is invalid: {0}")]
    Encode(#[from] FrameEncodeError),
    #[error("connection closed with a partial frame")]
    Truncated,
}

pub struct Connection {
    stream:    TcpStream,
    decoder:   FrameDecoder,
    state:     ProtocolState,
    peer_addr: SocketAddr,
}

impl Connection {
    pub fn new(stream: TcpStream, peer_addr: SocketAddr) -> Self {
        debug!("[STATE] Handshake peer={peer_addr}");
        Self {
            stream,
            decoder: FrameDecoder::new(),
            state: ProtocolState::Handshake,
            peer_addr,
        }
    }

    pub fn state(&self) -> ProtocolState {
        self.state
    }

    pub fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }

    pub fn set_state(&mut self, state: ProtocolState) {
        self.state = state;
        debug!("[STATE] {:?} peer={}", state, self.peer_addr);
    }

    pub async fn read_frame(&mut self) -> Result<Option<Frame>, ConnectionError> {
        loop {
            if let Some(frame) = self.decoder.next_frame()? {
                debug!(
                    "[RX] state={:?} id=0x{:02x} length={} peer={}",
                    self.state,
                    frame.id,
                    frame.payload.len(),
                    self.peer_addr
                );
                trace!("[RX bytes] {:02x?}", frame.payload);
                return Ok(Some(frame));
            }
            let mut read_buf = [0u8; 8192];
            let count = self.stream.read(&mut read_buf).await?;
            if count == 0 {
                return if self.decoder.is_empty() {
                    Ok(None)
                } else {
                    Err(ConnectionError::Truncated)
                };
            }
            self.decoder.feed(&read_buf[..count])?;
        }
    }

    pub async fn write_frame(&mut self, id: i32, payload: &[u8]) -> Result<(), ConnectionError> {
        let frame = encode_frame(id, payload)?;
        debug!(
            "[TX] state={:?} id=0x{:02x} length={} peer={}",
            self.state,
            id,
            payload.len(),
            self.peer_addr
        );
        trace!("[TX bytes] {:02x?}", frame);
        self.stream.write_all(&frame).await?;
        Ok(())
    }

    pub async fn shutdown(&mut self) -> Result<(), ConnectionError> {
        self.set_state(ProtocolState::Closed);
        self.stream.shutdown().await?;
        Ok(())
    }
}
