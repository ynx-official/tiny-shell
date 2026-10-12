use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use tiny_shell_mcp::{
    contracts::{Call, MAX_FRAME},
    ipc::{self, Challenge, Handler},
    policy::{Policy, Scope},
    storage::Store,
};
use tokio::io::AsyncWriteExt;

#[test]
fn changing_a_target_never_inherits_all_scope_authorization() {
    let mut policy = Policy {
        scope: Scope::All,
        ..Policy::default()
    };
    policy.observe([("id".into(), "old".into())]);
    policy.observe([("id".into(), "new".into()), ("new-id".into(), "new".into())]);
    assert!(!policy.authorized("id", "new"));
    assert!(policy.authorized("new-id", "new"));
    policy.grant("id".into(), "new".into());
    assert!(policy.authorized("id", "new"));
}
#[test]
fn schema_parsing_rejects_unknown_fields_and_unsafe_inputs() {
    let id = uuid::Uuid::new_v4().to_string();
    assert!(Call::parse("get_status", json!({"approved":true})).is_err());
    assert!(Call::parse("read_file", json!({"session_id":id,"path":"relative"})).is_err());
    assert!(
        Call::parse(
            "run_command",
            json!({"session_id":id,"command":"ls","request_id":id,"timeout_ms":300001})
        )
        .is_err()
    );
    assert!(
        Call::parse(
            "write_file",
            json!({"session_id":id,"path":"/file","content":"x","mode":"update","request_id":id})
        )
        .is_err()
    );
    assert!(
        Call::parse(
            "run_command",
            json!({"session_id":id,"command":"ls","request_id":id})
        )
        .is_ok()
    );
    assert_eq!(tiny_shell_mcp::contracts::tools().len(), 10);
    for tool in tiny_shell_mcp::contracts::tools() {
        assert_eq!(
            tool.input_schema.get("additionalProperties"),
            Some(&Value::Bool(false))
        );
    }
}
#[test]
fn authentication_is_bound_to_nonce_generation_and_client() -> anyhow::Result<()> {
    let secret = hex::encode([4u8; 32]);
    let challenge = Challenge::new("generation");
    let proof = challenge.proof("client", &secret)?;
    challenge.verify("client", &secret, &proof)?;
    assert!(challenge.verify("other-client", &secret, &proof).is_err());
    assert!(
        Challenge::new("generation")
            .verify("client", &secret, &proof)
            .is_err()
    );
    assert!(
        challenge
            .verify("client", &hex::encode([3u8; 32]), &proof)
            .is_err()
    );
    Ok(())
}
#[test]
fn persisted_policy_defaults_closed_and_corruption_is_not_repaired() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let store = Store::new(directory.path())?;
    assert!(!store.load_policy()?.enabled);
    let mut policy = store.load_policy()?;
    policy.enabled = true;
    store.save_policy(&policy)?;
    assert!(store.load_policy()?.enabled);
    policy.enabled = false;
    store.save_policy(&policy)?;
    assert!(!store.load_policy()?.enabled);
    let client = store.register("test")?;
    assert_eq!(client.secret.len(), 64);
    store.revoke(&client.id)?;
    assert!(!store.client(&client.id)?.enabled);
    assert!(store.client("../policy").is_err());
    std::fs::write(store.root().join("policy.json"), b"broken")?;
    assert!(store.load_policy().is_err());
    assert_eq!(std::fs::read(store.root().join("policy.json"))?, b"broken");
    Ok(())
}
#[tokio::test]
async fn oversized_and_partial_frames_are_rejected() -> anyhow::Result<()> {
    let (mut client, mut server) = tokio::io::duplex(64);
    client.write_u32((MAX_FRAME + 1) as u32).await?;
    assert!(ipc::read_frame::<_, Value>(&mut server).await.is_err());
    let (mut client, mut server) = tokio::io::duplex(64);
    client.write_u32(10).await?;
    client.write_all(b"{}").await?;
    client.shutdown().await?;
    assert!(ipc::read_frame::<_, Value>(&mut server).await.is_err());
    Ok(())
}
struct Echo;
#[async_trait]
impl Handler for Echo {
    async fn call(&self, client: &str, _call: Call) -> tiny_shell_mcp::contracts::Result<Value> {
        Ok(json!({"client":client}))
    }
}
#[tokio::test]
async fn real_local_transport_authenticates_and_has_exclusive_owner() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let store = Store::new(directory.path())?;
    let client = store.register("test")?;
    let listener = ipc::Listener::bind(directory.path())?;
    assert!(ipc::Listener::bind(directory.path()).is_err());
    let task = tokio::spawn(listener.run(store.clone(), Arc::new(Echo)));
    let result = ipc::invoke(directory.path(), &client.id, "get_status", json!({}))
        .await
        .map_err(|e| anyhow::anyhow!(e.code))?;
    assert_eq!(result["data"]["client"], client.id);
    store.revoke(&client.id)?;
    assert!(
        ipc::invoke(directory.path(), &client.id, "get_status", json!({}))
            .await
            .is_err()
    );
    task.abort();
    let _ = task.await;
    Ok(())
}
