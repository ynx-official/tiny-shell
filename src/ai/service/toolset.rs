use serde_json::{Value, json};
use tiny_shell_mcp::contracts::{Call, Failure, Result};
use tiny_shell_mcp::policy::Mode;

pub(super) fn tools(mode: Mode) -> Vec<Value> {
    tiny_shell_mcp::contracts::tools().into_iter().filter_map(|tool| {
        let description = match tool.name.as_ref() {
            "run_command" if mode != Mode::ReadOnly => "Execute one non-interactive command on the bound SSH connection and return its completed result. Controlled mode waits for user approval. Never repeat an unknown outcome.",
            "write_file" if mode != Mode::ReadOnly => "Create or update text on the bound SSH connection and return the completed result. Updates require the version from read_file. Controlled mode waits for user approval.",
            "read_file" => "Read text and its version from the bound SSH connection. Large content is returned as an explicitly marked preview while preserving version metadata.",
            "list_directory" => "List entries from the bound SSH connection using SFTP. Use next_offset for the next page.",
            _ => return None,
        };
        let mut schema = serde_json::to_value(tool.input_schema).ok()?;
        let owned = ["connection_id","session_id","task_id","request_id","cwd"];
        if let Some(properties) = schema["properties"].as_object_mut() {
            for key in owned { properties.remove(key); }
        }
        if let Some(required) = schema["required"].as_array_mut() {
            required.retain(|field| !owned.contains(&field.as_str().unwrap_or_default()));
        }
        Some(json!({"type":"function","function":{"name":tool.name,"description":description,"parameters":schema}}))
    }).collect()
}

pub(super) fn parse(name: &str, args: Value, mode: Mode) -> Result<Call> {
    let fields: &[&str] = match name {
        "run_command" if mode != Mode::ReadOnly => &["command", "timeout_ms"],
        "write_file" if mode != Mode::ReadOnly => &["path", "content", "mode", "expected_version"],
        "read_file" => &["path"],
        "list_directory" => &["path", "offset", "limit"],
        _ => return Err(Failure::new("INVALID_ARGUMENT")),
    };
    let mut args = args
        .as_object()
        .cloned()
        .ok_or_else(|| Failure::new("INVALID_ARGUMENT"))?;
    if args.keys().any(|key| !fields.contains(&key.as_str())) {
        return Err(Failure::new("INVALID_ARGUMENT"));
    }
    args.insert("session_id".into(), uuid::Uuid::nil().to_string().into());
    if matches!(name, "run_command" | "write_file") {
        args.insert("request_id".into(), uuid::Uuid::nil().to_string().into());
    }
    Call::parse(name, args.into())
}
pub(super) fn bind(call: &mut Call, session: &str) {
    match call {
        Call::RunCommand(p) => {
            p.session_id = session.into();
            p.request_id = uuid::Uuid::new_v4().to_string();
        }
        Call::WriteFile(p) => {
            p.session_id = session.into();
            p.request_id = uuid::Uuid::new_v4().to_string();
        }
        Call::ReadFile(p) => p.session_id = session.into(),
        Call::ListDirectory(p) => p.session_id = session.into(),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn internal_tools_hide_transport_lifecycle_and_application_owned_identifiers() {
        let tools = tools(Mode::Full);
        assert_eq!(tools.len(), 4);
        for tool in tools {
            let function = &tool["function"];
            assert!(
                ["run_command", "list_directory", "read_file", "write_file"]
                    .contains(&function["name"].as_str().unwrap())
            );
            assert!(!function["description"].as_str().unwrap().contains("poll"));
            for field in [
                "connection_id",
                "session_id",
                "task_id",
                "request_id",
                "cwd",
            ] {
                assert!(
                    function["parameters"]["properties"].get(field).is_none(),
                    "application-owned {field} exposed"
                );
            }
        }
    }
    #[test]
    fn read_only_toolset_excludes_commands_and_writes() {
        let tools = tools(Mode::ReadOnly);
        assert_eq!(tools.len(), 2);
        assert!(tools.iter().all(|t| {
            ["read_file", "list_directory"].contains(&t["function"]["name"].as_str().unwrap())
        }));
    }
    #[test]
    fn operation_parser_enforces_mode_and_existing_argument_constraints() {
        assert!(parse("run_command", json!({"command":"true"}), Mode::ReadOnly).is_err());
        assert!(
            parse(
                "write_file",
                json!({"path":"/file","content":"x","mode":"create"}),
                Mode::ReadOnly
            )
            .is_err()
        );
        assert!(parse("get_task", json!({}), Mode::Full).is_err());
        assert!(parse("run_command", json!({"command":""}), Mode::Full).is_err());
        assert!(parse("read_file", json!({"path":"relative"}), Mode::Full).is_err());
        assert!(
            parse(
                "read_file",
                json!({"path":"/file","session_id":"foreign"}),
                Mode::Full
            )
            .is_err()
        );
        let mut call = parse("run_command", json!({"command":"true"}), Mode::Full).unwrap();
        let session = uuid::Uuid::new_v4().to_string();
        bind(&mut call, &session);
        let Call::RunCommand(call) = call else {
            panic!("wrong call")
        };
        assert_eq!(call.session_id, session);
        assert_ne!(call.request_id, uuid::Uuid::nil().to_string());
    }
}
