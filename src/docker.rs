use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use anyhow::{Context as _, Result, anyhow};
use serde::Deserialize;
use tokio::io::{AsyncRead, AsyncReadExt as _};

pub(crate) const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const LIST_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const ACTION_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DockerPage {
    Containers,
    Images,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DockerAction {
    Start,
    Stop,
    Restart,
    Remove,
    ForceRemove,
    EnableAutostart,
    DisableAutostart,
}

impl DockerAction {
    pub(crate) fn requires_confirmation(self) -> bool {
        matches!(
            self,
            Self::Stop | Self::Restart | Self::Remove | Self::ForceRemove
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DockerOperation {
    ListContainers,
    ListImages,
    ContainerAction {
        action: DockerAction,
        container_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DockerRequest {
    pub(crate) request_id: u64,
    pub(crate) operation: DockerOperation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DockerPayload {
    Containers(Vec<DockerContainer>),
    Images(Vec<DockerImage>),
    ActionCompleted(DockerAction),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DockerResponse {
    pub(crate) request_id: u64,
    pub(crate) result: Result<DockerPayload, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DockerContainerState {
    Created,
    Running,
    Paused,
    Restarting,
    Removing,
    Exited,
    Dead,
    Unknown(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum DockerRestartPolicy {
    No,
    Always,
    UnlessStopped,
    OnFailure,
    #[default]
    Unknown,
}

impl DockerRestartPolicy {
    pub(crate) fn autostart_enabled(&self) -> bool {
        matches!(self, Self::Always | Self::UnlessStopped)
    }
}

impl DockerContainerState {
    pub(crate) fn actions(&self) -> ContainerActions {
        match self {
            Self::Running => ContainerActions {
                stop: true,
                restart: true,
                ..ContainerActions::default()
            },
            Self::Created | Self::Exited => ContainerActions {
                start: true,
                ..ContainerActions::default()
            },
            Self::Paused | Self::Restarting | Self::Removing | Self::Dead | Self::Unknown(_) => {
                ContainerActions::default()
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ContainerActions {
    pub(crate) start: bool,
    pub(crate) stop: bool,
    pub(crate) restart: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DockerContainer {
    pub(crate) id: String,
    pub(crate) names: String,
    pub(crate) image: String,
    pub(crate) state: DockerContainerState,
    pub(crate) status: String,
    pub(crate) ports: String,
    pub(crate) restart_policy: DockerRestartPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DockerImage {
    pub(crate) id: String,
    pub(crate) repository: String,
    pub(crate) tag: String,
    pub(crate) created_since: String,
    pub(crate) size: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawContainer {
    #[serde(rename = "ID")]
    id: String,
    names: String,
    image: String,
    state: String,
    status: String,
    #[serde(default)]
    ports: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawImage {
    #[serde(rename = "ID")]
    id: String,
    repository: String,
    tag: String,
    created_since: String,
    size: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DockerCommandSpec {
    pub(crate) args: Vec<String>,
    pub(crate) timeout: Duration,
}

pub(crate) fn is_valid_container_id(id: &str) -> bool {
    (12..=64).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn command_spec(operation: &DockerOperation) -> Result<DockerCommandSpec> {
    let (args, timeout) = match operation {
        DockerOperation::ListContainers => (
            ["container", "ls", "--all", "--no-trunc", "--format", "json"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            LIST_TIMEOUT,
        ),
        DockerOperation::ListImages => (
            ["image", "ls", "--no-trunc", "--format", "json"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            LIST_TIMEOUT,
        ),
        DockerOperation::ContainerAction {
            action,
            container_id,
        } => {
            if !is_valid_container_id(container_id) {
                return Err(anyhow!("invalid Docker container ID"));
            }
            let args = match action {
                DockerAction::Start => vec!["container", "start", container_id],
                DockerAction::Stop => vec!["container", "stop", container_id],
                DockerAction::Restart => vec!["container", "restart", container_id],
                DockerAction::Remove => vec!["container", "rm", container_id],
                DockerAction::ForceRemove => {
                    vec!["container", "rm", "--force", container_id]
                }
                DockerAction::EnableAutostart => vec![
                    "container",
                    "update",
                    "--restart",
                    "unless-stopped",
                    container_id,
                ],
                DockerAction::DisableAutostart => {
                    vec!["container", "update", "--restart", "no", container_id]
                }
            };
            (
                args.into_iter().map(str::to_owned).collect(),
                ACTION_TIMEOUT,
            )
        }
    };
    Ok(DockerCommandSpec { args, timeout })
}

pub(crate) fn parse_output(operation: &DockerOperation, output: &str) -> Result<DockerPayload> {
    match operation {
        DockerOperation::ListContainers => output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let raw: RawContainer =
                    serde_json::from_str(line).context("parse Docker container list item")?;
                Ok(DockerContainer {
                    id: raw.id,
                    names: raw.names,
                    image: raw.image,
                    state: parse_container_state(raw.state),
                    status: raw.status,
                    ports: raw.ports,
                    restart_policy: DockerRestartPolicy::Unknown,
                })
            })
            .collect::<Result<Vec<_>>>()
            .map(DockerPayload::Containers),
        DockerOperation::ListImages => output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let raw: RawImage =
                    serde_json::from_str(line).context("parse Docker image list item")?;
                Ok(DockerImage {
                    id: raw.id,
                    repository: raw.repository,
                    tag: raw.tag,
                    created_since: raw.created_since,
                    size: raw.size,
                })
            })
            .collect::<Result<Vec<_>>>()
            .map(DockerPayload::Images),
        DockerOperation::ContainerAction { action, .. } => {
            Ok(DockerPayload::ActionCompleted(*action))
        }
    }
}

pub(crate) fn restart_policy_command_spec(
    containers: &[DockerContainer],
) -> Result<Option<DockerCommandSpec>> {
    if containers.is_empty() {
        return Ok(None);
    }
    if containers
        .iter()
        .any(|container| !is_valid_container_id(&container.id))
    {
        return Err(anyhow!("invalid Docker container ID"));
    }
    let mut args = vec![
        "container".to_string(),
        "inspect".to_string(),
        "--format={{.HostConfig.RestartPolicy.Name}}".to_string(),
    ];
    args.extend(containers.iter().map(|container| container.id.clone()));
    Ok(Some(DockerCommandSpec {
        args,
        timeout: LIST_TIMEOUT,
    }))
}

pub(crate) fn apply_restart_policies(
    containers: &mut [DockerContainer],
    output: &str,
) -> Result<()> {
    let policies = output.lines().map(str::trim).collect::<Vec<_>>();
    if policies.len() != containers.len() {
        return Err(anyhow!(
            "Docker restart policy count did not match container count"
        ));
    }
    for (container, policy) in containers.iter_mut().zip(policies) {
        container.restart_policy = match policy {
            "no" => DockerRestartPolicy::No,
            "always" => DockerRestartPolicy::Always,
            "unless-stopped" => DockerRestartPolicy::UnlessStopped,
            "on-failure" => DockerRestartPolicy::OnFailure,
            _ => DockerRestartPolicy::Unknown,
        };
    }
    Ok(())
}

fn parse_container_state(state: String) -> DockerContainerState {
    match state.as_str() {
        "created" => DockerContainerState::Created,
        "running" => DockerContainerState::Running,
        "paused" => DockerContainerState::Paused,
        "restarting" => DockerContainerState::Restarting,
        "removing" => DockerContainerState::Removing,
        "exited" => DockerContainerState::Exited,
        "dead" => DockerContainerState::Dead,
        _ => DockerContainerState::Unknown(state),
    }
}

pub(crate) fn shell_command_from_spec(spec: &DockerCommandSpec) -> String {
    format!("docker {}", spec.args.join(" "))
}

pub(crate) async fn execute_local(request: DockerRequest) -> DockerResponse {
    let result = async {
        let timeout = command_spec(&request.operation)?.timeout;
        tokio::time::timeout(
            timeout,
            execute_with_runner(&request.operation, &LocalDockerRunner),
        )
        .await
        .map_err(|_| anyhow!("Docker request timed out after {}s", timeout.as_secs()))?
    }
    .await
    .map_err(|error: anyhow::Error| format!("{error:#}"));
    DockerResponse {
        request_id: request.request_id,
        result,
    }
}

struct DockerCommandOutput {
    success: bool,
    status: String,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[async_trait::async_trait]
trait DockerCommandRunner: Sync {
    async fn run(&self, spec: &DockerCommandSpec) -> Result<DockerCommandOutput>;
}

struct LocalDockerRunner;

#[async_trait::async_trait]
impl DockerCommandRunner for LocalDockerRunner {
    async fn run(&self, spec: &DockerCommandSpec) -> Result<DockerCommandOutput> {
        run_local_command(spec).await
    }
}

async fn execute_with_runner(
    operation: &DockerOperation,
    runner: &dyn DockerCommandRunner,
) -> Result<DockerPayload> {
    let spec = command_spec(operation)?;
    let output = run_checked(runner, &spec).await?;
    let mut payload = parse_output(operation, &String::from_utf8_lossy(&output.stdout))?;
    if let DockerPayload::Containers(containers) = &mut payload
        && let Some(spec) = restart_policy_command_spec(containers)?
    {
        let output = run_checked(runner, &spec).await?;
        apply_restart_policies(containers, &String::from_utf8_lossy(&output.stdout))?;
    }
    Ok(payload)
}

async fn run_checked(
    runner: &dyn DockerCommandRunner,
    spec: &DockerCommandSpec,
) -> Result<DockerCommandOutput> {
    let output = runner.run(spec).await?;
    if !output.success {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if message.is_empty() {
            anyhow!("Docker command exited with {}", output.status)
        } else {
            anyhow!(message)
        });
    }
    Ok(output)
}

async fn run_local_command(spec: &DockerCommandSpec) -> Result<DockerCommandOutput> {
    let mut command = tokio::process::Command::new("docker");
    command
        .args(&spec.args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().context("start Docker CLI")?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("Docker stdout was not captured"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("Docker stderr was not captured"))?;

    let bytes_read = Arc::new(AtomicUsize::new(0));
    let collect = async {
        let (stdout, stderr, status) = tokio::try_join!(
            read_limited(stdout, bytes_read.clone()),
            read_limited(stderr, bytes_read),
            async { child.wait().await.context("wait for Docker CLI") }
        )?;
        Ok::<_, anyhow::Error>((stdout, stderr, status))
    };
    let (stdout, stderr, status) = tokio::time::timeout(spec.timeout, collect)
        .await
        .map_err(|_| anyhow!("Docker command timed out after {}s", spec.timeout.as_secs()))??;
    Ok(DockerCommandOutput {
        success: status.success(),
        status: status.to_string(),
        stdout,
        stderr,
    })
}

async fn read_limited(
    mut reader: impl AsyncRead + Unpin,
    bytes_read: Arc<AtomicUsize>,
) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader
            .read(&mut buffer)
            .await
            .context("read Docker output")?;
        if read == 0 {
            return Ok(output);
        }
        let previous = bytes_read.fetch_add(read, Ordering::Relaxed);
        if previous.saturating_add(read) > MAX_OUTPUT_BYTES {
            return Err(anyhow!(
                "Docker command output exceeded {} bytes",
                MAX_OUTPUT_BYTES
            ));
        }
        output.extend_from_slice(&buffer[..read]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeRunner {
        output: String,
        success: bool,
    }

    #[async_trait::async_trait]
    impl DockerCommandRunner for FakeRunner {
        async fn run(&self, _spec: &DockerCommandSpec) -> Result<DockerCommandOutput> {
            Ok(DockerCommandOutput {
                success: self.success,
                status: if self.success { "0" } else { "1" }.into(),
                stdout: self.output.as_bytes().to_vec(),
                stderr: if self.success {
                    Vec::new()
                } else {
                    b"fake Docker failure".to_vec()
                },
            })
        }
    }

    #[test]
    fn parses_container_json_lines_and_ignores_unknown_fields() {
        let output = concat!(
            r#"{"Command":"nginx","ID":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","Image":"nginx:latest","Names":"web","Ports":"0.0.0.0:80->80/tcp","State":"running","Status":"Up 2 hours","Future":"ok"}"#,
            "\n",
            r#"{"ID":"abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd","Image":"mysql:8","Names":"db","State":"exited","Status":"Exited (0) 1 hour ago"}"#,
            "\n"
        );

        let DockerPayload::Containers(containers) =
            parse_output(&DockerOperation::ListContainers, output).unwrap()
        else {
            panic!("expected containers");
        };
        assert_eq!(containers.len(), 2);
        assert_eq!(containers[0].names, "web");
        assert_eq!(containers[0].state, DockerContainerState::Running);
        assert_eq!(containers[1].ports, "");
        assert_eq!(containers[1].state, DockerContainerState::Exited);
    }

    #[test]
    fn parses_images_and_rejects_malformed_lines() {
        let output = concat!(
            r#"{"ID":"sha256:abc","Repository":"ubuntu","Tag":"latest","CreatedSince":"5 days ago","Size":"72.9MB","Unknown":1}"#,
            "\n"
        );
        let DockerPayload::Images(images) =
            parse_output(&DockerOperation::ListImages, output).unwrap()
        else {
            panic!("expected images");
        };
        assert_eq!(images[0].repository, "ubuntu");
        assert!(parse_output(&DockerOperation::ListImages, "not-json\n").is_err());
    }

    #[test]
    fn empty_output_produces_empty_lists() {
        assert_eq!(
            parse_output(&DockerOperation::ListContainers, "\n").unwrap(),
            DockerPayload::Containers(Vec::new())
        );
        assert_eq!(
            parse_output(&DockerOperation::ListImages, "").unwrap(),
            DockerPayload::Images(Vec::new())
        );
    }

    #[test]
    fn maps_states_to_safe_v1_actions() {
        assert_eq!(
            DockerContainerState::Running.actions(),
            ContainerActions {
                stop: true,
                restart: true,
                ..Default::default()
            }
        );
        for state in [DockerContainerState::Created, DockerContainerState::Exited] {
            assert_eq!(
                state.actions(),
                ContainerActions {
                    start: true,
                    ..Default::default()
                }
            );
        }
        for state in [
            DockerContainerState::Paused,
            DockerContainerState::Restarting,
            DockerContainerState::Removing,
            DockerContainerState::Dead,
        ] {
            assert_eq!(state.actions(), ContainerActions::default());
        }
    }

    #[test]
    fn validates_ids_and_builds_fixed_commands() {
        let id = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert!(is_valid_container_id(id));
        assert!(!is_valid_container_id("web; rm -rf /"));
        assert!(!is_valid_container_id("abc"));

        let spec = command_spec(&DockerOperation::ContainerAction {
            action: DockerAction::Restart,
            container_id: id.into(),
        })
        .unwrap();
        assert_eq!(spec.args, ["container", "restart", id]);
        assert_eq!(spec.timeout, ACTION_TIMEOUT);
        let remove = command_spec(&DockerOperation::ContainerAction {
            action: DockerAction::Remove,
            container_id: id.into(),
        })
        .unwrap();
        assert_eq!(remove.args, ["container", "rm", id]);
        let force_remove = command_spec(&DockerOperation::ContainerAction {
            action: DockerAction::ForceRemove,
            container_id: id.into(),
        })
        .unwrap();
        assert_eq!(force_remove.args, ["container", "rm", "--force", id]);
        let enable_autostart = command_spec(&DockerOperation::ContainerAction {
            action: DockerAction::EnableAutostart,
            container_id: id.into(),
        })
        .unwrap();
        assert_eq!(
            enable_autostart.args,
            ["container", "update", "--restart", "unless-stopped", id]
        );
        let disable_autostart = command_spec(&DockerOperation::ContainerAction {
            action: DockerAction::DisableAutostart,
            container_id: id.into(),
        })
        .unwrap();
        assert_eq!(
            disable_autostart.args,
            ["container", "update", "--restart", "no", id]
        );
        assert!(
            command_spec(&DockerOperation::ContainerAction {
                action: DockerAction::Stop,
                container_id: "bad".into(),
            })
            .is_err()
        );
        assert!(!DockerAction::Start.requires_confirmation());
        assert!(DockerAction::Stop.requires_confirmation());
        assert!(DockerAction::Restart.requires_confirmation());
        assert!(DockerAction::Remove.requires_confirmation());
        assert!(DockerAction::ForceRemove.requires_confirmation());
        assert!(!DockerAction::EnableAutostart.requires_confirmation());
        assert!(!DockerAction::DisableAutostart.requires_confirmation());
    }

    #[test]
    fn restart_policy_inspection_is_batched_and_controls_menu_availability() {
        let first = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let second = "abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd";
        let mut containers = match parse_output(
            &DockerOperation::ListContainers,
            &format!(
                "{{\"ID\":\"{first}\",\"Image\":\"nginx\",\"Names\":\"web\",\"State\":\"running\",\"Status\":\"Up\"}}\n{{\"ID\":\"{second}\",\"Image\":\"redis\",\"Names\":\"cache\",\"State\":\"exited\",\"Status\":\"Exited\"}}\n"
            ),
        )
        .unwrap()
        {
            DockerPayload::Containers(containers) => containers,
            _ => panic!("expected containers"),
        };

        let spec = restart_policy_command_spec(&containers).unwrap().unwrap();
        assert_eq!(
            spec.args,
            [
                "container",
                "inspect",
                "--format={{.HostConfig.RestartPolicy.Name}}",
                first,
                second,
            ]
        );
        apply_restart_policies(&mut containers, "unless-stopped\nno\n").unwrap();
        assert!(containers[0].restart_policy.autostart_enabled());
        assert!(!containers[1].restart_policy.autostart_enabled());
    }

    #[test]
    fn restart_policy_parser_rejects_incomplete_inspect_output() {
        let mut containers = match parse_output(
            &DockerOperation::ListContainers,
            r#"{"ID":"0123456789abcdef","Image":"nginx","Names":"web","State":"running","Status":"Up"}"#,
        )
        .unwrap()
        {
            DockerPayload::Containers(containers) => containers,
            _ => panic!("expected containers"),
        };
        assert!(apply_restart_policies(&mut containers, "").is_err());
    }

    #[tokio::test]
    async fn fake_runner_covers_read_and_action_results_without_touching_docker() {
        let list_runner = FakeRunner {
            output: r#"{"ID":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","Image":"nginx","Names":"web","State":"running","Status":"Up"}"#.into(),
            success: true,
        };
        let DockerPayload::Containers(containers) =
            execute_with_runner(&DockerOperation::ListContainers, &list_runner)
                .await
                .unwrap()
        else {
            panic!("expected containers");
        };
        assert_eq!(containers.len(), 1);

        let action_runner = FakeRunner {
            output: String::new(),
            success: true,
        };
        let result = execute_with_runner(
            &DockerOperation::ContainerAction {
                action: DockerAction::Start,
                container_id: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .into(),
            },
            &action_runner,
        )
        .await
        .unwrap();
        assert_eq!(result, DockerPayload::ActionCompleted(DockerAction::Start));
    }

    #[tokio::test]
    async fn output_reader_enforces_the_combined_limit() {
        let bytes_read = Arc::new(AtomicUsize::new(0));
        let reader = tokio::io::repeat(0).take((MAX_OUTPUT_BYTES + 1) as u64);
        assert!(read_limited(reader, bytes_read).await.is_err());
    }
}
