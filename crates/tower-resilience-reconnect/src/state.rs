//! Connection state tracking for reconnection logic.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Connection state information
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Connected and healthy
    Connected,

    /// Disconnected, waiting to reconnect
    Disconnected,

    /// Currently attempting to reconnect
    Reconnecting,
}

/// Shared reconnection state tracking
#[derive(Clone)]
pub struct ReconnectState {
    /// Current connection state
    state: Arc<AtomicU64>,

    /// Current reconnection attempt number (0-indexed)
    attempts: Arc<AtomicU32>,

    /// Last successful connection time (unix timestamp millis)
    last_connected: Arc<AtomicU64>,
}

impl ReconnectState {
    /// Create a new reconnect state
    pub fn new() -> Self {
        Self {
            state: Arc::new(AtomicU64::new(Self::encode_state(
                ConnectionState::Disconnected,
            ))),
            attempts: Arc::new(AtomicU32::new(0)),
            last_connected: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Get the current connection state
    pub fn state(&self) -> ConnectionState {
        Self::decode_state(self.state.load(Ordering::Acquire))
    }

    /// Set the connection state
    pub fn set_state(&self, state: ConnectionState) {
        self.state
            .store(Self::encode_state(state), Ordering::Release);
    }

    /// Get the current attempt number
    pub fn attempts(&self) -> u32 {
        self.attempts.load(Ordering::Acquire)
    }

    /// Increment and return the attempt number
    pub fn increment_attempts(&self) -> u32 {
        self.attempts.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// Reset attempts to zero
    pub fn reset_attempts(&self) {
        self.attempts.store(0, Ordering::Release);
    }

    /// Mark connection as successful
    pub fn mark_connected(&self) {
        self.set_state(ConnectionState::Connected);
        self.reset_attempts();
        let now = Instant::now();
        let millis = now.elapsed().as_millis() as u64;
        self.last_connected.store(millis, Ordering::Release);
    }

    /// Mark connection as disconnected
    pub fn mark_disconnected(&self) {
        self.set_state(ConnectionState::Disconnected);
    }

    /// Mark connection as reconnecting
    pub fn mark_reconnecting(&self) {
        self.set_state(ConnectionState::Reconnecting);
    }

    /// Get time since last successful connection
    pub fn time_since_connected(&self) -> Option<Duration> {
        let last = self.last_connected.load(Ordering::Acquire);
        if last == 0 {
            None
        } else {
            let now = Instant::now().elapsed().as_millis() as u64;
            Some(Duration::from_millis(now.saturating_sub(last)))
        }
    }

    fn encode_state(state: ConnectionState) -> u64 {
        match state {
            ConnectionState::Connected => 0,
            ConnectionState::Disconnected => 1,
            ConnectionState::Reconnecting => 2,
        }
    }

    fn decode_state(encoded: u64) -> ConnectionState {
        match encoded {
            0 => ConnectionState::Connected,
            1 => ConnectionState::Disconnected,
            _ => ConnectionState::Reconnecting,
        }
    }
}

impl Default for ReconnectState {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ReconnectState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReconnectState")
            .field("state", &self.state())
            .field("attempts", &self.attempts())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let state = ReconnectState::new();
        assert_eq!(state.state(), ConnectionState::Disconnected);
        assert_eq!(state.attempts(), 0);
    }

    #[test]
    fn test_state_transitions() {
        let state = ReconnectState::new();

        state.mark_reconnecting();
        assert_eq!(state.state(), ConnectionState::Reconnecting);

        state.mark_connected();
        assert_eq!(state.state(), ConnectionState::Connected);
        assert_eq!(state.attempts(), 0);

        state.mark_disconnected();
        assert_eq!(state.state(), ConnectionState::Disconnected);
    }

    #[test]
    fn test_attempts_tracking() {
        let state = ReconnectState::new();

        assert_eq!(state.increment_attempts(), 1);
        assert_eq!(state.increment_attempts(), 2);
        assert_eq!(state.increment_attempts(), 3);
        assert_eq!(state.attempts(), 3);

        state.reset_attempts();
        assert_eq!(state.attempts(), 0);
    }

    #[test]
    fn test_mark_connected_resets_attempts() {
        let state = ReconnectState::new();

        state.increment_attempts();
        state.increment_attempts();
        assert_eq!(state.attempts(), 2);

        state.mark_connected();
        assert_eq!(state.attempts(), 0);
        assert_eq!(state.state(), ConnectionState::Connected);
    }
}
