//! In-process capabilities cannot be deserialized or authenticated over IPC.
use super::*;
use crate::policy::Mode;
use crate::storage::{atomic_bytes, private_bytes};

/// Global built-in permissions; each capability still binds exactly one SSH target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AiPolicy {
    pub enabled: bool,
    pub mode: Mode,
}

#[derive(Default)]
pub(super) struct Access {
    policy: AiPolicy,
    generation: u64,
    clients: HashMap<String, Binding>,
}
struct Binding {
    target: Target,
    mode: Option<Mode>,
    generation: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredAccess {
    schema_version: u32,
    enabled: bool,
    #[serde(default)]
    mode: Option<Mode>,
}
impl Access {
    pub(super) fn load(store: &Store, external: &Policy) -> anyhow::Result<Self> {
        let path = store.root().join("ai-access.json");
        let policy = if path.exists() {
            let saved: StoredAccess = serde_json::from_slice(&private_bytes(&path, 4096)?)?;
            anyhow::ensure!(
                saved.schema_version == 1 || (saved.schema_version == 2 && saved.mode.is_some()),
                "unsupported internal access format"
            );
            let policy = AiPolicy {
                enabled: saved.enabled,
                // Preserve the previous shared mode once when migrating v1.
                mode: saved.mode.unwrap_or(external.mode),
            };
            if saved.schema_version == 1 {
                atomic_bytes(
                    &path,
                    &serde_json::to_vec(&StoredAccess {
                        schema_version: 2,
                        enabled: policy.enabled,
                        mode: Some(policy.mode),
                    })?,
                )?;
            }
            policy
        } else {
            AiPolicy::default()
        };
        Ok(Self {
            policy,
            generation: 0,
            clients: HashMap::new(),
        })
    }
}
#[derive(Clone)]
pub struct InternalClient {
    engine: Arc<Engine>,
    id: String,
    generation: u64,
}
impl InternalClient {
    pub async fn call_traced(
        &self,
        call: Call,
        mut context: crate::audit::Context,
    ) -> Result<Value> {
        if !self.is_valid() {
            return Err(Failure::new("CLIENT_REVOKED"));
        }
        context.source = crate::audit::Source::Ai;
        context.client_id = Some(self.id.clone());
        context.conversation_id = self.id.strip_prefix("ai:").map(str::to_owned);
        self.engine
            .dispatch_traced(&self.id, call, Some(self.generation), context)
            .await
    }
    pub async fn call(&self, call: Call) -> Result<Value> {
        if !self.is_valid() {
            return Err(Failure::new("CLIENT_REVOKED"));
        }
        self.engine
            .dispatch(&self.id, call, Some(self.generation))
            .await
    }
    /// Built-in policy changes or a new turn for this conversation revoke the handle.
    pub fn is_valid(&self) -> bool {
        let Ok(access) = self.engine.ai.lock() else {
            return false;
        };
        access.policy.enabled
            && access
                .clients
                .get(&self.id)
                .is_some_and(|b| b.generation == self.generation)
    }
    /// Let the model adapter abandon an in-flight request when SSH authorization changes.
    pub async fn revoked(&self) {
        let mut changes = self.engine.changes.subscribe();
        while self.is_valid() {
            if changes.changed().await.is_err() {
                return;
            }
        }
    }
    pub fn mode(&self) -> Result<Mode> {
        let access = self
            .engine
            .ai
            .lock()
            .map_err(|_| Failure::new("INTERNAL_ERROR"))?;
        if !access.policy.enabled
            || access
                .clients
                .get(&self.id)
                .is_none_or(|b| b.generation != self.generation)
        {
            return Err(Failure::new("CLIENT_REVOKED"));
        }
        Ok(access
            .clients
            .get(&self.id)
            .and_then(|b| b.mode)
            .unwrap_or(access.policy.mode))
    }
    pub fn stop(&self) -> Result<()> {
        self.engine.stop_client(&self.id, self.generation)
    }
}
impl Engine {
    pub(super) fn verify_internal_epoch(&self, client: &str, expected: Option<u64>) -> Result<()> {
        if let Some(generation) = expected {
            let access = self.ai.lock().map_err(|_| Failure::new("INTERNAL_ERROR"))?;
            if !access.policy.enabled
                || access
                    .clients
                    .get(client)
                    .is_none_or(|b| b.generation != generation)
            {
                return Err(Failure::new("CLIENT_REVOKED"));
            }
        }
        Ok(())
    }
    pub fn ai_enabled(&self) -> bool {
        self.ai.lock().is_ok_and(|a| a.policy.enabled)
    }
    pub fn ai_policy(&self) -> Result<AiPolicy> {
        Ok(self
            .ai
            .lock()
            .map_err(|_| Failure::new("INTERNAL_ERROR"))?
            .policy)
    }
    pub fn set_ai_enabled(&self, enabled: bool) -> Result<()> {
        let mut policy = self.ai_policy()?;
        policy.enabled = enabled;
        self.set_ai_policy(policy)
    }
    pub fn set_ai_policy(&self, policy: AiPolicy) -> Result<()> {
        self.set_ai_policy_checked(policy, None)
    }
    pub fn set_synced_ai_policy(&self, policy: AiPolicy, expected_stamp: u64) -> Result<()> {
        self.set_ai_policy_checked(policy, Some(expected_stamp))
    }
    fn set_ai_policy_checked(&self, policy: AiPolicy, expected_stamp: Option<u64>) -> Result<()> {
        // Same lock order as dispatch: make persistence, epoch change and cancellation atomic.
        let mut inner = self.lock()?;
        if expected_stamp.is_some_and(|stamp| stamp != self.configuration_stamp()) {
            return Err(Failure::new("POLICY_CHANGED"));
        }
        let mut access = self.ai.lock().map_err(|_| Failure::new("INTERNAL_ERROR"))?;
        if access.policy == policy {
            return Ok(());
        }
        atomic_bytes(
            &self.store.root().join("ai-access.json"),
            &serde_json::to_vec(&StoredAccess {
                schema_version: 2,
                enabled: policy.enabled,
                mode: Some(policy.mode),
            })
            .map_err(|_| Failure::new("STORAGE_ERROR"))?,
        )
        .map_err(|_| Failure::new("STORAGE_ERROR"))?;
        let actor = if expected_stamp.is_some() {
            "cloud_sync"
        } else {
            "local_user"
        };
        let channel = if expected_stamp.is_some() {
            "synchronization"
        } else {
            "application"
        };
        self.audit.record(crate::audit::Event::new(crate::audit::Context::new(crate::audit::Source::Local),"policy_changed","permissions","completed",json!({"scope":"builtin_ai_all_conversations","before":{"enabled":access.policy.enabled,"mode":access.policy.mode},"after":{"enabled":policy.enabled,"mode":policy.mode},"actor":actor,"channel":channel})));
        access.policy = policy;
        self.configuration_stamp.fetch_add(1, Ordering::Relaxed);
        access.generation = access.generation.wrapping_add(1);
        let revoked_generation = access.generation;
        for binding in access.clients.values_mut() {
            binding.generation = revoked_generation;
        }
        drop(access);
        self.cancel_source(&mut inner, true);
        self.changed();
        Ok(())
    }
    pub fn internal_client(
        self: &Arc<Self>,
        conversation: &str,
        target: Target,
    ) -> Result<InternalClient> {
        self.internal_client_with_mode(conversation, target, None)
    }
    /// An explicit conversation mode overrides the default, never the global enable switch.
    pub fn internal_client_with_mode(
        self: &Arc<Self>,
        conversation: &str,
        target: Target,
        mode: Option<Mode>,
    ) -> Result<InternalClient> {
        crate::contracts::valid_id(conversation)?;
        let id = format!("ai:{conversation}");
        let inner = self.lock()?;
        let mut access = self.ai.lock().map_err(|_| Failure::new("INTERNAL_ERROR"))?;
        if !access.policy.enabled {
            return Err(Failure::new("CONNECTION_NOT_ALLOWED"));
        }
        if access
            .clients
            .get(&id)
            .is_some_and(|b| b.target.id != target.id || b.target.digest != target.digest)
        {
            return Err(Failure::new("TARGET_CHANGED"));
        }
        // A new turn gets a fresh epoch. Old handles must never inherit its mode,
        // and pending approvals/sessions must finish before it can be issued.
        if inner
            .tasks
            .values()
            .any(|t| t.review.client == id && !t.review.state.terminal())
            || inner.sessions.values().any(|s| s.client == id)
        {
            return Err(Failure::new("CLIENT_BUSY"));
        }
        access.generation = access.generation.wrapping_add(1);
        let generation = access.generation;
        access.clients.insert(
            id.clone(),
            Binding {
                target,
                mode,
                generation,
            },
        );
        drop(access);
        drop(inner);
        self.changed();
        Ok(InternalClient {
            engine: self.clone(),
            id,
            generation,
        })
    }
    /// Validate the current saved target before sharing any metadata with a provider.
    pub async fn checked_internal_client(
        self: &Arc<Self>,
        conversation: &str,
        target: Target,
    ) -> Result<InternalClient> {
        self.checked_internal_client_with_mode(conversation, target, None)
            .await
    }
    pub async fn checked_internal_client_with_mode(
        self: &Arc<Self>,
        conversation: &str,
        target: Target,
        mode: Option<Mode>,
    ) -> Result<InternalClient> {
        let client = self.internal_client_with_mode(conversation, target.clone(), mode)?;
        if !self
            .remote
            .targets()
            .await?
            .iter()
            .any(|t| t.id == target.id && t.digest == target.digest)
        {
            return Err(Failure::new("TARGET_CHANGED"));
        }
        if !client.is_valid() {
            return Err(Failure::new("CLIENT_REVOKED"));
        }
        Ok(client)
    }
    pub(super) fn effective_policy(&self, client: &str, external: &Policy) -> Result<Policy> {
        if !client.starts_with("ai:") {
            return Ok(external.clone());
        }
        let access = self.ai.lock().map_err(|_| Failure::new("INTERNAL_ERROR"))?;
        let binding = access
            .clients
            .get(client)
            .ok_or_else(|| Failure::new("CLIENT_REVOKED"))?;
        let mut policy = Policy {
            enabled: access.policy.enabled,
            mode: binding.mode.unwrap_or(access.policy.mode),
            revision: binding.generation,
            ..Policy::default()
        };
        policy
            .grants
            .insert(binding.target.id.clone(), binding.target.digest.clone());
        Ok(policy)
    }
    pub(super) fn client_enabled(&self, client: &str) -> bool {
        if client.starts_with("ai:") {
            self.ai
                .lock()
                .is_ok_and(|a| a.policy.enabled && a.clients.contains_key(client))
        } else {
            self.store.client(client).is_ok_and(|c| c.enabled)
        }
    }
    pub(super) fn bound_targets(&self, client: &str, targets: Vec<Target>) -> Result<Vec<Target>> {
        if !client.starts_with("ai:") {
            return Ok(targets);
        }
        let access = self.ai.lock().map_err(|_| Failure::new("INTERNAL_ERROR"))?;
        let bound = access
            .clients
            .get(client)
            .ok_or_else(|| Failure::new("CLIENT_REVOKED"))?;
        Ok(targets
            .into_iter()
            .filter(|t| t.id == bound.target.id && t.digest == bound.target.digest)
            .collect())
    }
    fn stop_client(&self, client: &str, generation: u64) -> Result<()> {
        let mut inner = self.lock()?;
        self.verify_internal_epoch(client, Some(generation))?;
        for task in inner
            .tasks
            .values_mut()
            .filter(|t| t.review.client == client && !t.review.state.terminal())
        {
            Self::cancel(task);
        }
        let sessions = inner
            .sessions
            .iter()
            .filter(|(_, s)| s.client == client)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for session in sessions {
            if let Some(active) = inner.sessions.remove(&session) {
                self.close_automatically(session, active, "turn_finished_or_cancelled");
            }
        }
        self.changed();
        Ok(())
    }
}
