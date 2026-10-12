use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    ReadOnly,
    Controlled,
    Full,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Approve,
    Deny,
}
pub fn decide(enabled: bool, authorized: bool, mode: Mode, mutation: bool) -> Decision {
    if !enabled || !authorized {
        return Decision::Deny;
    }
    match (mode, mutation) {
        (Mode::ReadOnly, true) => Decision::Deny,
        (Mode::Controlled, true) => Decision::Approve,
        _ => Decision::Allow,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    #[default]
    Specified,
    All,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub schema_version: u32,
    pub enabled: bool,
    pub mode: Mode,
    pub scope: Scope,
    pub grants: BTreeMap<String, String>,
    pub known_targets: BTreeMap<String, String>,
    pub revision: u64,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            schema_version: 1,
            enabled: false,
            mode: Mode::ReadOnly,
            scope: Scope::Specified,
            grants: BTreeMap::new(),
            known_targets: BTreeMap::new(),
            revision: 0,
        }
    }
}
impl Policy {
    pub fn authorized(&self, id: &str, digest: &str) -> bool {
        let targets = match self.scope {
            Scope::Specified => &self.grants,
            Scope::All => &self.known_targets,
        };
        targets.get(id).is_some_and(|stored| stored == digest)
    }
    // Existing IDs never inherit an authorization after their target changes.
    pub fn observe(&mut self, targets: impl IntoIterator<Item = (String, String)>) {
        for (id, digest) in targets {
            self.known_targets.entry(id).or_insert(digest);
        }
    }
    pub fn grant(&mut self, id: String, digest: String) {
        self.known_targets.insert(id.clone(), digest.clone());
        self.grants.insert(id, digest);
        self.revision = self.revision.saturating_add(1);
    }
}
