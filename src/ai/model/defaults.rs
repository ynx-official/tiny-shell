//! One global selection, projected onto the existing on-disk schema for compatibility.
use super::{Provider, Settings};
use anyhow::{Result, ensure};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ModelSelection {
    pub provider_id: String,
    pub model_id: String,
}

impl Settings {
    pub fn default_selection(&self) -> Option<ModelSelection> {
        let provider = self.providers.iter().find(|p| {
            Some(&p.id) == self.default_provider.as_ref()
                && p.enabled
                && !p.default_model.is_empty()
                && p.models.iter().any(|m| m.id == p.default_model)
        })?;
        Some(ModelSelection {
            provider_id: provider.id.clone(),
            model_id: provider.default_model.clone(),
        })
    }

    pub fn with_provider_default(
        &self,
        mut provider: Provider,
        selection: Option<&ModelSelection>,
    ) -> Result<Self> {
        provider.validate()?;
        ensure!(
            !provider.name.trim().is_empty() && provider.name.chars().count() <= 100,
            "AI_INVALID_NAME"
        );
        ensure!(
            !provider.models.is_empty()
                && provider
                    .models
                    .iter()
                    .all(|m| !m.id.is_empty() && m.id.len() <= 256),
            "AI_NO_MODEL"
        );
        // Non-default providers retain this legacy field for connection tests and old clients.
        // It is never interpreted as another global default.
        if !provider
            .models
            .iter()
            .any(|m| m.id == provider.default_model)
        {
            provider.default_model = provider.models[0].id.clone();
        }
        let mut next = self.clone();
        if let Some(old) = next.providers.iter_mut().find(|p| p.id == provider.id) {
            *old = provider;
        } else {
            next.providers.push(provider);
        }
        if let Some(selection) = selection {
            let selected = next
                .providers
                .iter_mut()
                .find(|p| {
                    p.id == selection.provider_id
                        && p.enabled
                        && p.models.iter().any(|m| m.id == selection.model_id)
                })
                .ok_or_else(|| anyhow::anyhow!("AI_DEFAULT_REQUIRED"))?;
            selected.default_model = selection.model_id.clone();
            next.default_provider = Some(selected.id.clone());
        } else {
            ensure!(
                !next
                    .providers
                    .iter()
                    .any(|p| p.enabled && !p.models.is_empty()),
                "AI_DEFAULT_REQUIRED"
            );
            next.default_provider = None;
        }
        Ok(next)
    }

    pub fn without_provider(&self, id: &str) -> Result<Self> {
        let mut next = self.clone();
        next.providers.retain(|p| p.id != id);
        if next.default_provider.as_deref() == Some(id) {
            ensure!(
                !next
                    .providers
                    .iter()
                    .any(|p| p.enabled && !p.models.is_empty()),
                "AI_DEFAULT_REPLACE"
            );
            next.default_provider = None;
        }
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Conversation, Model};
    use super::*;

    fn provider(id: &str) -> Provider {
        Provider {
            id: id.into(),
            name: id.into(),
            base_url: "https://api.example.com/v1".into(),
            models: vec!["shared", "second"]
                .into_iter()
                .map(|id| Model {
                    id: id.into(),
                    supports_tools: true,
                })
                .collect(),
            default_model: "shared".into(),
            ..Provider::default()
        }
    }

    fn settings() -> Settings {
        Settings {
            providers: vec![provider("a"), provider("b")],
            default_provider: Some("a".into()),
            ..Settings::default()
        }
    }

    #[test]
    fn default_pair_uses_provider_identity_and_only_changes_new_conversations() {
        let before = settings();
        let existing = Conversation::new(None, &before);
        let selection = ModelSelection {
            provider_id: "b".into(),
            model_id: "second".into(),
        };
        // Editing A and choosing B's model must be one settings update.
        let after = before
            .with_provider_default(provider("a"), Some(&selection))
            .unwrap();
        assert_eq!(after.default_selection(), Some(selection));
        let new = Conversation::new(None, &after);
        assert_eq!(new.provider_id.as_deref(), Some("b"));
        assert_eq!(new.model, "second");
        assert_eq!(existing.provider_id.as_deref(), Some("a"));
        assert_eq!(existing.model, "shared");
        assert_eq!(before.default_provider.as_deref(), Some("a"));
    }

    #[test]
    fn invalid_or_incomplete_default_never_overwrites_settings() {
        let before = settings();
        for selection in [
            None,
            Some(ModelSelection {
                provider_id: "b".into(),
                model_id: String::new(),
            }),
            Some(ModelSelection {
                provider_id: "missing".into(),
                model_id: "shared".into(),
            }),
            Some(ModelSelection {
                provider_id: "b".into(),
                model_id: "missing".into(),
            }),
        ] {
            assert!(
                before
                    .with_provider_default(provider("a"), selection.as_ref())
                    .is_err()
            );
            assert_eq!(before.default_provider.as_deref(), Some("a"));
        }
        let mut disabled = provider("a");
        disabled.enabled = false;
        assert!(
            before
                .with_provider_default(disabled.clone(), before.default_selection().as_ref())
                .is_err()
        );
        let replacement = ModelSelection {
            provider_id: "b".into(),
            model_id: "shared".into(),
        };
        assert!(
            before
                .with_provider_default(disabled, Some(&replacement))
                .is_ok()
        );
    }

    #[test]
    fn removal_requires_replacement_unless_no_enabled_providers_remain() {
        let before = settings();
        assert!(before.without_provider("a").is_err());
        let last = before.without_provider("b").unwrap();
        assert!(
            last.without_provider("a")
                .unwrap()
                .default_selection()
                .is_none()
        );
        let mut disabled = provider("a");
        disabled.enabled = false;
        assert!(
            last.with_provider_default(disabled, None)
                .unwrap()
                .default_selection()
                .is_none()
        );
    }

    #[test]
    fn legacy_settings_round_trip_and_ignore_inactive_provider_preferences() {
        let old = settings();
        let serialized = serde_json::to_string(&old).unwrap();
        let restored: Settings = serde_json::from_str(&serialized).unwrap();
        assert_eq!(
            restored.default_selection(),
            Some(ModelSelection {
                provider_id: "a".into(),
                model_id: "shared".into()
            })
        );
        let mut invalid = restored;
        invalid.providers[0].models.clear();
        assert!(invalid.default_selection().is_none());
        let conversation = Conversation::new(None, &invalid);
        assert!(conversation.provider_id.is_none());
        assert!(conversation.model.is_empty());
    }
}
