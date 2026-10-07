use autopilot::tui::pty::PtySession;

#[test]
fn test_pty_session_initial_state() {
    let session = PtySession::new(24, 80);
    assert_eq!(session.current_rows, 24);
    assert_eq!(session.current_cols, 80);
    assert!(session.active_task_id.is_none());
}

#[test]
fn test_pty_resize_deduplication() {
    let mut session = PtySession::new(24, 80);
    // Identical resize should be a no-op
    session.resize(24, 80);
    assert_eq!(session.current_rows, 24);
    assert_eq!(session.current_cols, 80);

    // Changed dimensions should update
    session.resize(30, 100);
    assert_eq!(session.current_rows, 30);
    assert_eq!(session.current_cols, 100);
}
