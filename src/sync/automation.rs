use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tiny_shell_mcp::{
    engine::AiPolicy,
    policy::{Mode, Policy, Scope},
};

use crate::{ai::Settings, sync::model::SyncSecret};
#[cfg(test)]
mod tests;

#[derive(Clone, Default)]
pub struct AutomationConfig {
    pub(crate) expected_ai_stamp: Option<u64>,
    pub(crate) expected_mcp_stamp: Option<u64>,
    pub(crate) ai: Option<AiConfiguration>,
    pub(crate) mcp: Option<McpConfiguration>,
}

/// Plaintext keys exist only in memory during password-authorized reconciliation.
/// Deliberately has no Debug implementation.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AiConfiguration {
    pub settings: Settings,
    pub keys: BTreeMap<String, String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct SyncAiConfiguration {
    pub settings: Settings,
    pub keys: BTreeMap<String, SyncSecret>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct McpConfiguration {
    pub enabled: bool,
    pub mode: Mode,
    pub scope: Scope,
    pub grants: BTreeMap<String, String>,
    pub known_targets: BTreeMap<String, String>,
    pub ai_enabled: bool,
    pub ai_mode: Mode,
}

impl McpConfiguration {
    pub fn capture(policy: Policy, ai: AiPolicy) -> Self {
        Self {
            enabled: policy.enabled,
            mode: policy.mode,
            scope: policy.scope,
            grants: policy.grants,
            known_targets: policy.known_targets,
            ai_enabled: ai.enabled,
            ai_mode: ai.mode,
        }
    }
    pub fn policy(&self) -> Policy {
        Policy {
            enabled: self.enabled,
            mode: self.mode,
            scope: self.scope,
            grants: self.grants.clone(),
            known_targets: self.known_targets.clone(),
            ..Policy::default()
        }
    }
    pub fn ai_policy(&self) -> AiPolicy {
        AiPolicy {
            enabled: self.ai_enabled,
            mode: self.ai_mode,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundKey {
    provider_id: String,
    base_url: String,
    key: String,
}

impl AiConfiguration {
    pub fn validate(&self) -> Result<()> {
        self.settings.execution.validate()?;
        let mut ids = std::collections::HashSet::new();
        for provider in &self.settings.providers {
            provider.validate()?;
            ensure!(
                uuid::Uuid::parse_str(&provider.id).is_ok() && ids.insert(provider.id.clone()),
                "AI_INVALID_NAME"
            );
            ensure!(
                !provider.name.trim().is_empty() && provider.name.chars().count() <= 100,
                "AI_INVALID_NAME"
            );
            ensure!(
                provider
                    .models
                    .iter()
                    .all(|m| !m.id.is_empty() && m.id.len() <= 256)
                    && provider
                        .models
                        .iter()
                        .any(|m| m.id == provider.default_model),
                "AI_NO_MODEL"
            );
        }
        ensure!(
            self.settings.default_provider.as_ref().is_none_or(|id| self
                .settings
                .providers
                .iter()
                .any(|p| &p.id == id && p.enabled)),
            "AI_NO_PROVIDER"
        );
        ensure!(
            self.keys
                .iter()
                .all(|(id, key)| ids.contains(id) && key.len() <= 16384),
            "AI_KEY_REQUIRED"
        );
        Ok(())
    }

    pub fn export(
        &self,
        password: &str,
        previous: Option<&SyncAiConfiguration>,
    ) -> Result<SyncAiConfiguration> {
        self.validate()?;
        let mut keys = BTreeMap::new();
        for provider in &self.settings.providers {
            let secret = match self.keys.get(&provider.id) {
                None => previous
                    .and_then(|p| p.keys.get(&provider.id))
                    .cloned()
                    .unwrap_or(SyncSecret::Omitted),
                Some(_) if password.chars().count() < 8 => SyncSecret::Omitted,
                Some(key) if key.is_empty() => SyncSecret::Empty,
                Some(key) => {
                    // Reuse authenticated ciphertext only when its identity, endpoint and
                    // plaintext match; random nonce changes must not cause endless PUTs.
                    let previous = previous.and_then(|p| p.keys.get(&provider.id));
                    if previous
                        .and_then(|s| decode_key(s, provider, password).ok())
                        .as_ref()
                        == Some(key)
                    {
                        previous.cloned().unwrap_or(SyncSecret::Omitted)
                    } else {
                        let value = serde_json::to_string(&BoundKey {
                            provider_id: provider.id.clone(),
                            base_url: provider.base_url.clone(),
                            key: key.clone(),
                        })?;
                        SyncSecret::Encrypted(crate::crypto::encrypt_field(&value, password)?)
                    }
                }
            };
            keys.insert(provider.id.clone(), secret);
        }
        Ok(SyncAiConfiguration {
            settings: self.settings.clone(),
            keys,
        })
    }
}

fn decode_key(
    secret: &SyncSecret,
    provider: &crate::ai::Provider,
    password: &str,
) -> Result<String> {
    let SyncSecret::Encrypted(ciphertext) = secret else {
        anyhow::bail!("AI_KEY_LOCKED")
    };
    ensure!(!password.is_empty(), "AI_KEY_LOCKED");
    let value: BoundKey =
        serde_json::from_str(&crate::crypto::decrypt_field(ciphertext, password)?)?;
    ensure!(
        value.provider_id == provider.id
            && value.base_url == provider.base_url
            && value.key.len() <= 16384,
        "AI_KEY_LOCKED"
    );
    Ok(value.key)
}

impl SyncAiConfiguration {
    pub fn resolve(
        &self,
        local: Option<&AiConfiguration>,
        password: Option<&str>,
    ) -> (AiConfiguration, u32) {
        let mut keys = BTreeMap::new();
        let mut unavailable = 0;
        for provider in &self.settings.providers {
            let local_key = local
                .filter(|local| {
                    local
                        .settings
                        .providers
                        .iter()
                        .any(|p| p.id == provider.id && p.base_url == provider.base_url)
                })
                .and_then(|local| local.keys.get(&provider.id));
            let value = match (
                self.keys.get(&provider.id),
                password.filter(|p| !p.is_empty()),
            ) {
                (Some(SyncSecret::Empty), Some(_)) => Some(String::new()),
                (Some(secret @ SyncSecret::Encrypted(_)), Some(password)) => {
                    match decode_key(secret, provider, password) {
                        Ok(key) => Some(key),
                        Err(_) => {
                            unavailable += 1;
                            local_key.cloned()
                        }
                    }
                }
                (Some(SyncSecret::Encrypted(_)), None) => {
                    unavailable += 1;
                    local_key.cloned()
                }
                (Some(SyncSecret::LegacyPlaintext(_)), _) => {
                    unavailable += 1;
                    local_key.cloned()
                }
                _ => local_key.cloned(),
            };
            if let Some(value) = value {
                keys.insert(provider.id.clone(), value);
            }
        }
        (
            AiConfiguration {
                settings: self.settings.clone(),
                keys,
            },
            unavailable,
        )
    }
}

pub(super) fn merge_initial_ai(
    local: Option<&AiConfiguration>,
    mut remote: AiConfiguration,
) -> AiConfiguration {
    let Some(local) = local else {
        return remote;
    };
    if local.settings == Settings::default() {
        return remote;
    }
    let mut merged = local.clone();
    for provider in remote.settings.providers.drain(..) {
        if !merged
            .settings
            .providers
            .iter()
            .any(|p| p.id == provider.id)
        {
            if let Some(key) = remote.keys.remove(&provider.id) {
                merged.keys.insert(provider.id.clone(), key);
            }
            merged.settings.providers.push(provider);
        }
    }
    if merged.settings.default_provider.is_none() {
        merged.settings.default_provider = remote.settings.default_provider.filter(|id| {
            merged
                .settings
                .providers
                .iter()
                .any(|p| &p.id == id && p.enabled)
        });
    }
    if merged.settings.execution == crate::ai::ExecutionSettings::default() {
        merged.settings.execution = remote.settings.execution;
    }
    merged
}
