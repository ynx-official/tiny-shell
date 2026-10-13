use super::tests::{fixture, provider, request, settled};
use super::*;
use serde_json::json;
use std::time::Duration;
use tokio::{io::AsyncWriteExt, net::TcpListener, sync::mpsc};

/// Exercise the real HTTP/SSE -> runner -> encrypted history and audit path across turns.
#[tokio::test]
async fn usage_across_turns_keeps_missing_separate_from_in_flight_and_reported_zero() -> Result<()>
{
    let directory =
        std::env::temp_dir().join(format!("tiny-shell-ai-test-{}", uuid::Uuid::new_v4()));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let service = fixture(directory.clone(), provider(listener.local_addr()?), None)?;
    let id = service.create(None)?;
    let (started, mut observed) = mpsc::channel(1);
    let (release, mut released) = mpsc::channel(1);
    let server = tokio::spawn(async move {
        for turn in 0..4 {
            let (mut socket, _) = listener.accept().await?;
            let body = request(&mut socket).await?;
            assert_eq!(body["stream_options"]["include_usage"], true);
            let prefix = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {}\n\ndata: {}\n\n",
                json!({"choices":[{"delta":{"content":"fixture reply"}}]}),
                json!({"choices":[{"delta":{},"finish_reason":"stop"}]})
            );
            socket.write_all(prefix.as_bytes()).await?;
            started.send(turn).await?;
            released
                .recv()
                .await
                .ok_or_else(|| anyhow!("fixture stopped"))?;
            if turn == 3 {
                // The fourth request is cancelled before any usage tail arrives.
                continue;
            }
            if turn > 0 {
                let (input, output) = if turn == 1 { (40, 15) } else { (0, 0) };
                let tail = format!(
                    "data: {}\n\n",
                    json!({"choices":[],"usage":{"prompt_tokens":input,"completion_tokens":output,"total_tokens":input+output}})
                );
                // A repeated metadata frame must update this request, never add another charge.
                socket.write_all(tail.as_bytes()).await?;
                socket.write_all(tail.as_bytes()).await?;
            }
            socket.write_all(b"data: [DONE]\n\n").await?;
            if turn == 0 {
                let (mut title, _) = listener.accept().await?;
                let body = request(&mut title).await?;
                assert_eq!(body["stream"], false);
                super::tests::title_response(&mut title, "检查测试用量").await?;
            }
        }
        Ok::<_, anyhow::Error>(())
    });
    for turn in 0..4 {
        service.send(id.clone(), format!("fixture turn {turn}"))?;
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), observed.recv()).await?,
            Some(turn)
        );
        let pending = service.snapshot().conversations[0].usage_summary();
        assert_eq!(pending.requests, turn + 1 + usize::from(turn > 0));
        assert_eq!(
            pending.unknown,
            usize::from(turn > 0),
            "an unfinished request is waiting for usage, not unknown"
        );
        assert_eq!(pending.pending, 1);
        let audit = service.audit.summary(Some(id.clone())).await?;
        assert_eq!(
            (audit.requests, audit.pending, audit.unknown),
            (pending.requests, 1, pending.unknown)
        );
        if turn == 3 {
            service.stop(&id);
            settled(&service).await?;
        }
        release.send(()).await?;
        settled(&service).await?;
        let c = service.snapshot().conversations.remove(0);
        let summary = c.usage_summary();
        assert_eq!(
            summary.requests,
            turn + 2,
            "includes the single background title request"
        );
        assert_eq!(summary.total, if turn == 0 { 5 } else { 60 });
        assert_eq!(summary.unknown, if turn == 3 { 2 } else { 1 });
        assert_eq!(summary.confirmed, turn.min(2) + 1);
        assert_eq!(summary.pending, 0);
        assert_eq!(
            c.requests
                .iter()
                .map(|r| &r.id)
                .collect::<HashSet<_>>()
                .len(),
            turn + 2
        );
        let stored = std::fs::read(directory.join("ai/conversations").join(format!("{id}.enc")))?;
        let loaded: Conversation =
            serde_json::from_slice(&crate::ai::storage::unseal(&[9; 32], &id, &stored)?)?;
        assert_eq!(loaded.usage_summary().unknown, summary.unknown);
        assert_eq!(loaded.usage_summary().total, summary.total);
        assert_eq!(loaded.usage_summary().requests, summary.requests);
        assert_eq!(loaded.usage_summary().pending, 0);
        let audit = service.audit.summary(Some(id.clone())).await?;
        assert_eq!(
            (audit.requests, audit.total, audit.unknown),
            (summary.requests, summary.total, summary.unknown)
        );
    }
    tokio::time::timeout(Duration::from_secs(5), server).await???;
    service.audit.close().await?;
    std::fs::remove_dir_all(directory)?;
    Ok(())
}
