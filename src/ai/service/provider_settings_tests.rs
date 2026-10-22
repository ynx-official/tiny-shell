use super::tests::{fixture, settled};
use super::*;

#[tokio::test]
async fn provider_and_global_default_are_persisted_together_without_changing_existing_chats()
-> Result<()> {
    let directory =
        std::env::temp_dir().join(format!("tiny-shell-provider-test-{}", uuid::Uuid::new_v4()));
    let first = Provider {
        name: "First".into(),
        base_url: "https://api.example.com/v1".into(),
        models: vec![Model {
            id: "shared".into(),
            supports_tools: true,
        }],
        default_model: "shared".into(),
        ..Provider::default()
    };
    let service = fixture(directory.clone(), first.clone(), None)?;
    let old_conversation = service.create(None)?;
    let mut second = first.clone();
    second.id = uuid::Uuid::new_v4().to_string();
    second.name = "Second".into();
    let selection = ModelSelection {
        provider_id: second.id.clone(),
        model_id: "shared".into(),
    };
    service.save_provider(
        second.clone(),
        Some("fixture-key".into()),
        Some(selection.clone()),
    )?;
    settled(&service).await?;
    assert_eq!(service.snapshot().notice.as_deref(), Some("AI_SAVED"));
    let persisted: Settings =
        serde_json::from_slice(&std::fs::read(directory.join("ai/providers.json"))?)?;
    assert_eq!(persisted.default_selection(), Some(selection.clone()));
    assert_eq!(persisted.providers.len(), 2);

    let new_conversation = service.create(None)?;
    let snapshot = service.snapshot();
    assert_eq!(
        snapshot
            .conversations
            .iter()
            .find(|c| c.id == old_conversation)
            .and_then(|c| c.provider_id.as_deref()),
        Some(first.id.as_str())
    );
    assert_eq!(
        snapshot
            .conversations
            .iter()
            .find(|c| c.id == new_conversation)
            .and_then(|c| c.provider_id.as_deref()),
        Some(second.id.as_str())
    );

    // Reject removing the selected provider before any credentials or settings are touched.
    let before = std::fs::read(directory.join("ai/providers.json"))?;
    assert!(service.remove_provider(second.id.clone()).is_err());
    assert_eq!(
        service.storage()?.key(&second.id, &second.base_url)?,
        "fixture-key"
    );
    let invalid = ModelSelection {
        provider_id: second.id.clone(),
        model_id: "missing".into(),
    };
    assert!(
        service
            .save_provider(first.clone(), None, Some(invalid))
            .is_err()
    );
    assert_eq!(std::fs::read(directory.join("ai/providers.json"))?, before);
    assert_eq!(
        service.snapshot().settings.default_selection(),
        Some(selection)
    );

    // A selection from another provider is saved even while the editor stays on the first.
    let replacement = ModelSelection {
        provider_id: first.id.clone(),
        model_id: "shared".into(),
    };
    service.save_provider(second.clone(), None, Some(replacement.clone()))?;
    settled(&service).await?;
    assert_eq!(
        service.snapshot().settings.default_selection(),
        Some(replacement)
    );
    service.remove_provider(second.id)?;
    settled(&service).await?;
    assert_eq!(service.snapshot().settings.providers.len(), 1);
    service.audit.close().await?;
    std::fs::remove_dir_all(directory)?;
    Ok(())
}
