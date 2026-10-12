use tiny_shell_mcp::{
    engine::Target,
    policy::{Policy, Scope},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GrantState {
    Authorized,
    Unassigned,
    Changed,
}

pub(super) fn grant_state(policy: &Policy, target: &Target) -> GrantState {
    if policy.authorized(&target.id, &target.digest) {
        return GrantState::Authorized;
    }
    let stored = match policy.scope {
        Scope::Specified => &policy.grants,
        Scope::All => &policy.known_targets,
    };
    if stored.contains_key(&target.id) {
        GrantState::Changed
    } else {
        GrantState::Unassigned
    }
}

pub(super) fn matches_query(target: &Target, query: &str) -> bool {
    let searchable = format!(
        "{} {} {} {} {}",
        target.name,
        target.group.as_deref().unwrap_or(""),
        target.host,
        target.user,
        target.port
    )
    .to_lowercase();
    query
        .split_whitespace()
        .all(|term| searchable.contains(&term.to_lowercase()))
}

pub(super) fn specify(policy: &mut Policy, targets: &[Target]) {
    if policy.scope == Scope::All {
        // Materialize the current effective grants before leaving dynamic scope. Never
        // carry deleted targets or authorize a changed digest as a side effect.
        policy.grants = targets
            .iter()
            .filter(|t| policy.authorized(&t.id, &t.digest))
            .map(|t| (t.id.clone(), t.digest.clone()))
            .collect();
    }
    policy.scope = Scope::Specified;
}

pub(super) fn select_targets(
    policy: &mut Policy,
    targets: &[Target],
    selected: &[Target],
    enabled: bool,
) {
    specify(policy, targets);
    for target in selected {
        if enabled {
            if !policy.authorized(&target.id, &target.digest) {
                policy.grant(target.id.clone(), target.digest.clone());
            }
        } else {
            policy.grants.remove(&target.id);
            policy.known_targets.remove(&target.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(id: &str, name: &str) -> Target {
        Target {
            id: id.into(),
            name: name.into(),
            group: Some("Development".into()),
            host: "192.0.2.10".into(),
            user: "Dev".into(),
            port: 2222,
            digest: format!("digest-{id}"),
        }
    }

    fn all_policy(targets: &[Target]) -> Policy {
        let mut policy = Policy {
            enabled: true,
            scope: Scope::All,
            ..Policy::default()
        };
        policy.observe(targets.iter().map(|t| (t.id.clone(), t.digest.clone())));
        policy
    }

    #[test]
    fn removing_one_connection_from_all_preserves_other_current_grants() {
        let targets = vec![target("a", "A"), target("b", "B"), target("c", "C")];
        let mut policy = all_policy(&targets);
        select_targets(&mut policy, &targets, &targets[1..2], false);
        assert_eq!(policy.scope, Scope::Specified);
        assert!(policy.authorized("a", "digest-a"));
        assert!(!policy.authorized("b", "digest-b"));
        assert!(policy.authorized("c", "digest-c"));
        assert!(!policy.authorized("future", "digest-future"));
    }

    #[test]
    fn bulk_selection_affects_only_visible_results_and_never_enables_future_targets() {
        let targets = vec![target("a", "A"), target("b", "B")];
        let mut policy = Policy::default();
        policy.grant("b".into(), "digest-b".into());
        select_targets(&mut policy, &targets, &targets[..1], true);
        assert!(policy.authorized("a", "digest-a"));
        assert!(policy.authorized("b", "digest-b"));
        assert_eq!(policy.scope, Scope::Specified);
        policy.observe([(String::from("future"), String::from("digest-future"))]);
        assert!(!policy.authorized("future", "digest-future"));
    }

    #[test]
    fn switching_to_specified_does_not_reauthorize_changed_or_deleted_targets() {
        let original = target("a", "A");
        let mut changed = original.clone();
        changed.digest = "changed".into();
        let mut policy = all_policy(&[original, target("deleted", "Deleted"), target("b", "B")]);
        let current = vec![changed, target("b", "B")];
        specify(&mut policy, &current);
        assert!(!policy.authorized("a", "changed"));
        assert!(!policy.grants.contains_key("deleted"));
        assert!(policy.authorized("b", "digest-b"));
    }

    #[test]
    fn target_changes_have_a_distinct_reauthorization_state() {
        let mut current = target("a", "A");
        let mut policy = Policy::default();
        assert_eq!(grant_state(&policy, &current), GrantState::Unassigned);
        policy.grant(current.id.clone(), current.digest.clone());
        assert_eq!(grant_state(&policy, &current), GrantState::Authorized);
        current.digest = "new-target".into();
        assert_eq!(grant_state(&policy, &current), GrantState::Changed);
        policy.scope = Scope::All;
        assert_eq!(grant_state(&policy, &current), GrantState::Changed);
    }

    #[test]
    fn selecting_an_already_authorized_result_keeps_the_policy_revision() {
        let targets = vec![target("a", "A")];
        let mut policy = Policy::default();
        policy.grant("a".into(), "digest-a".into());
        let before = policy.revision;
        select_targets(&mut policy, &targets, &targets, true);
        assert_eq!(
            policy.revision, before,
            "no-op selection must not invalidate active tasks"
        );
    }

    #[test]
    fn search_combines_terms_across_name_group_host_user_and_port() {
        let current = target("a", "测试服务器");
        assert!(matches_query(&current, "  测试 DEV 2222 "));
        assert!(matches_query(&current, "development 192.0.2"));
        assert!(matches_query(&current, ""));
        assert!(!matches_query(&current, "测试 203.0.113"));
        assert!(!matches_query(&current, "production"));
    }
}
