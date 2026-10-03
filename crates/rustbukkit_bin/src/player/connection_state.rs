#![allow(dead_code)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;

/// Represents the current stage of a player's connection lifecycle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionStage {
    /// Initial TCP connection, waiting for handshake packet
    Connected,
    /// Handshake packet received, awaiting login packets
    Handshaking,
    /// Login sequence in progress (authentication, encryption negotiation)
    Authenticating,
    /// Configuration phase (client settings, feature flags)
    Configuring,
    /// Player fully in game world
    InGame,
    /// Gracefully disconnecting
    Disconnecting,
    /// Connection terminated
    Disconnected,
}

impl std::fmt::Display for ConnectionStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connected => write!(f, "Connected"),
            Self::Handshaking => write!(f, "Handshaking"),
            Self::Authenticating => write!(f, "Authenticating"),
            Self::Configuring => write!(f, "Configuring"),
            Self::InGame => write!(f, "InGame"),
            Self::Disconnecting => write!(f, "Disconnecting"),
            Self::Disconnected => write!(f, "Disconnected"),
        }
    }
}

/// Tracks the connection state with timestamps and state change history
pub struct ConnectionStateTracker {
    current_stage:    RwLock<ConnectionStage>,
    /// Unix timestamp (ms) when connection was established
    connected_at:     u64,
    /// Unix timestamp (ms) when current stage was entered
    stage_started_at: AtomicU64,
}

impl ConnectionStateTracker {
    pub fn new() -> Self {
        let now = current_timestamp_ms();
        Self {
            current_stage:    RwLock::new(ConnectionStage::Connected),
            connected_at:     now,
            stage_started_at: AtomicU64::new(now),
        }
    }

    /// Get the current connection stage
    pub fn current_stage(&self) -> ConnectionStage {
        *self.current_stage.read()
    }

    /// Transition to a new connection stage
    pub fn transition(&self, new_stage: ConnectionStage) {
        let old_stage = self.current_stage();
        *self.current_stage.write() = new_stage;
        self.stage_started_at
            .store(current_timestamp_ms(), Ordering::Release);

        tracing::info!("[CONNECTION] State transition: {} -> {}", old_stage, new_stage);
    }

    /// Get time spent in current stage (ms)
    pub fn stage_duration_ms(&self) -> u64 {
        let started = self.stage_started_at.load(Ordering::Acquire);
        current_timestamp_ms().saturating_sub(started)
    }

    /// Get total connection duration (ms)
    pub fn connection_duration_ms(&self) -> u64 {
        current_timestamp_ms().saturating_sub(self.connected_at)
    }

    /// Check if connection is alive
    pub fn is_connected(&self) -> bool {
        matches!(
            self.current_stage(),
            ConnectionStage::Connected
                | ConnectionStage::Handshaking
                | ConnectionStage::Authenticating
                | ConnectionStage::Configuring
                | ConnectionStage::InGame
        )
    }

    /// Get detailed state info
    pub fn state_info(&self) -> StateInfo {
        let stage = self.current_stage();
        StateInfo {
            stage,
            stage_duration_ms: self.stage_duration_ms(),
            total_duration_ms: self.connection_duration_ms(),
        }
    }
}

impl Default for ConnectionStateTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot of connection state information
#[derive(Debug, Clone)]
pub struct StateInfo {
    pub stage:             ConnectionStage,
    pub stage_duration_ms: u64,
    pub total_duration_ms: u64,
}

impl std::fmt::Display for StateInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{{ stage: {}, stage_duration: {}ms, total_duration: {}ms }}",
            self.stage, self.stage_duration_ms, self.total_duration_ms
        )
    }
}

/// Get current Unix timestamp in milliseconds
fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_stage_transitions() {
        let tracker = ConnectionStateTracker::new();

        assert_eq!(tracker.current_stage(), ConnectionStage::Connected);
        assert!(tracker.is_connected());

        tracker.transition(ConnectionStage::Handshaking);
        assert_eq!(tracker.current_stage(), ConnectionStage::Handshaking);

        tracker.transition(ConnectionStage::Disconnected);
        assert!(!tracker.is_connected());
    }

    #[test]
    fn test_duration_tracking() {
        let tracker = ConnectionStateTracker::new();
        std::thread::sleep(std::time::Duration::from_millis(10));

        let duration = tracker.connection_duration_ms();
        assert!(duration >= 10);
    }
}
