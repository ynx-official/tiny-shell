use anyhow::{Result, anyhow};
use std::path::Path;

/// Adapt the same authenticated bridge command to a client's configuration schema.
pub fn format_client_config(label: &str, config: &str) -> Result<String> {
    if !matches!(label, "Codex CLI" | "OpenCode") {
        return Ok(config.to_owned());
    }
    let value: serde_json::Value = serde_json::from_str(config)?;
    let server = &value["mcpServers"]["tiny-shell"];
    let command = server["command"]
        .as_str()
        .ok_or_else(|| anyhow!("bridge command missing"))?;
    let args = server["args"]
        .as_array()
        .filter(|args| args.iter().all(serde_json::Value::is_string))
        .ok_or_else(|| anyhow!("bridge arguments missing"))?;
    if label == "Codex CLI" {
        // JSON string escaping is compatible with TOML basic strings and keeps
        // Windows backslashes, quotes and Unicode paths intact.
        Ok(format!(
            "[mcp_servers.tiny-shell]\ncommand = {}\nargs = {}\n",
            serde_json::to_string(command)?,
            serde_json::to_string(args)?
        ))
    } else {
        let mut invocation = vec![serde_json::Value::String(command.to_owned())];
        invocation.extend(args.iter().cloned());
        Ok(serde_json::to_string_pretty(
            &serde_json::json!({"mcp":{"tiny-shell":{"type":"local","command":invocation,"enabled":true}}}),
        )?)
    }
}

/// Export a bridge command without putting the registration secret in client configuration.
pub fn client_config(executable: &Path, appimage: Option<&Path>, id: &str) -> Result<String> {
    let parent = executable
        .parent()
        .ok_or_else(|| anyhow!("helper directory unavailable"))?;
    let helper = parent.join(if cfg!(windows) {
        "tiny-shell-mcp.exe"
    } else {
        "tiny-shell-mcp"
    });
    let mut args = vec!["--client".to_owned(), id.to_owned()];
    let command = if let Some(appimage) = appimage {
        args.insert(0, "--mcp".to_owned());
        appimage.to_path_buf()
    } else if helper.is_file() {
        helper
    } else {
        // `cargo run` builds only the application. Its stdio entry shares the
        // same authenticated bridge and must not require a second binary.
        args.insert(0, "--mcp".to_owned());
        executable.to_path_buf()
    };
    Ok(serde_json::to_string_pretty(
        &serde_json::json!({"mcpServers":{"tiny-shell":{"command":command,"args":args}}}),
    )?)
}

#[cfg(test)]
mod tests {
    use super::client_config;
    use serde_json::{Value, json};

    const CLIENT_ID: &str = "31d8c804-22ab-4c6b-88fc-6ecbdbf58bb6";

    fn configuration() -> String {
        json!({"mcpServers":{"tiny-shell":{"command":"C:\\Tiny Shell\\终端.exe","args":["--mcp","--client",CLIENT_ID]}}}).to_string()
    }

    #[test]
    fn codex_configuration_is_toml_and_preserves_escaped_paths() -> anyhow::Result<()> {
        let text = super::format_client_config("Codex CLI", &configuration())?;
        assert!(text.starts_with("[mcp_servers.tiny-shell]\n"));
        assert!(text.contains("command = \"C:\\\\Tiny Shell\\\\终端.exe\""));
        assert!(text.contains(&format!("args = [\"--mcp\",\"--client\",\"{CLIENT_ID}\"]")));
        assert!(!text.contains("secret"));
        Ok(())
    }

    #[test]
    fn opencode_configuration_includes_executable_in_command_array() -> anyhow::Result<()> {
        let text = super::format_client_config("OpenCode", &configuration())?;
        let parsed: Value = serde_json::from_str(&text)?;
        assert_eq!(parsed["mcp"]["tiny-shell"]["type"], "local");
        assert_eq!(
            parsed["mcp"]["tiny-shell"]["command"],
            json!(["C:\\Tiny Shell\\终端.exe", "--mcp", "--client", CLIENT_ID])
        );
        assert_eq!(parsed["mcp"]["tiny-shell"]["enabled"], true);
        assert!(parsed.get("mcpServers").is_none());
        Ok(())
    }

    #[test]
    fn claude_and_generic_clients_keep_the_original_schema() -> anyhow::Result<()> {
        for label in ["Claude Code", "MCP Client"] {
            assert_eq!(
                super::format_client_config(label, &configuration())?,
                configuration()
            );
        }
        Ok(())
    }

    #[test]
    fn cargo_run_without_a_sibling_helper_exports_the_application_entry() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let executable = directory.path().join("Tiny Shell 开发程序.exe");
        std::fs::write(&executable, [])?;
        let config: Value = serde_json::from_str(&client_config(&executable, None, CLIENT_ID)?)?;
        assert_eq!(
            config["mcpServers"]["tiny-shell"]["command"],
            json!(executable)
        );
        assert_eq!(
            config["mcpServers"]["tiny-shell"]["args"],
            json!(["--mcp", "--client", CLIENT_ID])
        );
        assert!(config.to_string().find("secret").is_none());
        Ok(())
    }

    #[test]
    fn complete_package_keeps_the_standalone_helper_entry() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let executable = directory.path().join("tiny-shell.exe");
        let helper = directory.path().join(if cfg!(windows) {
            "tiny-shell-mcp.exe"
        } else {
            "tiny-shell-mcp"
        });
        std::fs::write(&executable, [])?;
        std::fs::write(&helper, [])?;
        let config: Value = serde_json::from_str(&client_config(&executable, None, CLIENT_ID)?)?;
        assert_eq!(config["mcpServers"]["tiny-shell"]["command"], json!(helper));
        assert_eq!(
            config["mcpServers"]["tiny-shell"]["args"],
            json!(["--client", CLIENT_ID])
        );
        Ok(())
    }

    #[test]
    fn appimage_entry_does_not_require_a_sibling_helper() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let executable = directory.path().join("usr/bin/tiny-shell");
        std::fs::create_dir_all(
            executable
                .parent()
                .ok_or_else(|| anyhow::anyhow!("parent missing"))?,
        )?;
        std::fs::write(&executable, [])?;
        let appimage = directory.path().join("Tiny Shell.AppImage");
        std::fs::write(&appimage, [])?;
        let config: Value =
            serde_json::from_str(&client_config(&executable, Some(&appimage), CLIENT_ID)?)?;
        assert_eq!(
            config["mcpServers"]["tiny-shell"]["command"],
            json!(appimage)
        );
        assert_eq!(
            config["mcpServers"]["tiny-shell"]["args"],
            json!(["--mcp", "--client", CLIENT_ID])
        );
        Ok(())
    }
}
