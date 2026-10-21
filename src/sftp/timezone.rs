//! Per-connection display time zone. File timestamps remain untouched.
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use base64::Engine as _;
use chrono::{DateTime, Local, Utc};
use chrono_tz::Tz;

use super::SftpClientHandler;

const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_PROBE_OUTPUT: usize = 4096;
const PROBE_PREFIX: &str = "TINYSHELL_TZ=";

// An explicitly set TZ takes precedence over system settings. Never infer a
// region from `date +%Z` or reuse today's offset for historical file dates.
const UNIX_PROBE: &str = r#"sh -c '
if [ "${TZ+x}" = x ]; then
    printf "TINYSHELL_TZ=%s\n" "${TZ:-UTC}"
    exit 0
fi
zone=$(readlink -f /etc/localtime 2>/dev/null)
case "$zone" in
    */zoneinfo/*) printf "TINYSHELL_TZ=%s\n" "$zone"; exit 0 ;;
esac
zone=$(readlink /etc/localtime 2>/dev/null)
case "$zone" in
    */zoneinfo/*) printf "TINYSHELL_TZ=%s\n" "$zone"; exit 0 ;;
esac
zone=$(timedatectl show --property=Timezone --value 2>/dev/null)
if [ -n "$zone" ]; then
    printf "TINYSHELL_TZ=%s\n" "$zone"
    exit 0
fi
if [ -r /etc/timezone ]; then
    IFS= read -r zone < /etc/timezone
    printf "TINYSHELL_TZ=%s\n" "$zone"
    exit 0
fi
printf "TINYSHELL_TZ=\n"
'"#;

// PowerShell 7/.NET 6+ can resolve Windows IDs to IANA names. Older or
// restricted SSH servers safely fall back to the client's local time zone.
const WINDOWS_PROBE_SCRIPT: &str = "$ErrorActionPreference = 'Stop'; $zone = [TimeZoneInfo]::Local; $id = $zone.Id; if (-not $zone.HasIanaId) { $iana = ''; if (-not [TimeZoneInfo]::TryConvertWindowsIdToIanaId($id, [ref]$iana)) { exit 1 }; $id = $iana }; [Console]::WriteLine('TINYSHELL_TZ=' + $id)";

fn windows_probe_command() -> String {
    // SSH's default shell may itself be PowerShell. EncodedCommand prevents
    // that outer shell from expanding variables before the probe runs.
    let script = WINDOWS_PROBE_SCRIPT
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    format!(
        "pwsh -NoLogo -NoProfile -NonInteractive -EncodedCommand {}",
        base64::engine::general_purpose::STANDARD.encode(script)
    )
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SftpTimeZone {
    #[default]
    Detecting,
    Server(Tz),
    Local,
}

impl SftpTimeZone {
    pub(crate) fn format_mtime(self, timestamp: u32) -> String {
        let Some(time) = DateTime::from_timestamp(i64::from(timestamp), 0) else {
            return "—".to_string();
        };
        match self {
            Self::Server(zone) => time.with_timezone(&zone).format("%Y-%m-%d %H:%M"),
            Self::Detecting | Self::Local => time.with_timezone(&Local).format("%Y-%m-%d %H:%M"),
        }
        .to_string()
    }

    pub(crate) fn zone_label(self, now: DateTime<Utc>) -> String {
        match self {
            Self::Server(zone) => format!(
                "{} (UTC{})",
                zone.name(),
                now.with_timezone(&zone).format("%:z")
            ),
            Self::Detecting | Self::Local => {
                format!("UTC{}", now.with_timezone(&Local).format("%:z"))
            }
        }
    }
}

fn parse_probe_output(output: &[u8]) -> Option<Tz> {
    let output = std::str::from_utf8(output).ok()?;
    let mut values = output
        .lines()
        .filter_map(|line| line.strip_prefix(PROBE_PREFIX));
    let value = values.next()?.trim();
    if values.next().is_some() {
        return None;
    }
    let value = value.strip_prefix(':').unwrap_or(value);
    let value = value
        .split_once("/zoneinfo/")
        .map_or(value, |(_, name)| name);
    // `right/` zoneinfo files use leap seconds, unlike SFTP's Unix timestamps.
    let value = value.strip_prefix("posix/").unwrap_or(value);
    value.parse().ok()
}

/// Called once by the established SFTP connection, never by directory or
/// transfer operations. Each platform probe has a bounded lifetime/output.
pub(super) async fn detect_server_timezone(
    handle: &russh::client::Handle<SftpClientHandler>,
) -> SftpTimeZone {
    match probe_command(handle, UNIX_PROBE).await {
        Ok(output) => parse_probe_output(&output)
            .map(SftpTimeZone::Server)
            .unwrap_or(SftpTimeZone::Local),
        Err(_) => match probe_command(handle, &windows_probe_command()).await {
            Ok(output) => parse_probe_output(&output)
                .map(SftpTimeZone::Server)
                .unwrap_or(SftpTimeZone::Local),
            Err(error) => {
                tracing::debug!(%error, "SFTP time zone detection unavailable; using local time");
                SftpTimeZone::Local
            }
        },
    }
}

async fn probe_command(
    handle: &russh::client::Handle<SftpClientHandler>,
    command: &str,
) -> Result<Vec<u8>> {
    let deadline = tokio::time::Instant::now() + PROBE_TIMEOUT;
    let mut channel = tokio::time::timeout_at(deadline, handle.channel_open_session())
        .await
        .context("time zone channel timeout")?
        .context("open time zone channel")?;
    let result = tokio::time::timeout_at(deadline, async {
        channel.exec(true, command).await?;
        let mut output = Vec::new();
        let mut received = 0;
        let mut status = None;
        while let Some(message) = channel.wait().await {
            match message {
                russh::ChannelMsg::Data { data } => {
                    received += data.len();
                    if received > MAX_PROBE_OUTPUT {
                        return Err(anyhow!("time zone output limit exceeded"));
                    }
                    output.extend_from_slice(&data);
                }
                russh::ChannelMsg::ExtendedData { data, .. } => {
                    received += data.len();
                    if received > MAX_PROBE_OUTPUT {
                        return Err(anyhow!("time zone output limit exceeded"));
                    }
                }
                russh::ChannelMsg::ExitStatus { exit_status } => status = Some(exit_status),
                russh::ChannelMsg::Failure => return Err(anyhow!("time zone command denied")),
                russh::ChannelMsg::Close => break,
                _ => {}
            }
        }
        if status != Some(0) {
            return Err(anyhow!("time zone command did not succeed"));
        }
        Ok(output)
    })
    .await;
    // Close only the probe channel, including on timeout or malformed output.
    let _ = tokio::time::timeout(Duration::from_millis(200), channel.close()).await;
    result.context("time zone command timeout")?
}

#[cfg(test)]
#[path = "timezone_tests.rs"]
mod tests;
