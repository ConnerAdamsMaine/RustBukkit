use std::net::SocketAddr;
use std::time::Duration;

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

impl ProtocolState {
    fn allows(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Handshake, Self::Status | Self::Login)
                | (Self::Login, Self::Configuration | Self::Play)
                | (Self::Configuration, Self::Play)
                | (
                    Self::Handshake | Self::Status | Self::Login | Self::Configuration | Self::Play,
                    Self::Closed
                )
        )
    }
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
    #[error("invalid protocol state transition from {from:?} to {to:?}")]
    InvalidTransition {
        from: ProtocolState,
        to:   ProtocolState,
    },
    #[error("socket write timed out")]
    WriteTimeout,
}

pub struct Connection {
    stream:        TcpStream,
    decoder:       FrameDecoder,
    state:         ProtocolState,
    peer_addr:     SocketAddr,
    player_name:   Option<String>,
    write_timeout: Duration,
}

impl Connection {
    pub fn new(stream: TcpStream, peer_addr: SocketAddr) -> Self {
        debug!("[STATE] Handshake peer={peer_addr}");
        Self {
            stream,
            decoder: FrameDecoder::new(),
            state: ProtocolState::Handshake,
            peer_addr,
            player_name: None,
            write_timeout: Duration::from_secs(30),
        }
    }

    pub fn state(&self) -> ProtocolState {
        self.state
    }

    pub fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }

    pub fn set_player_name(&mut self, player_name: String) {
        self.player_name = Some(player_name);
    }

    pub fn player_label(&self) -> &str {
        self.player_name.as_deref().unwrap_or("-")
    }

    pub fn set_write_timeout(&mut self, timeout: Duration) {
        self.write_timeout = timeout;
    }

    pub fn set_state(&mut self, state: ProtocolState) -> Result<(), ConnectionError> {
        if !self.state.allows(state) {
            return Err(ConnectionError::InvalidTransition {
                from: self.state,
                to:   state,
            });
        }
        self.state = state;
        debug!("[STATE] {:?} peer={}", state, self.peer_addr);
        Ok(())
    }

    pub async fn read_frame(&mut self) -> Result<Option<Frame>, ConnectionError> {
        loop {
            if let Some(frame) = self.decoder.next_frame()? {
                debug!(
                    "[RX] player={} state={:?} id=0x{:02x} length={} peer={}",
                    self.player_label(),
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
            "[TX] player={} state={:?} id=0x{:02x} length={} peer={}",
            self.player_label(),
            self.state,
            id,
            payload.len(),
            self.peer_addr
        );
        trace!("[TX bytes] {:02x?}", frame);
        tokio::time::timeout(self.write_timeout, self.stream.write_all(&frame))
            .await
            .map_err(|_| ConnectionError::WriteTimeout)??;
        Ok(())
    }

    pub async fn shutdown(&mut self) -> Result<(), ConnectionError> {
        self.set_state(ProtocolState::Closed)?;
        self.stream.shutdown().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ProtocolState;

    #[test]
    fn accepts_only_valid_1_19_2_state_paths() {
        assert!(ProtocolState::Handshake.allows(ProtocolState::Status));
        assert!(ProtocolState::Handshake.allows(ProtocolState::Login));
        assert!(ProtocolState::Login.allows(ProtocolState::Play));
        assert!(!ProtocolState::Handshake.allows(ProtocolState::Play));
        assert!(!ProtocolState::Status.allows(ProtocolState::Play));
        assert!(!ProtocolState::Play.allows(ProtocolState::Login));
        assert!(!ProtocolState::Closed.allows(ProtocolState::Play));
    }
}
