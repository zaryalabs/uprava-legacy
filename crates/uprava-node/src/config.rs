use std::{
    io::Read,
    path::PathBuf,
    process::Stdio,
    time::{Duration, Instant},
};

use anyhow::Context;
use reqwest::Url;

use super::default_state_path;

/// Validated Node daemon process configuration.
#[derive(Debug, Clone)]
pub(crate) struct NodeConfig {
    pub(crate) core_url: Url,
    pub(crate) display_name: String,
    pub(crate) heartbeat_interval: Duration,
    pub(crate) state_path: PathBuf,
    pub(crate) workspace_paths: Vec<PathBuf>,
    pub(crate) codex_binary: String,
    pub(crate) codex_version: Option<String>,
    pub(crate) codex_managed_unavailable_reason: Option<String>,
    pub(crate) codex_ignore_user_config: bool,
    pub(crate) codex_timeout: Duration,
    pub(crate) opensandbox_url: Option<Url>,
    pub(crate) task_runtime_image: String,
    pub(crate) toolhive_url: Url,
    pub(crate) toolhive_timeout: Duration,
}

impl NodeConfig {
    /// Load configuration and reject an empty workspace allow-list.
    pub(crate) fn from_env() -> anyhow::Result<Self> {
        let core_url = std::env::var("UPRAVA_CORE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8080".to_owned())
            .parse::<Url>()
            .context("UPRAVA_CORE_URL must be a valid URL")?;
        let display_name =
            std::env::var("UPRAVA_NODE_DISPLAY_NAME").unwrap_or_else(|_| "Local Node".to_owned());
        let heartbeat_interval = parse_env_duration_seconds("UPRAVA_NODE_HEARTBEAT_SECONDS", 5)?;
        let state_path = std::env::var("UPRAVA_NODE_STATE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| default_state_path());
        let workspace_paths = parse_workspace_paths()?;
        let codex_binary =
            std::env::var("UPRAVA_CODEX_BINARY").unwrap_or_else(|_| "codex".to_owned());
        let (codex_version, codex_managed_unavailable_reason) = probe_managed_codex(&codex_binary);
        let codex_ignore_user_config = parse_env_bool("UPRAVA_CODEX_IGNORE_USER_CONFIG", true)?;
        let codex_timeout =
            parse_env_duration_seconds("UPRAVA_CODEX_TIMEOUT_SECONDS", 24 * 60 * 60)?;
        let opensandbox_url = std::env::var("UPRAVA_OPENSANDBOX_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(|value| {
                value
                    .parse::<Url>()
                    .context("UPRAVA_OPENSANDBOX_URL must be a valid URL")
            })
            .transpose()?;
        if let Some(url) = &opensandbox_url {
            validate_insecure_opensandbox_url(url)?;
        }
        let task_runtime_image = std::env::var("UPRAVA_TASK_RUNTIME_IMAGE")
            .unwrap_or_else(|_| "uprava/codex-runtime:0.2.26".to_owned());
        if task_runtime_image.trim().is_empty() {
            anyhow::bail!("UPRAVA_TASK_RUNTIME_IMAGE must not be empty");
        }
        let toolhive_url = std::env::var("UPRAVA_TOOLHIVE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:18081".to_owned())
            .parse::<Url>()
            .context("UPRAVA_TOOLHIVE_URL must be a valid URL")?;
        let toolhive_timeout =
            parse_env_duration_seconds("UPRAVA_TOOLHIVE_TIMEOUT_SECONDS", 5 * 60)?;

        Ok(Self {
            core_url,
            display_name,
            heartbeat_interval,
            state_path,
            workspace_paths,
            codex_binary,
            codex_version,
            codex_managed_unavailable_reason,
            codex_ignore_user_config,
            codex_timeout,
            opensandbox_url,
            task_runtime_image,
            toolhive_url,
            toolhive_timeout,
        })
    }
}

fn probe_managed_codex(binary: &str) -> (Option<String>, Option<String>) {
    if !super::command_available(binary) {
        return (None, Some("binary_not_found".to_owned()));
    }
    let version = match bounded_codex_version_probe(binary) {
        Ok(version) => version,
        Err(reason) => return (None, Some(reason.to_owned())),
    };
    let Some(parts) = version.split_whitespace().find_map(parse_numeric_version) else {
        return (Some(version), Some("version_unrecognized".to_owned()));
    };
    let supported = parts.0 > 0 || parts >= (0, 144, 1);
    (
        Some(version),
        (!supported).then(|| "version_unsupported".to_owned()),
    )
}

fn bounded_codex_version_probe(binary: &str) -> Result<String, &'static str> {
    let mut child = std::process::Command::new(binary)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "version_probe_failed")?;
    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("version_probe_timeout");
            }
            Err(_) => return Err("version_probe_failed"),
        }
    };
    if !status.success() {
        return Err("version_probe_failed");
    }
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .ok_or("version_probe_failed")?
        .take(4096)
        .read_to_end(&mut bytes)
        .map_err(|_| "version_probe_failed")?;
    Ok(String::from_utf8_lossy(&bytes).trim().to_owned())
}

pub(crate) fn parse_numeric_version(value: &str) -> Option<(u64, u64, u64)> {
    let mut parts = value.trim_start_matches('v').split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts
        .next()?
        .split(|character: char| !character.is_ascii_digit())
        .next()?
        .parse()
        .ok()?;
    Some((major, minor, patch))
}

fn validate_insecure_opensandbox_url(url: &Url) -> anyhow::Result<()> {
    let loopback = url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    });
    if url.scheme() != "http" || !loopback || !url.username().is_empty() || url.password().is_some()
    {
        anyhow::bail!(
            "UPRAVA_OPENSANDBOX_URL must be an unauthenticated loopback HTTP URL until API-key support is enabled"
        );
    }
    Ok(())
}

fn parse_env_bool(name: &str, fallback: bool) -> anyhow::Result<bool> {
    match std::env::var(name) {
        Ok(value) if value.eq_ignore_ascii_case("true") => Ok(true),
        Ok(value) if value.eq_ignore_ascii_case("false") => Ok(false),
        Ok(_) => anyhow::bail!("{name} must be true or false"),
        Err(_) => Ok(fallback),
    }
}

fn parse_env_duration_seconds(name: &str, fallback_seconds: u64) -> anyhow::Result<Duration> {
    match std::env::var(name) {
        Ok(value) => value
            .parse::<u64>()
            .map(Duration::from_secs)
            .with_context(|| format!("{name} must be an unsigned integer number of seconds")),
        Err(_) => Ok(Duration::from_secs(fallback_seconds)),
    }
}

fn parse_workspace_paths() -> anyhow::Result<Vec<PathBuf>> {
    let value = std::env::var("UPRAVA_NODE_WORKSPACES")
        .context("UPRAVA_NODE_WORKSPACES must list one or more allowed workspace roots")?;
    let paths = value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if paths.is_empty() {
        anyhow::bail!("UPRAVA_NODE_WORKSPACES must list one or more allowed workspace roots");
    }
    Ok(paths)
}
