use tiny_shell_mcp::policy::{Decision, Mode, decide};

#[test]
fn disabled_or_unlisted_always_denies() {
    for mode in [Mode::ReadOnly, Mode::Controlled, Mode::Full] {
        for mutation in [false, true] {
            assert_eq!(decide(false, true, mode, mutation), Decision::Deny);
            assert_eq!(decide(true, false, mode, mutation), Decision::Deny);
        }
    }
}

#[test]
fn permission_modes_have_distinct_mutation_behavior() {
    assert_eq!(decide(true, true, Mode::ReadOnly, true), Decision::Deny);
    assert_eq!(
        decide(true, true, Mode::Controlled, true),
        Decision::Approve
    );
    assert_eq!(decide(true, true, Mode::Full, true), Decision::Allow);
    for mode in [Mode::ReadOnly, Mode::Controlled, Mode::Full] {
        assert_eq!(decide(true, true, mode, false), Decision::Allow);
    }
}
