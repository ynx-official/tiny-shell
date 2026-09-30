use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Grow-only counters with one independent writer per workspace window.
/// Taking component-wise maxima makes repeated DAV/S3 merges idempotent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CommandUsage(BTreeMap<String, u64>);

impl CommandUsage {
    pub(crate) fn record(&mut self, writer: &str) {
        let count = self.0.entry(writer.to_owned()).or_default();
        *count = count.saturating_add(1);
    }

    pub(crate) fn total(&self) -> u64 {
        self.0.values().copied().fold(0, u64::saturating_add)
    }

    pub(crate) fn merge(&mut self, other: &Self) {
        for (writer, count) in &other.0 {
            let current = self.0.entry(writer.clone()).or_default();
            *current = (*current).max(*count);
        }
    }
}

pub(crate) fn merge_category_usage(
    target: &mut [super::config::QuickCommandCategory],
    source: &[super::config::QuickCommandCategory],
) {
    let source: BTreeMap<_, _> = source
        .iter()
        .flat_map(|category| &category.commands)
        .map(|command| (command.id.as_str(), &command.usage))
        .collect();
    for command in target
        .iter_mut()
        .flat_map(|category| &mut category.commands)
    {
        if let Some(usage) = source.get(command.id.as_str()) {
            command.usage.merge(usage);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_merge_is_commutative_idempotent_and_survives_serialization() {
        let mut left = CommandUsage::default();
        left.record("a");
        let mut right = left.clone();
        right.record("b");
        right.record("b");
        left.record("a");
        let mut reverse = right.clone();
        reverse.merge(&left);
        left.merge(&right);
        assert_eq!(left, reverse);
        assert_eq!(left.total(), 4);
        left.merge(&right);
        assert_eq!(left.total(), 4);
        let restored: CommandUsage =
            serde_json::from_slice(&serde_json::to_vec(&left).unwrap()).unwrap();
        assert_eq!(restored, left);
    }

    #[test]
    fn counters_and_totals_saturate_instead_of_wrapping() {
        let mut usage = CommandUsage(BTreeMap::from([("a".into(), u64::MAX)]));
        usage.record("a");
        usage.record("b");
        assert_eq!(usage.total(), u64::MAX);
    }

    #[test]
    fn legacy_commands_without_usage_start_at_zero() {
        let command: crate::session::config::QuickCommand =
            serde_json::from_str(r#"{"id":"a","name":"List","command":"ls"}"#).unwrap();
        assert_eq!(command.usage.total(), 0);
    }
}
