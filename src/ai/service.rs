use super::{storage::Store, *};
use anyhow::{Result, anyhow, ensure};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::watch;
mod audit;
mod budget;
mod continuation;
#[cfg(test)]
mod continuation_tests;
#[cfg(test)]
mod followup_tests;
mod followups;
#[cfg(test)]
mod optimization_tests;
mod panel_snapshot;
#[cfg(test)]
mod permission_tests;
mod permissions;
mod runner;
mod sync;
#[cfg(test)]
mod tests;
mod title;
#[cfg(test)]
mod title_tests;
mod tools;
mod toolset;
#[cfg(test)]
mod usage_tests;

#[derive(Clone, Default)]
pub(crate) struct Snapshot {
    pub settings: Settings,
    pub conversations: Vec<Conversation>,
    pub ready: bool,
    pub temporary: bool,
    pub storage_error: Option<String>,
    pub keys_saved: HashSet<String>,
    pub temporary_keys: HashSet<String>,
    pub busy: bool,
    pub notice: Option<String>,
    pub history_notice: Option<String>,
    pub discovered_models: Option<(String, String, Vec<String>)>,
}
struct State {
    snapshot: Snapshot,
    cancel: HashMap<String, watch::Sender<bool>>,
    title_cancel: HashMap<String, watch::Sender<bool>>,
    followup_cancel: HashMap<String, (String, watch::Sender<bool>)>,
    continuations: HashMap<String, tokio::sync::oneshot::Sender<()>>,
}
pub(crate) struct Service {
    state: Mutex<State>,
    store: Mutex<Option<Arc<Store>>>,
    persistence: Mutex<()>,
    stamp: AtomicU64,
    configuration_stamp: AtomicU64,
    pub runtime: tokio::runtime::Handle,
    pub engine: Option<Arc<tiny_shell_mcp::engine::Engine>>,
    pub audit: Arc<tiny_shell_mcp::audit::Store>,
}
impl Service {
    pub fn new(
        directory: PathBuf,
        runtime: tokio::runtime::Handle,
        engine: Option<Arc<tiny_shell_mcp::engine::Engine>>,
        privacy_password: Option<String>,
    ) -> Arc<Self> {
        let audit = engine
            .as_ref()
            .map(|e| e.audit.clone())
            .unwrap_or_else(|| tiny_shell_mcp::audit::Store::new(directory.clone()));
        let service = Arc::new(Self {
            state: Mutex::new(State {
                snapshot: Snapshot::default(),
                cancel: HashMap::new(),
                title_cancel: HashMap::new(),
                followup_cancel: HashMap::new(),
                continuations: HashMap::new(),
            }),
            store: Mutex::new(None),
            persistence: Mutex::new(()),
            stamp: AtomicU64::new(1),
            configuration_stamp: AtomicU64::new(1),
            runtime: runtime.clone(),
            engine,
            audit,
        });
        let worker = service.clone();
        runtime.spawn_blocking(move || {
            let result =
                Store::open(directory).map(|(store, settings, conversations, mut warning)| {
                    if let Some(password) = privacy_password {
                        if let Err(error) = store.unlock_keys(&password, &settings) {
                            warning = Some(error.to_string());
                        }
                    } else if !settings.providers.is_empty() {
                        warning = Some("AI_SYNC_PASSWORD_REQUIRED".into());
                    }
                    let keys = settings
                        .providers
                        .iter()
                        .filter(|p| store.key_saved(&p.id))
                        .map(|p| p.id.clone())
                        .collect();
                    (store, settings, conversations, warning, keys)
                });
            if let Ok(mut state) = worker.state.lock() {
                match result {
                    Ok((store, settings, conversations, warning, keys)) => {
                        state.snapshot.temporary = store.temporary();
                        state.snapshot.keys_saved = keys;
                        state.snapshot.settings = settings;
                        state.snapshot.conversations = conversations;
                        state.snapshot.storage_error = warning;
                        if let Ok(mut destination) = worker.store.lock() {
                            *destination = Some(Arc::new(store));
                        }
                    }
                    Err(_) => state.snapshot.storage_error = Some("AI_STORAGE_ERROR".into()),
                }
                state.snapshot.ready = true;
            }
            worker.changed();
        });
        service
    }
    pub fn stamp(&self) -> u64 {
        self.stamp.load(Ordering::Relaxed)
    }
    pub fn configuration_stamp(&self) -> u64 {
        self.configuration_stamp.load(Ordering::Relaxed)
    }
    fn configuration_changed(&self) {
        self.configuration_stamp.fetch_add(1, Ordering::Relaxed);
    }
    pub fn activity_counts(&self) -> (usize, usize) {
        self.state
            .lock()
            .map(|state| {
                let conversations = &state.snapshot.conversations;
                (
                    conversations.iter().filter(|c| c.state.active()).count(),
                    conversations
                        .iter()
                        .filter(|c| {
                            matches!(
                                c.state,
                                TurnState::AwaitingApproval | TurnState::AwaitingContinuation
                            )
                        })
                        .count(),
                )
            })
            .unwrap_or_default()
    }
    fn changed(&self) {
        self.stamp.fetch_add(1, Ordering::Relaxed);
    }
    pub fn snapshot(&self) -> Snapshot {
        self.state
            .lock()
            .map(|s| s.snapshot.clone())
            .unwrap_or_default()
    }
    fn storage(&self) -> Result<Arc<Store>> {
        self.store
            .lock()
            .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?
            .clone()
            .ok_or_else(|| anyhow!("AI_STORAGE_ERROR"))
    }
    pub fn create(&self, target: Option<BoundTarget>) -> Result<String> {
        let mut state = self.state.lock().map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        ensure!(state.snapshot.ready, "AI_LOADING");
        let conversation = Conversation::new(target, &state.snapshot.settings);
        let id = conversation.id.clone();
        state.snapshot.conversations.insert(0, conversation);
        self.changed();
        Ok(id)
    }
    pub fn configure_conversation(
        &self,
        id: &str,
        provider_id: Option<String>,
        model: String,
        target: Option<BoundTarget>,
    ) -> Result<()> {
        self.mutate(id, |c| {
            ensure!(!c.state.active(), "AI_BUSY");
            if c.messages.is_empty() {
                c.target = target;
            }
            c.provider_id = provider_id;
            c.model = model;
            Ok(())
        })
    }
    fn mutate(&self, id: &str, action: impl FnOnce(&mut Conversation) -> Result<()>) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        let conversation = state
            .snapshot
            .conversations
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(|| anyhow!("AI_CONVERSATION_MISSING"))?;
        action(conversation)?;
        conversation.updated = chrono::Utc::now().timestamp();
        self.changed();
        Ok(())
    }
    pub fn rename(self: &Arc<Self>, id: String, title: String) {
        // Record the user's choice before any async persistence or title response can race it.
        if let Err(error) = self.mutate(&id, |c| {
            ensure!(
                !title.trim().is_empty() && title.chars().count() <= 100,
                "AI_INVALID_NAME"
            );
            c.title = title.trim().into();
            c.title_state = TitleState::Preserved;
            Ok(())
        }) {
            self.history_notice(Err(error));
            return;
        }
        self.cancel_title(&id);
        let service = self.clone();
        self.runtime.spawn_blocking(move || {
            let result = service.checkpoint(&id);
            service.history_notice(result);
        });
    }
    pub fn stop(&self, id: &str) {
        if let Ok(state) = self.state.lock()
            && let Some(cancel) = state.cancel.get(id)
        {
            cancel.send_replace(true);
        }
        self.cancel_title(id);
        self.cancel_followups(id);
    }
    pub fn stop_all(&self) {
        if let Ok(state) = self.state.lock() {
            for cancel in state.cancel.values().chain(state.title_cancel.values()) {
                cancel.send_replace(true);
            }
            for (_, cancel) in state.followup_cancel.values() {
                cancel.send_replace(true);
            }
        }
    }
    pub fn delete(self: &Arc<Self>, ids: Vec<String>) {
        for id in &ids {
            self.stop(id);
        }
        let service = self.clone();
        self.runtime.spawn_blocking(move || {
            let result = (|| {
                let _serial = service
                    .persistence
                    .lock()
                    .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
                let store = service.storage()?;
                for id in &ids {
                    store.delete(id)?;
                    service.history_deleted(id);
                    let mut state = service
                        .state
                        .lock()
                        .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
                    state.snapshot.conversations.retain(|c| &c.id != id);
                }
                Ok(())
            })();
            service.history_notice(result);
        });
    }
    fn checkpoint(&self, id: &str) -> Result<()> {
        // Keep I/O away from the snapshot lock while serializing against deletion.
        let _serial = self
            .persistence
            .lock()
            .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        let state = self.state.lock().map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        let conversation = state
            .snapshot
            .conversations
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| anyhow!("AI_CONVERSATION_MISSING"))?
            .clone();
        drop(state);
        if conversation.messages.is_empty() {
            return Ok(());
        }
        self.storage()?.save(&conversation)
    }
    fn storage_failure(&self, error: anyhow::Error) {
        if let Ok(mut state) = self.state.lock() {
            state.snapshot.storage_error = Some(error.to_string());
        }
        self.changed();
    }
    fn notice(&self, result: Result<()>) {
        if let Ok(mut state) = self.state.lock() {
            state.snapshot.busy = false;
            state.snapshot.notice = Some(
                result
                    .err()
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "AI_SAVED".into()),
            );
        }
        self.changed();
    }
    fn history_notice(&self, result: Result<()>) {
        // History jobs must not release a provider request or replace its completion result.
        if let Ok(mut state) = self.state.lock() {
            state.snapshot.history_notice = Some(
                result
                    .err()
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "AI_SAVED".into()),
            );
        }
        self.changed();
    }
    fn begin_operation(&self) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        ensure!(!state.snapshot.busy, "AI_BUSY");
        state.snapshot.busy = true;
        state.snapshot.notice = None;
        self.changed();
        Ok(())
    }
    fn check_endpoint(&self, provider: &Provider, key_supplied: bool) -> Result<()> {
        let state = self.state.lock().map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        ensure!(
            key_supplied
                || state
                    .snapshot
                    .settings
                    .providers
                    .iter()
                    .find(|p| p.id == provider.id)
                    .is_none_or(|p| p.base_url == provider.base_url),
            "AI_ENDPOINT_KEY_REQUIRED"
        );
        Ok(())
    }
    async fn request_key(
        self: &Arc<Self>,
        provider: &Provider,
        key: Option<String>,
    ) -> Result<String> {
        if let Some(key) = key {
            return Ok(key);
        }
        let service = self.clone();
        let provider = provider.clone();
        tokio::task::spawn_blocking(move || {
            let _serial = service
                .persistence
                .lock()
                .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
            service.check_endpoint(&provider, false)?;
            let result = service.storage()?.key(&provider.id, &provider.base_url);
            if provider.allow_no_key {
                Ok(result.unwrap_or_default())
            } else {
                result
            }
        })
        .await?
    }
    pub fn save_provider(
        self: &Arc<Self>,
        provider: Provider,
        key: Option<String>,
        default: bool,
    ) -> Result<()> {
        provider.validate()?;
        self.check_endpoint(&provider, key.is_some())?;
        ensure!(
            !provider.name.trim().is_empty() && provider.name.chars().count() <= 100,
            "AI_INVALID_NAME"
        );
        ensure!(
            provider
                .models
                .iter()
                .any(|m| m.id == provider.default_model)
                && provider
                    .models
                    .iter()
                    .all(|m| !m.id.is_empty() && m.id.len() <= 256),
            "AI_NO_MODEL"
        );
        self.begin_operation()?;
        let service = self.clone();
        self.runtime.spawn_blocking(move || {
            let result = (|| {
                let _serial = service
                    .persistence
                    .lock()
                    .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
                let store = service.storage()?;
                let state = service
                    .state
                    .lock()
                    .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
                let old = state
                    .snapshot
                    .settings
                    .providers
                    .iter()
                    .find(|p| p.id == provider.id);
                let old_url = old.map(|p| p.base_url.clone());
                let mut settings = state.snapshot.settings.clone();
                drop(state);
                if old_url
                    .as_ref()
                    .is_some_and(|url| url != &provider.base_url)
                    && key.is_none()
                {
                    anyhow::bail!("AI_ENDPOINT_KEY_REQUIRED");
                }
                if let Some(old) = settings.providers.iter_mut().find(|p| p.id == provider.id) {
                    *old = provider.clone();
                } else {
                    settings.providers.push(provider.clone());
                }
                if default {
                    ensure!(provider.enabled, "AI_PROVIDER_DISABLED");
                    settings.default_provider = Some(provider.id.clone());
                }
                if !provider.enabled && settings.default_provider.as_deref() == Some(&provider.id) {
                    settings.default_provider = None;
                }
                if let Some(key) = key {
                    store.set_key(&provider.id, &provider.base_url, &key)?;
                }
                store.save_settings(&settings)?;
                let temporary_key = store.key_temporary(&provider.id);
                let key_saved = !temporary_key && store.key_saved(&provider.id);
                let mut state = service
                    .state
                    .lock()
                    .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
                state.snapshot.settings = settings;
                service.configuration_changed();
                state.snapshot.temporary = store.temporary();
                if temporary_key {
                    state.snapshot.temporary_keys.insert(provider.id.clone());
                } else {
                    state.snapshot.temporary_keys.remove(&provider.id);
                }
                if key_saved {
                    state.snapshot.keys_saved.insert(provider.id.clone());
                } else {
                    state.snapshot.keys_saved.remove(&provider.id);
                }
                Ok(())
            })();
            service.notice(result);
        });
        Ok(())
    }
    pub fn remove_provider(self: &Arc<Self>, id: String) -> Result<()> {
        self.begin_operation()?;
        let service = self.clone();
        self.runtime.spawn_blocking(move || {
            let result = (|| {
                let _serial = service
                    .persistence
                    .lock()
                    .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
                let store = service.storage()?;
                let state = service
                    .state
                    .lock()
                    .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
                ensure!(
                    !state
                        .snapshot
                        .conversations
                        .iter()
                        .any(|c| c.provider_id.as_deref() == Some(&id) && c.state.active()),
                    "AI_BUSY"
                );
                let mut settings = state.snapshot.settings.clone();
                drop(state);
                store.delete_key(&id)?;
                settings.providers.retain(|p| p.id != id);
                if settings.default_provider.as_deref() == Some(&id) {
                    settings.default_provider = None;
                }
                store.save_settings(&settings)?;
                let mut state = service
                    .state
                    .lock()
                    .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
                state.snapshot.settings = settings;
                service.configuration_changed();
                state.snapshot.keys_saved.remove(&id);
                state.snapshot.temporary_keys.remove(&id);
                Ok(())
            })();
            service.notice(result);
        });
        Ok(())
    }
    pub fn clear_key(self: &Arc<Self>, id: String) -> Result<()> {
        self.begin_operation()?;
        let service = self.clone();
        self.runtime.spawn_blocking(move || {
            let result = (|| {
                let _serial = service
                    .persistence
                    .lock()
                    .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
                service.storage()?.delete_key(&id)
            })();
            if result.is_ok()
                && let Ok(mut state) = service.state.lock()
            {
                service.configuration_changed();
                state.snapshot.keys_saved.remove(&id);
                state.snapshot.temporary_keys.remove(&id);
            }
            service.notice(result);
        });
        Ok(())
    }
    pub fn test_provider(self: &Arc<Self>, provider: Provider, key: Option<String>) -> Result<()> {
        provider.validate()?;
        self.check_endpoint(&provider, key.is_some())?;
        self.begin_operation()?;
        let service = self.clone();
        self.runtime.spawn(async move {
            let result = service.test_provider_request(&provider, key).await;
            match result {
                Ok(()) => {
                    service.notice(Ok(()));
                    if let Ok(mut state) = service.state.lock() {
                        state.snapshot.notice = Some("AI_TEST_OK".into());
                    }
                    service.changed();
                }
                Err(e) => service.notice(Err(e)),
            }
        });
        Ok(())
    }
    pub fn list_models(self: &Arc<Self>, provider: Provider, key: Option<String>) -> Result<()> {
        provider.validate()?;
        self.check_endpoint(&provider, key.is_some())?;
        self.begin_operation()?;
        let service = self.clone();
        self.runtime.spawn(async move {
            let result = async {
                let key = service.request_key(&provider, key).await?;
                super::provider::models(&provider, &key).await
            }
            .await;
            match result {
                Ok(models) => {
                    if let Ok(mut state) = service.state.lock() {
                        state.snapshot.busy = false;
                        state.snapshot.discovered_models =
                            Some((provider.id.clone(), provider.base_url.clone(), models));
                        state.snapshot.notice = None;
                    }
                    service.changed();
                }
                Err(e) => service.notice(Err(e)),
            }
        });
        Ok(())
    }
}
