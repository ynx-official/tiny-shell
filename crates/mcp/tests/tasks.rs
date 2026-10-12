use tiny_shell_mcp::tasks::State;
#[test]
fn terminal_tasks_never_reenter_execution() {
    for state in [
        State::Succeeded,
        State::Failed,
        State::Cancelled,
        State::TimedOut,
        State::Interrupted,
        State::OutcomeUnknown,
    ] {
        assert!(state.terminal());
        assert!(!state.permits(State::Running));
        assert!(!state.permits(State::Queued));
    }
}
#[test]
fn cancelled_approval_cannot_become_running() {
    assert!(State::AwaitingUser.permits(State::Cancelled));
    assert!(!State::Cancelled.permits(State::Running));
    assert!(!State::AwaitingUser.permits(State::Succeeded));
}
