use super::*;
use crate::sync::automation::AiConfiguration;
use std::collections::BTreeMap;

impl Service {
    pub(crate) fn export_sync(&self, password: &str) -> Result<AiConfiguration> {
        let _serial = self
            .persistence
            .lock()
            .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        let settings = {
            let state = self.state.lock().map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
            ensure!(state.snapshot.ready, "AI_LOADING");
            state.snapshot.settings.clone()
        };
        let store = self.storage()?;
        if !password.is_empty() {
            store.prepare_keys(password, &settings)?;
        }
        ensure!(
            !store.keys_locked() || !store.has_key_file(),
            "AI_KEY_LOCKED"
        );
        let mut keys = BTreeMap::new();
        for provider in &settings.providers {
            if store.keys_locked() && !store.key_temporary(&provider.id) {
                continue;
            }
            keys.insert(
                provider.id.clone(),
                store.key(&provider.id, &provider.base_url)?,
            );
        }
        self.refresh_key_status(&store, &settings)?;
        self.changed();
        Ok(AiConfiguration { settings, keys })
    }

    pub(crate) fn import_sync(
        &self,
        configuration: &AiConfiguration,
        password: &str,
        expected_stamp: Option<u64>,
    ) -> Result<()> {
        configuration.validate()?;
        let _serial = self
            .persistence
            .lock()
            .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        ensure!(
            expected_stamp.is_none_or(|stamp| stamp == self.configuration_stamp())
                && !self.snapshot().busy,
            "AI_BUSY"
        );
        let settings = self.snapshot().settings;
        let store = self.storage()?;
        if !password.is_empty() {
            store.adopt_password(password, &settings)?;
        }
        // A locked existing vault must not be rebound to remotely changed endpoints.
        // Leave its settings intact until the correct privacy password is supplied.
        if store.keys_locked() && password.is_empty() && store.has_key_file() {
            return Ok(());
        }
        let mut keys = configuration.keys.clone();
        for provider in &configuration.settings.providers {
            if !keys.contains_key(&provider.id)
                && (!store.keys_locked() || store.key_temporary(&provider.id))
                && settings
                    .providers
                    .iter()
                    .any(|old| old.id == provider.id && old.base_url == provider.base_url)
            {
                keys.insert(
                    provider.id.clone(),
                    store.key(&provider.id, &provider.base_url)?,
                );
            }
        }
        let before = self.export_unlocked(&store, &settings)?;
        let next = AiConfiguration {
            settings: configuration.settings.clone(),
            keys,
        };
        if before == next {
            self.refresh_key_status(&store, &settings)?;
            self.changed();
            return Ok(());
        }
        // Keys bind their endpoint independently. Saving settings first cannot
        // send an old key to a new URL; a key-write failure preserves the vault.
        store.save_settings(&next.settings)?;
        if let Err(error) = store.replace_keys(&next.keys, &next.settings) {
            store.save_settings(&settings)?;
            return Err(error);
        }
        self.state
            .lock()
            .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?
            .snapshot
            .settings = next.settings.clone();
        self.cancel_disabled_followups();
        self.refresh_key_status(&store, &next.settings)?;
        self.configuration_changed();
        self.changed();
        Ok(())
    }

    fn export_unlocked(&self, store: &Store, settings: &Settings) -> Result<AiConfiguration> {
        let mut keys = BTreeMap::new();
        for provider in &settings.providers {
            if store.keys_locked() && !store.key_temporary(&provider.id) {
                continue;
            }
            keys.insert(
                provider.id.clone(),
                store.key(&provider.id, &provider.base_url)?,
            );
        }
        Ok(AiConfiguration {
            settings: settings.clone(),
            keys,
        })
    }

    fn refresh_key_status(&self, store: &Store, settings: &Settings) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        state.snapshot.keys_saved = settings
            .providers
            .iter()
            .filter(|p| store.key_saved(&p.id))
            .map(|p| p.id.clone())
            .collect();
        state.snapshot.temporary_keys = settings
            .providers
            .iter()
            .filter(|p| store.key_temporary(&p.id))
            .map(|p| p.id.clone())
            .collect();
        if !store.keys_locked()
            && state
                .snapshot
                .storage_error
                .as_deref()
                .is_some_and(|error| matches!(error, "AI_SYNC_PASSWORD_REQUIRED" | "AI_KEY_LOCKED"))
        {
            state.snapshot.storage_error = None;
        }
        Ok(())
    }

    pub(crate) fn rotate_sync_password(&self, password: &str) -> Result<()> {
        let _serial = self
            .persistence
            .lock()
            .map_err(|_| anyhow!("AI_STORAGE_ERROR"))?;
        let store = self.storage()?;
        if store.keys_locked() {
            store.unlock_keys(password, &self.snapshot().settings)?;
        } else {
            store.rotate_keys(password)?;
        }
        self.configuration_changed();
        self.changed();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stale_sync_cannot_replace_new_model_configuration_or_key() -> Result<()> {
        let directory =
            std::env::temp_dir().join(format!("tiny-shell-ai-sync-{}", uuid::Uuid::new_v4()));
        let provider =
            super::super::tests::provider(std::net::SocketAddr::from(([127, 0, 0, 1], 9)));
        let service = super::super::tests::fixture(directory.clone(), provider.clone(), None)?;
        let before = service.snapshot().settings;
        let mut configuration = AiConfiguration {
            settings: before.clone(),
            keys: BTreeMap::new(),
        };
        configuration.settings.execution.tool_calls = 32;
        let failure = service
            .import_sync(&configuration, "", Some(0))
            .err()
            .ok_or_else(|| anyhow!("stale synchronization was accepted"))?;
        assert_eq!(failure.to_string(), "AI_BUSY");
        assert!(service.snapshot().settings == before);
        assert_eq!(
            service.storage()?.key(&provider.id, &provider.base_url)?,
            "fixture-key"
        );
        service.audit.close().await?;
        std::fs::remove_dir_all(directory)?;
        Ok(())
    }
}
