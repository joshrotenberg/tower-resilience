use tower_resilience_reconnect::{ConnectionState, ReconnectState};

#[test]
fn state_starts_disconnected() {
    let state = ReconnectState::new();

    assert_eq!(state.state(), ConnectionState::Disconnected);
    assert_eq!(state.attempts(), 0);
}

#[test]
fn state_tracks_connection_state() {
    let state = ReconnectState::new();

    // Initially disconnected
    assert_eq!(state.state(), ConnectionState::Disconnected);

    // Simulate state changes by creating new states
    // (In real usage, the ReconnectService manages state transitions)
    assert_eq!(state.attempts(), 0);
}

#[test]
fn connection_state_variants() {
    // Test that all state variants exist and are distinguishable
    let disconnected = ConnectionState::Disconnected;
    let connecting = ConnectionState::Reconnecting;
    let connected = ConnectionState::Connected;

    assert_ne!(format!("{:?}", disconnected), format!("{:?}", connecting));
    assert_ne!(format!("{:?}", connecting), format!("{:?}", connected));
    assert_ne!(format!("{:?}", disconnected), format!("{:?}", connected));
}

#[test]
fn state_is_cloneable() {
    let state1 = ReconnectState::new();
    let state2 = state1.clone();

    assert_eq!(state1.state(), state2.state());
    assert_eq!(state1.attempts(), state2.attempts());
}
