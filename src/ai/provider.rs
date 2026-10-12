//! OpenAI-compatible HTTP and streaming adapter.
use super::{Message, Provider};
use anyhow::{Result, anyhow, ensure};
use futures::StreamExt;
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};
use tokio::sync::watch;

fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|_| anyhow!("AI_NETWORK_ERROR"))
}
fn authorized(request: reqwest::RequestBuilder, key: &str) -> reqwest::RequestBuilder {
    if key.is_empty() {
        request
    } else {
        request.bearer_auth(key)
    }
}
async fn json_response(response: reqwest::Response) -> Result<Value> {
    ensure!(
        response.status().is_success(),
        "AI_HTTP_{}",
        response.status().as_u16()
    );
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        bytes.extend(chunk.map_err(|_| anyhow!("AI_NETWORK_ERROR"))?);
        ensure!(bytes.len() <= 1024 * 1024, "AI_RESPONSE_LIMIT");
    }
    serde_json::from_slice(&bytes).map_err(|_| anyhow!("AI_BAD_RESPONSE"))
}
pub(super) async fn models(p: &Provider, key: &str) -> Result<Vec<String>> {
    ensure!(!key.is_empty() || p.allow_no_key, "AI_KEY_REQUIRED");
    let response = authorized(client()?.get(p.endpoint("models")?), key)
        .send()
        .await
        .map_err(|_| anyhow!("AI_NETWORK_ERROR"))?;
    let data = json_response(response).await?;
    let list = data["data"]
        .as_array()
        .ok_or_else(|| anyhow!("AI_BAD_RESPONSE"))?;
    let mut ids = list
        .iter()
        .filter_map(|v| v["id"].as_str())
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .take(2000)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    Ok(ids)
}
pub(super) async fn test(p: &Provider, key: &str) -> Result<()> {
    ensure!(!key.is_empty() || p.allow_no_key, "AI_KEY_REQUIRED");
    ensure!(!p.default_model.is_empty(), "AI_NO_MODEL");
    let response=authorized(client()?.post(p.endpoint("chat/completions")?),key).json(&json!({"model":p.default_model,"messages":[{"role":"user","content":"Reply OK."}],"stream":false,"max_tokens":8})).send().await.map_err(|_|anyhow!("AI_NETWORK_ERROR"))?;
    let value = json_response(response).await?;
    ensure!(
        value["choices"][0]["message"].is_object(),
        "AI_BAD_RESPONSE"
    );
    Ok(())
}
fn json_message(value: &Value) -> Result<Message> {
    let message = &value["choices"][0]["message"];
    let result = Message {
        role: "assistant".into(),
        content: message["content"].as_str().unwrap_or_default().into(),
        tool_calls: message["tool_calls"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
        tool_call_id: None,
    };
    validate_message(&result, value["choices"][0]["finish_reason"].as_str())?;
    Ok(result)
}
fn validate_message(message: &Message, finish: Option<&str>) -> Result<()> {
    ensure!(
        !matches!(finish, Some("length" | "content_filter")),
        "AI_RESPONSE_INCOMPLETE"
    );
    ensure!(
        !message.content.is_empty() || !message.tool_calls.is_empty(),
        "AI_EMPTY_RESPONSE"
    );
    ensure!(message.tool_calls.len() <= 32, "AI_TOOL_LIMIT");
    if !message.tool_calls.is_empty() {
        ensure!(finish == Some("tool_calls"), "AI_INVALID_TOOL");
        let mut ids = std::collections::HashSet::new();
        for call in &message.tool_calls {
            let id = call["id"]
                .as_str()
                .ok_or_else(|| anyhow!("AI_INVALID_TOOL"))?;
            let name = call["function"]["name"]
                .as_str()
                .ok_or_else(|| anyhow!("AI_INVALID_TOOL"))?;
            let args = call["function"]["arguments"]
                .as_str()
                .ok_or_else(|| anyhow!("AI_INVALID_TOOL"))?;
            ensure!(
                !id.is_empty()
                    && id.len() <= 256
                    && !id.chars().any(char::is_control)
                    && ids.insert(id),
                "AI_INVALID_TOOL"
            );
            ensure!(
                call["type"] == "function"
                    && !name.is_empty()
                    && name.len() <= 64
                    && args.len() <= 256 * 1024,
                "AI_INVALID_TOOL"
            );
            ensure!(
                serde_json::from_str::<Value>(args).is_ok_and(|v| v.is_object()),
                "AI_INVALID_TOOL"
            );
        }
    }
    ensure!(
        message.content.len()
            + message
                .tool_calls
                .iter()
                .map(|v| v.to_string().len())
                .sum::<usize>()
            <= 1024 * 1024,
        "AI_RESPONSE_LIMIT"
    );
    Ok(())
}
#[derive(Default)]
struct Stream {
    bytes: Vec<u8>,
    content: String,
    calls: BTreeMap<usize, Value>,
    finish: Option<String>,
    done: bool,
}
impl Stream {
    fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>> {
        self.bytes.extend_from_slice(chunk);
        ensure!(self.bytes.len() <= 1024 * 1024, "AI_RESPONSE_LIMIT");
        let mut deltas = Vec::new();
        while let Some(end) = self.bytes.iter().position(|&b| b == b'\n') {
            let line = self.bytes.drain(..=end).collect::<Vec<_>>();
            let line = std::str::from_utf8(&line)
                .map_err(|_| anyhow!("AI_BAD_RESPONSE"))?
                .trim();
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                self.done = true;
                continue;
            }
            let value: Value =
                serde_json::from_str(data).map_err(|_| anyhow!("AI_BAD_RESPONSE"))?;
            let choice = &value["choices"][0];
            if let Some(reason) = choice["finish_reason"].as_str() {
                self.finish = Some(reason.into());
            }
            if let Some(content) = choice["delta"]["content"].as_str() {
                self.content.push_str(content);
                deltas.push(content.into());
                ensure!(self.content.len() <= 1024 * 1024, "AI_RESPONSE_LIMIT");
            }
            if let Some(calls) = choice["delta"]["tool_calls"].as_array() {
                for delta in calls {
                    let index = delta["index"]
                        .as_u64()
                        .ok_or_else(|| anyhow!("AI_INVALID_TOOL"))?
                        as usize;
                    ensure!(index < 32, "AI_TOOL_LIMIT");
                    let call = self.calls.entry(index).or_insert_with(
                        || json!({"id":"","type":"function","function":{"name":"","arguments":""}}),
                    );
                    for (field, path) in [("id", false), ("name", true), ("arguments", true)] {
                        let part = if path {
                            delta["function"][field].as_str()
                        } else {
                            delta[field].as_str()
                        };
                        if let Some(part) = part {
                            let destination = if path {
                                &mut call["function"][field]
                            } else {
                                &mut call[field]
                            };
                            let mut text = destination.as_str().unwrap_or_default().to_owned();
                            text.push_str(part);
                            ensure!(text.len() <= 256 * 1024, "AI_RESPONSE_LIMIT");
                            *destination = text.into();
                        }
                    }
                }
            }
        }
        Ok(deltas)
    }
    fn finish(self) -> Result<Message> {
        ensure!(self.done || self.finish.is_some(), "AI_STREAM_INTERRUPTED");
        let message = Message {
            role: "assistant".into(),
            content: self.content,
            tool_calls: self.calls.into_values().collect(),
            tool_call_id: None,
        };
        validate_message(&message, self.finish.as_deref())?;
        Ok(message)
    }
}
pub(super) async fn complete(
    p: &Provider,
    model: &str,
    key: &str,
    messages: &[Value],
    tools: &[Value],
    cancel: &mut watch::Receiver<bool>,
    mut delta: impl FnMut(String),
) -> Result<Message> {
    ensure!(!*cancel.borrow(), "AI_CANCELLED");
    let mut body = json!({"model":model,"messages":messages,"stream":true});
    if !tools.is_empty() {
        body["tools"] = tools.to_vec().into();
    }
    let send = authorized(client()?.post(p.endpoint("chat/completions")?), key)
        .json(&body)
        .send();
    let response = tokio::select! {result=send=>result.map_err(|_|anyhow!("AI_NETWORK_ERROR"))?,_=cancel.changed()=>return Err(anyhow!("AI_CANCELLED"))};
    ensure!(
        response.status().is_success(),
        "AI_HTTP_{}",
        response.status().as_u16()
    );
    if response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"))
    {
        let value = tokio::select! { result = json_response(response) => result?, _ = cancel.changed() => return Err(anyhow!("AI_CANCELLED")) };
        let result = json_message(&value)?;
        delta(result.content.clone());
        return Ok(result);
    }
    let mut stream = response.bytes_stream();
    let mut parser = Stream::default();
    loop {
        let next = tokio::select! {_=cancel.changed()=>return Err(anyhow!("AI_CANCELLED")),next=tokio::time::timeout(Duration::from_secs(60),stream.next())=>next.map_err(|_|anyhow!("AI_TIMEOUT"))?};
        let Some(chunk) = next else {
            break;
        };
        for text in parser.push(&chunk.map_err(|_| anyhow!("AI_STREAM_INTERRUPTED"))?)? {
            delta(text);
        }
        if parser.done {
            break;
        }
    }
    parser.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn json_fallback_does_not_accept_incomplete_or_invalid_tools() {
        let valid_call = json!({"id":"call-1","type":"function","function":{"name":"open_session","arguments":"{}"}});
        let response = |calls: Vec<Value>, reason: &str| json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":calls},"finish_reason":reason}]});
        assert!(json_message(&response(vec![valid_call.clone()], "tool_calls")).is_ok());
        assert!(json_message(&response(vec![valid_call.clone()], "length")).is_err());
        assert!(
            json_message(&response(
                vec![valid_call.clone(), valid_call.clone()],
                "tool_calls"
            ))
            .is_err()
        );
        let mut broken = valid_call;
        broken["function"]["arguments"] = "{\"path\":".into();
        assert!(json_message(&response(vec![broken], "tool_calls")).is_err());
    }
    #[test]
    fn split_utf8_and_arguments_are_complete_before_use() {
        let input = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"你好\"}}]}\r\n\r\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call1\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"/a\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n"
        );
        let mut stream = Stream::default();
        for byte in input.as_bytes() {
            stream.push(&[*byte]).unwrap();
        }
        let message = stream.finish().unwrap();
        assert_eq!(message.content, "你好");
        assert_eq!(
            message.tool_calls[0]["function"]["arguments"],
            "{\"path\":\"/a\"}"
        );
    }
    #[test]
    fn broken_stream_does_not_execute_partial_tools() {
        let mut stream = Stream::default();
        stream
            .push(b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n")
            .unwrap();
        assert!(stream.finish().is_err());
    }
}
