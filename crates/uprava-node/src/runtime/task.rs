//! Isolated task worktrees, OpenSandbox lifecycle and evidence collection.

use super::*;

const OPEN_SANDBOX_EXECD_PORT: u16 = 44_772;
const TASK_OUTPUT_BYTES: usize = 64 * 1024;
const TASK_DIFF_BYTES: usize = 256 * 1024;
const MAX_EXECD_FRAME_BUFFER_BYTES: usize = 512 * 1024;
const MAX_TASK_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;
const TASK_HOME: &str = "/tmp/uprava-task-home";
const SANDBOX_READY_TIMEOUT: Duration = Duration::from_secs(60);
const SANDBOX_READY_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug)]
pub(crate) struct TaskCommandError {
    code: &'static str,
    message: String,
}

impl TaskCommandError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn payload(&self) -> JsonValue {
        JsonValue(serde_json::json!({
            "error_code": self.code,
            "message": self.message,
            "retryable": false,
        }))
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateSandboxRequest<'a> {
    image: SandboxImage<'a>,
    timeout: u64,
    resource_limits: SandboxResourceLimits<'a>,
    entrypoint: Vec<&'a str>,
    metadata: BTreeMap<&'a str, &'a str>,
    volumes: Vec<SandboxVolume<'a>>,
}

#[derive(Debug, Serialize)]
struct SandboxImage<'a> {
    uri: &'a str,
}

#[derive(Debug, Serialize)]
struct SandboxResourceLimits<'a> {
    cpu: &'a str,
    memory: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SandboxVolume<'a> {
    name: &'a str,
    host: SandboxHostPath<'a>,
    mount_path: &'a str,
    read_only: bool,
}

#[derive(Debug, Serialize)]
struct SandboxHostPath<'a> {
    path: &'a str,
}

#[derive(Debug, Deserialize)]
struct SandboxResponse {
    id: String,
}

#[derive(Debug, Deserialize)]
struct SandboxInspectResponse {
    status: SandboxStatusResponse,
}

#[derive(Debug, Deserialize)]
struct SandboxStatusResponse {
    state: String,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SandboxEndpointResponse {
    endpoint: String,
    #[serde(default)]
    headers: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
struct ExecdCommandRequest<'a> {
    command: &'a str,
    cwd: &'a str,
    background: bool,
    timeout: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    uid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gid: Option<u32>,
    envs: BTreeMap<&'a str, &'a str>,
}

#[derive(Debug, Default)]
struct ExecdOutcome {
    command_id: Option<String>,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    stdout_truncated: bool,
    stderr_truncated: bool,
    cancelled: bool,
    stream_error: Option<String>,
}

#[derive(Debug)]
struct TaskWorktree {
    path: PathBuf,
    branch: String,
}

pub(crate) async fn task_command_result(
    config: &NodeConfig,
    command: &CommandEnvelope,
    cancellation: Option<watch::Receiver<bool>>,
    live_sender: Option<&ControlFrameSender>,
    state_store: Option<&NodeStateStore>,
) -> (CommandState, JsonValue, Vec<EventEnvelope>) {
    match execute_task(config, command, cancellation, live_sender, state_store).await {
        Ok((result, events)) => match serde_json::to_value(result) {
            Ok(value) => (CommandState::Completed, JsonValue(value), events),
            Err(error) => (
                CommandState::Failed,
                TaskCommandError::new(
                    "task_run.result_serialization_failed",
                    format!("Task result could not be serialized: {error}"),
                )
                .payload(),
                events,
            ),
        },
        Err(error) => (CommandState::Failed, error.payload(), vec![]),
    }
}

async fn execute_task(
    config: &NodeConfig,
    command: &CommandEnvelope,
    cancellation: Option<watch::Receiver<bool>>,
    live_sender: Option<&ControlFrameSender>,
    state_store: Option<&NodeStateStore>,
) -> Result<(TaskRunResultPackage, Vec<EventEnvelope>), TaskCommandError> {
    let CommandPayload::RunTask {
        workspace_path,
        spec,
    } = &command.payload
    else {
        return Err(TaskCommandError::new(
            "task_run.payload_mismatch",
            "Task command is missing its execution specification",
        ));
    };
    let opensandbox_url = config.opensandbox_url.as_ref().ok_or_else(|| {
        TaskCommandError::new(
            "task_run.runtime_unconfigured",
            "UPRAVA_OPENSANDBOX_URL is not configured on this Node",
        )
    })?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|error| {
            TaskCommandError::new("task_run.runtime_client_failed", error.to_string())
        })?;
    let mut events = Vec::new();
    push_task_state(
        command,
        TaskRunState::PreparingWorkspace,
        TaskCleanupState::Pending,
        None,
        &mut events,
        live_sender,
    );
    let worktree = prepare_task_worktree(config, workspace_path, spec)?;
    persist_task_mapping(state_store, spec, &worktree, None).await?;
    if cancellation_requested(cancellation.as_ref()) {
        let mapping_cleanup_succeeded = remove_task_mapping(state_store, &spec.task_run_id)
            .await
            .is_ok();
        let cleanup_state = if mapping_cleanup_succeeded {
            TaskCleanupState::Completed
        } else {
            TaskCleanupState::Failed
        };
        let mut result = cancelled_before_sandbox(spec, &worktree, cleanup_state);
        if !mapping_cleanup_succeeded {
            result.unresolved_risks.push(
                "Task runtime mapping cleanup failed; startup reconciliation is required"
                    .to_owned(),
            );
        }
        push_task_state(
            command,
            result.state,
            result.cleanup_state,
            Some("Task was cancelled before sandbox creation"),
            &mut events,
            live_sender,
        );
        return Ok((result, events));
    }

    push_task_state(
        command,
        TaskRunState::StartingRuntime,
        TaskCleanupState::Pending,
        None,
        &mut events,
        live_sender,
    );
    let sandbox_id = match create_sandbox(&client, opensandbox_url, spec, &worktree, command).await
    {
        Ok(id) => id,
        Err(error) => {
            let mapping_cleanup_succeeded = remove_task_mapping(state_store, &spec.task_run_id)
                .await
                .is_ok();
            let cleanup_state = if mapping_cleanup_succeeded {
                TaskCleanupState::Completed
            } else {
                TaskCleanupState::Failed
            };
            let mut result = failed_result(
                spec,
                &worktree,
                cleanup_state,
                "Sandbox could not be created",
                error.code,
                &error.message,
            );
            if !mapping_cleanup_succeeded {
                result.unresolved_risks.push(
                    "Task runtime mapping cleanup failed; startup reconciliation is required"
                        .to_owned(),
                );
            }
            push_task_state(
                command,
                result.state,
                result.cleanup_state,
                Some(&error.message),
                &mut events,
                live_sender,
            );
            return Ok((result, events));
        }
    };
    if let Err(error) = persist_task_mapping(state_store, spec, &worktree, Some(&sandbox_id)).await
    {
        let sandbox_cleanup_succeeded = delete_sandbox(&client, opensandbox_url, &sandbox_id)
            .await
            .is_ok();
        let mapping_cleanup_succeeded = if sandbox_cleanup_succeeded {
            remove_task_mapping(state_store, &spec.task_run_id)
                .await
                .is_ok()
        } else {
            false
        };
        let cleanup_state = if sandbox_cleanup_succeeded && mapping_cleanup_succeeded {
            TaskCleanupState::Completed
        } else {
            TaskCleanupState::Failed
        };
        let mut result = failed_result(
            spec,
            &worktree,
            cleanup_state,
            "Sandbox mapping could not be persisted",
            error.code,
            &error.message,
        );
        if cleanup_state == TaskCleanupState::Failed {
            result.unresolved_risks.push(format!(
                "Sandbox {sandbox_id} or its runtime mapping requires operator reconciliation"
            ));
        }
        push_task_state(
            command,
            result.state,
            result.cleanup_state,
            Some(&error.message),
            &mut events,
            live_sender,
        );
        return Ok((result, events));
    }

    let execution = run_task_in_sandbox(
        TaskSandboxContext {
            client: &client,
            opensandbox_url,
            sandbox_id: &sandbox_id,
            command,
            live_sender,
        },
        spec,
        &worktree,
        cancellation,
        &mut events,
    )
    .await;
    push_task_state(
        command,
        TaskRunState::CollectingEvidence,
        TaskCleanupState::Pending,
        None,
        &mut events,
        live_sender,
    );
    let evidence = collect_task_evidence(spec, &worktree);
    let sandbox_cleanup_succeeded = delete_sandbox(&client, opensandbox_url, &sandbox_id)
        .await
        .is_ok();
    let mapping_cleanup_succeeded = if sandbox_cleanup_succeeded {
        remove_task_mapping(state_store, &spec.task_run_id)
            .await
            .is_ok()
    } else {
        false
    };
    let cleanup_state = if sandbox_cleanup_succeeded && mapping_cleanup_succeeded {
        TaskCleanupState::Completed
    } else {
        TaskCleanupState::Failed
    };
    let mut result = assemble_task_result(spec, &worktree, execution, evidence, cleanup_state);
    if cleanup_state == TaskCleanupState::Failed {
        result.unresolved_risks.push(format!(
            "Sandbox {sandbox_id} cleanup failed; operator reconciliation is required"
        ));
    }
    result.unresolved_risks.push(
        "Codex credential-profile mounting is deferred; this image must be authenticated manually"
            .to_owned(),
    );
    push_task_state(
        command,
        result.state,
        result.cleanup_state,
        result
            .terminal_reason
            .as_ref()
            .map(|reason| reason.message.as_str()),
        &mut events,
        live_sender,
    );
    Ok((result, events))
}

#[derive(Debug)]
struct TaskExecution {
    state: TaskRunState,
    summary: String,
    checks: Vec<TaskCheckResult>,
    terminal_reason: Option<ScheduledMessageFailure>,
}

#[derive(Debug)]
struct TaskEvidence {
    final_revision: Option<String>,
    diff: String,
    diff_truncated: bool,
    artifacts: Vec<TaskArtifactEvidence>,
    risks: Vec<String>,
}

struct TaskSandboxContext<'a> {
    client: &'a reqwest::Client,
    opensandbox_url: &'a Url,
    sandbox_id: &'a str,
    command: &'a CommandEnvelope,
    live_sender: Option<&'a ControlFrameSender>,
}

async fn run_task_in_sandbox(
    context: TaskSandboxContext<'_>,
    spec: &TaskRunSpec,
    worktree: &TaskWorktree,
    cancellation: Option<watch::Receiver<bool>>,
    events: &mut Vec<EventEnvelope>,
) -> TaskExecution {
    let TaskSandboxContext {
        client,
        opensandbox_url,
        sandbox_id,
        command,
        live_sender,
    } = context;
    let task_deadline = Instant::now() + Duration::from_secs(spec.timeout_seconds);
    let mut readiness_cancellation = cancellation.clone();
    if let Err(error) = wait_sandbox_ready(
        client,
        opensandbox_url,
        sandbox_id,
        readiness_cancellation.as_mut(),
    )
    .await
    {
        if error.code == "task_run.cancelled" {
            return TaskExecution {
                state: TaskRunState::Cancelled,
                summary: "Task was cancelled while the sandbox was starting".to_owned(),
                checks: vec![],
                terminal_reason: Some(ScheduledMessageFailure {
                    code: error.code.to_owned(),
                    message: error.message,
                }),
            };
        }
        return failed_execution(error.code, &error.message);
    }
    let endpoint = match sandbox_endpoint(client, opensandbox_url, sandbox_id, &worktree.path).await
    {
        Ok(endpoint) => endpoint,
        Err(error) => {
            return failed_execution(error.code, &error.message);
        }
    };
    push_task_state(
        command,
        TaskRunState::Running,
        TaskCleanupState::Pending,
        None,
        events,
        live_sender,
    );
    let Some(codex_timeout) = remaining_task_time(task_deadline) else {
        return timed_out_execution(
            "Task Run reached its hard timeout before Codex started",
            vec![],
        );
    };
    let summary_file = format!(".uprava-task-summary-{}", spec.task_run_id.as_str());
    let codex_command = format!(
        "mkdir -p {TASK_HOME} && printf '%s' \"$UPRAVA_TASK_PROMPT\" | codex exec --ignore-user-config --cd /workspace --skip-git-repo-check --dangerously-bypass-approvals-and-sandbox --json --output-last-message {} -",
        shell_quote(&format!("/workspace/{summary_file}"))
    );
    let check_cancellation = cancellation.clone();
    let codex = execute_sandbox_command(
        client,
        &endpoint,
        &codex_command,
        codex_timeout,
        [
            ("UPRAVA_TASK_PROMPT", spec.prompt.as_str()),
            ("HOME", TASK_HOME),
            ("CODEX_HOME", TASK_HOME),
        ]
        .into_iter()
        .collect(),
        cancellation,
    )
    .await;
    let summary_path = worktree.path.join(&summary_file);
    let summary = std::fs::read_to_string(&summary_path)
        .ok()
        .map(|value| cap_string(value, TASK_OUTPUT_BYTES).0)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            last_nonempty_line(&codex.stdout)
                .unwrap_or("Task execution completed")
                .to_owned()
        });
    if let Err(error) = std::fs::remove_file(&summary_path) {
        if error.kind() != ErrorKind::NotFound {
            tracing::warn!(error = %error, "task summary file cleanup failed");
        }
    }
    if codex.cancelled {
        return TaskExecution {
            state: TaskRunState::Cancelled,
            summary,
            checks: vec![],
            terminal_reason: Some(ScheduledMessageFailure {
                code: "task_run.cancelled".to_owned(),
                message: "Task Run was cancelled".to_owned(),
            }),
        };
    }
    if codex.stream_error.as_deref() == Some("timeout") {
        return TaskExecution {
            state: TaskRunState::TimedOut,
            summary,
            checks: vec![],
            terminal_reason: Some(ScheduledMessageFailure {
                code: "task_run.timed_out".to_owned(),
                message: "Task Run exceeded its hard timeout".to_owned(),
            }),
        };
    }

    push_task_state(
        command,
        TaskRunState::Checking,
        TaskCleanupState::Pending,
        None,
        events,
        live_sender,
    );
    let mut checks = Vec::with_capacity(spec.checks.len());
    for check in &spec.checks {
        let Some(remaining) = remaining_task_time(task_deadline) else {
            return timed_out_execution(
                "Task Run reached its hard timeout while checks were pending",
                checks,
            );
        };
        let declared_timeout = Duration::from_secs(check.timeout_seconds);
        let check_uses_task_deadline = remaining <= declared_timeout;
        let started = Instant::now();
        let check_command = std::iter::once(check.command.as_str())
            .chain(check.args.iter().map(String::as_str))
            .map(shell_quote)
            .collect::<Vec<_>>()
            .join(" ");
        let outcome = execute_sandbox_command(
            client,
            &endpoint,
            &check_command,
            remaining.min(declared_timeout),
            [("HOME", TASK_HOME), ("CODEX_HOME", TASK_HOME)]
                .into_iter()
                .collect(),
            check_cancellation.clone(),
        )
        .await;
        let cancelled = outcome.cancelled;
        let timed_out = outcome.stream_error.as_deref() == Some("timeout");
        checks.push(TaskCheckResult {
            label: check.label.clone(),
            command: check_command,
            success: outcome.exit_code == Some(0) && outcome.stream_error.is_none(),
            exit_code: outcome.exit_code,
            stdout: outcome.stdout,
            stderr: outcome.stderr,
            stdout_truncated: outcome.stdout_truncated,
            stderr_truncated: outcome.stderr_truncated,
            duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        });
        if cancelled {
            return TaskExecution {
                state: TaskRunState::Cancelled,
                summary,
                checks,
                terminal_reason: Some(ScheduledMessageFailure {
                    code: "task_run.cancelled".to_owned(),
                    message: "Task Run was cancelled while checks were running".to_owned(),
                }),
            };
        }
        if check_uses_task_deadline && timed_out {
            return timed_out_execution(
                "Task Run reached its hard timeout while a check was running",
                checks,
            );
        }
    }
    let codex_success = codex.exit_code == Some(0) && codex.stream_error.is_none();
    let checks_success = checks.iter().all(|check| check.success);
    if codex_success && checks_success {
        TaskExecution {
            state: TaskRunState::Succeeded,
            summary,
            checks,
            terminal_reason: None,
        }
    } else {
        let (code, message) = if !codex_success {
            (
                "task_run.provider_failed",
                codex
                    .stream_error
                    .as_deref()
                    .unwrap_or("Codex exited unsuccessfully"),
            )
        } else {
            (
                "task_run.check_failed",
                "One or more declared checks failed",
            )
        };
        TaskExecution {
            state: TaskRunState::Failed,
            summary,
            checks,
            terminal_reason: Some(ScheduledMessageFailure {
                code: code.to_owned(),
                message: message.to_owned(),
            }),
        }
    }
}

#[derive(Debug)]
struct SandboxEndpoint {
    url: Url,
    headers: BTreeMap<String, String>,
    uid: Option<u32>,
    gid: Option<u32>,
}

async fn create_sandbox(
    client: &reqwest::Client,
    opensandbox_url: &Url,
    spec: &TaskRunSpec,
    worktree: &TaskWorktree,
    command: &CommandEnvelope,
) -> Result<String, TaskCommandError> {
    let endpoint = opensandbox_url.join("/v1/sandboxes").map_err(|error| {
        TaskCommandError::new("task_run.runtime_url_invalid", error.to_string())
    })?;
    let worktree_path = worktree.path.to_str().ok_or_else(|| {
        TaskCommandError::new(
            "task_run.worktree_path_invalid",
            "Worktree path is not valid UTF-8",
        )
    })?;
    let request = CreateSandboxRequest {
        image: SandboxImage {
            uri: &spec.runtime_image,
        },
        timeout: spec.ttl_seconds,
        resource_limits: SandboxResourceLimits {
            cpu: &spec.resource_limits.cpu,
            memory: &spec.resource_limits.memory,
        },
        entrypoint: vec!["tail", "-f", "/dev/null"],
        metadata: [
            ("uprava-run-id", spec.task_run_id.as_str()),
            ("uprava-node-id", command.target.node_id().as_str()),
        ]
        .into_iter()
        .collect(),
        volumes: vec![SandboxVolume {
            name: "workspace",
            host: SandboxHostPath {
                path: worktree_path,
            },
            mount_path: "/workspace",
            read_only: false,
        }],
    };
    let response = client
        .post(endpoint)
        .timeout(SANDBOX_READY_TIMEOUT)
        .json(&request)
        .send()
        .await
        .map_err(|error| {
            TaskCommandError::new("task_run.sandbox_create_failed", error.to_string())
        })?
        .error_for_status()
        .map_err(|error| {
            TaskCommandError::new("task_run.sandbox_create_failed", error.to_string())
        })?
        .json::<SandboxResponse>()
        .await
        .map_err(|error| {
            TaskCommandError::new("task_run.sandbox_response_invalid", error.to_string())
        })?;
    Ok(response.id)
}

async fn wait_sandbox_ready(
    client: &reqwest::Client,
    opensandbox_url: &Url,
    sandbox_id: &str,
    cancellation: Option<&mut watch::Receiver<bool>>,
) -> Result<(), TaskCommandError> {
    let endpoint = opensandbox_url
        .join(&format!("/v1/sandboxes/{sandbox_id}"))
        .map_err(|error| {
            TaskCommandError::new("task_run.runtime_url_invalid", error.to_string())
        })?;
    let started = Instant::now();
    loop {
        if cancellation_requested(cancellation.as_deref()) {
            return Err(TaskCommandError::new(
                "task_run.cancelled",
                "Task Run was cancelled before the sandbox became ready",
            ));
        }
        let response = client
            .get(endpoint.clone())
            .timeout(Duration::from_secs(10))
            .send()
            .await;
        let response = match response {
            Ok(response) if response.status().is_success() => response,
            Ok(response)
                if response.status() == reqwest::StatusCode::NOT_FOUND
                    || response.status() == reqwest::StatusCode::CONFLICT
                    || response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
                    || response.status().is_server_error() =>
            {
                if started.elapsed() >= SANDBOX_READY_TIMEOUT {
                    return Err(TaskCommandError::new(
                        "task_run.sandbox_ready_timeout",
                        format!(
                            "OpenSandbox readiness polling ended with HTTP {}",
                            response.status()
                        ),
                    ));
                }
                tokio::time::sleep(SANDBOX_READY_POLL_INTERVAL).await;
                continue;
            }
            Ok(response) => {
                return Err(TaskCommandError::new(
                    "task_run.sandbox_inspect_failed",
                    format!("OpenSandbox readiness returned HTTP {}", response.status()),
                ));
            }
            Err(error) => {
                if started.elapsed() >= SANDBOX_READY_TIMEOUT {
                    return Err(TaskCommandError::new(
                        "task_run.sandbox_ready_timeout",
                        error.to_string(),
                    ));
                }
                tokio::time::sleep(SANDBOX_READY_POLL_INTERVAL).await;
                continue;
            }
        };
        let response = response
            .json::<SandboxInspectResponse>()
            .await
            .map_err(|error| {
                TaskCommandError::new("task_run.sandbox_response_invalid", error.to_string())
            })?;
        match sandbox_readiness(&response)? {
            true => return Ok(()),
            _ if started.elapsed() >= SANDBOX_READY_TIMEOUT => {
                return Err(TaskCommandError::new(
                    "task_run.sandbox_ready_timeout",
                    "OpenSandbox did not become ready within 60 seconds",
                ));
            }
            _ => tokio::time::sleep(SANDBOX_READY_POLL_INTERVAL).await,
        }
    }
}

fn sandbox_readiness(response: &SandboxInspectResponse) -> Result<bool, TaskCommandError> {
    match response.status.state.to_ascii_lowercase().as_str() {
        "running" => Ok(true),
        "failed" | "terminated" | "stopping" => Err(TaskCommandError::new(
            "task_run.sandbox_start_failed",
            response.status.message.clone().unwrap_or_else(|| {
                format!(
                    "OpenSandbox entered lifecycle state {} before execution",
                    response.status.state
                )
            }),
        )),
        _ => Ok(false),
    }
}

async fn sandbox_endpoint(
    client: &reqwest::Client,
    opensandbox_url: &Url,
    sandbox_id: &str,
    worktree_path: &Path,
) -> Result<SandboxEndpoint, TaskCommandError> {
    let endpoint = opensandbox_url
        .join(&format!(
            "/v1/sandboxes/{sandbox_id}/endpoints/{OPEN_SANDBOX_EXECD_PORT}?use_server_proxy=true"
        ))
        .map_err(|error| {
            TaskCommandError::new("task_run.runtime_url_invalid", error.to_string())
        })?;
    let response = client
        .get(endpoint)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|error| {
            TaskCommandError::new("task_run.endpoint_lookup_failed", error.to_string())
        })?
        .error_for_status()
        .map_err(|error| {
            TaskCommandError::new("task_run.endpoint_lookup_failed", error.to_string())
        })?
        .json::<SandboxEndpointResponse>()
        .await
        .map_err(|error| {
            TaskCommandError::new("task_run.endpoint_response_invalid", error.to_string())
        })?;
    let url = response
        .endpoint
        .parse::<Url>()
        .map_err(|error| TaskCommandError::new("task_run.execd_url_invalid", error.to_string()))?;
    let (uid, gid) = task_command_identity(worktree_path);
    Ok(SandboxEndpoint {
        url,
        headers: response.headers,
        uid,
        gid,
    })
}

#[cfg(unix)]
fn task_command_identity(worktree_path: &Path) -> (Option<u32>, Option<u32>) {
    use std::os::unix::fs::MetadataExt;

    match std::fs::metadata(worktree_path) {
        Ok(metadata) => (Some(metadata.uid()), Some(metadata.gid())),
        Err(error) => {
            tracing::warn!(
                error = %error,
                "task worktree ownership could not be inspected"
            );
            (None, None)
        }
    }
}

#[cfg(not(unix))]
fn task_command_identity(_worktree_path: &Path) -> (Option<u32>, Option<u32>) {
    (None, None)
}

async fn delete_sandbox(
    client: &reqwest::Client,
    opensandbox_url: &Url,
    sandbox_id: &str,
) -> Result<(), TaskCommandError> {
    let endpoint = opensandbox_url
        .join(&format!("/v1/sandboxes/{sandbox_id}"))
        .map_err(|error| {
            TaskCommandError::new("task_run.runtime_url_invalid", error.to_string())
        })?;
    let response = client
        .delete(endpoint)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|error| TaskCommandError::new("task_run.cleanup_failed", error.to_string()))?;
    if response.status() != reqwest::StatusCode::NOT_FOUND {
        response
            .error_for_status()
            .map_err(|error| TaskCommandError::new("task_run.cleanup_failed", error.to_string()))?;
    }
    Ok(())
}

async fn execute_sandbox_command(
    client: &reqwest::Client,
    endpoint: &SandboxEndpoint,
    command: &str,
    timeout: Duration,
    envs: BTreeMap<&str, &str>,
    mut cancellation: Option<watch::Receiver<bool>>,
) -> ExecdOutcome {
    let started = Instant::now();
    let command_url = execd_command_url(&endpoint.url);
    let request = ExecdCommandRequest {
        command,
        cwd: "/workspace",
        background: false,
        timeout: timeout.as_millis().min(u128::from(u64::MAX)) as u64,
        uid: endpoint.uid,
        gid: endpoint.gid,
        envs,
    };
    let mut builder = client.post(command_url).json(&request);
    for (name, value) in &endpoint.headers {
        builder = builder.header(name, value);
    }
    let start_timeout = timeout.min(Duration::from_secs(15));
    let response = match tokio::time::timeout(start_timeout, builder.send()).await {
        Ok(Ok(response)) => match response.error_for_status() {
            Ok(response) => response,
            Err(error) => {
                return ExecdOutcome {
                    stream_error: Some(error.to_string()),
                    ..ExecdOutcome::default()
                };
            }
        },
        Ok(Err(error)) => {
            return ExecdOutcome {
                stream_error: Some(error.to_string()),
                ..ExecdOutcome::default()
            };
        }
        Err(_) => {
            return ExecdOutcome {
                stream_error: Some(if start_timeout == timeout {
                    "timeout".to_owned()
                } else {
                    "OpenSandbox execd did not start the command in time".to_owned()
                }),
                ..ExecdOutcome::default()
            };
        }
    };
    let mut stream = response.bytes_stream();
    let mut buffer = Vec::<u8>::new();
    let mut outcome = ExecdOutcome::default();
    let hard_timeout = tokio::time::sleep(timeout.saturating_sub(started.elapsed()));
    tokio::pin!(hard_timeout);
    loop {
        tokio::select! {
            _ = &mut hard_timeout => {
                outcome.stream_error = Some("timeout".to_owned());
                cancel_execd_command(client, endpoint, outcome.command_id.as_deref()).await;
                break;
            }
            cancellation_result = wait_for_task_cancellation(cancellation.as_mut()), if cancellation.is_some() => {
                if cancellation_result {
                    outcome.cancelled = true;
                    cancel_execd_command(client, endpoint, outcome.command_id.as_deref()).await;
                    break;
                }
            }
            chunk = stream.next() => {
                match chunk {
                    Some(Ok(chunk)) => {
                        if buffer.len().saturating_add(chunk.len()) > MAX_EXECD_FRAME_BUFFER_BYTES {
                            outcome.stream_error = Some("OpenSandbox execd frame buffer exceeded its limit".to_owned());
                            cancel_execd_command(client, endpoint, outcome.command_id.as_deref()).await;
                            break;
                        }
                        buffer.extend_from_slice(&chunk);
                        consume_execd_frames(&mut buffer, &mut outcome);
                    }
                    Some(Err(error)) => {
                        outcome.stream_error = Some(error.to_string());
                        break;
                    }
                    None => {
                        consume_final_execd_frame(&mut buffer, &mut outcome);
                        break;
                    }
                }
            }
        }
    }
    outcome
}

async fn wait_for_task_cancellation(receiver: Option<&mut watch::Receiver<bool>>) -> bool {
    let Some(receiver) = receiver else {
        std::future::pending::<()>().await;
        return false;
    };
    if *receiver.borrow() {
        return true;
    }
    receiver.changed().await.is_ok() && *receiver.borrow()
}

async fn cancel_execd_command(
    client: &reqwest::Client,
    endpoint: &SandboxEndpoint,
    command_id: Option<&str>,
) {
    let Some(command_id) = command_id else {
        return;
    };
    let mut url = execd_command_url(&endpoint.url);
    url.query_pairs_mut().append_pair("id", command_id);
    let mut builder = client.delete(url);
    for (name, value) in &endpoint.headers {
        builder = builder.header(name, value);
    }
    if let Err(error) = builder
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
    {
        tracing::warn!(error = %error, "OpenSandbox execd command cancellation failed");
    }
}

fn execd_command_url(endpoint: &Url) -> Url {
    let mut url = endpoint.clone();
    let path = format!("{}/command", endpoint.path().trim_end_matches('/'));
    url.set_path(&path);
    url
}

fn consume_execd_frames(buffer: &mut Vec<u8>, outcome: &mut ExecdOutcome) {
    while let Some((index, delimiter_len)) = next_sse_frame(buffer) {
        let frame = buffer.drain(..index + delimiter_len).collect::<Vec<_>>();
        parse_execd_frame(&frame[..index], outcome);
    }
}

fn next_sse_frame(buffer: &[u8]) -> Option<(usize, usize)> {
    let lf = buffer
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|index| (index, 2));
    let crlf = buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| (index, 4));
    match (lf, crlf) {
        (Some(left), Some(right)) => Some(if left.0 <= right.0 { left } else { right }),
        (Some(frame), None) | (None, Some(frame)) => Some(frame),
        (None, None) => None,
    }
}

fn consume_final_execd_frame(buffer: &mut Vec<u8>, outcome: &mut ExecdOutcome) {
    if !buffer.is_empty() {
        let frame = std::mem::take(buffer);
        parse_execd_frame(&frame, outcome);
    }
}

fn parse_execd_frame(frame: &[u8], outcome: &mut ExecdOutcome) {
    let text = String::from_utf8_lossy(frame);
    let data = text
        .lines()
        .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
        .collect::<Vec<_>>();
    let owned_payload;
    let payload = if data.is_empty() {
        text.trim()
    } else {
        owned_payload = data.join("\n");
        owned_payload.trim()
    };
    if payload.is_empty() {
        return;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
        return;
    };
    let event_type = value
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let event_text = value
        .get("text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    match event_type {
        "init" => {
            if !event_text.is_empty() {
                outcome.command_id = Some(event_text.to_owned());
            }
        }
        "stdout" => append_capped(
            &mut outcome.stdout,
            event_text,
            TASK_OUTPUT_BYTES,
            &mut outcome.stdout_truncated,
        ),
        "stderr" | "error" => append_capped(
            &mut outcome.stderr,
            event_text,
            TASK_OUTPUT_BYTES,
            &mut outcome.stderr_truncated,
        ),
        "execution_complete" | "status" => {
            outcome.exit_code = value
                .get("exit_code")
                .or_else(|| value.get("exitCode"))
                .or_else(|| value.get("code"))
                .and_then(serde_json::Value::as_i64)
                .and_then(|code| i32::try_from(code).ok())
                .or(outcome.exit_code);
        }
        _ => {}
    }
}

fn prepare_task_worktree(
    config: &NodeConfig,
    workspace_path: &str,
    spec: &TaskRunSpec,
) -> Result<TaskWorktree, TaskCommandError> {
    let workspace = canonical_workspace_root(config, workspace_path)
        .map_err(|error| TaskCommandError::new(error.code, error.message))?;
    let repo = git_text(&workspace, &["rev-parse", "--show-toplevel"])?;
    let repo = std::fs::canonicalize(repo.trim())
        .map_err(|error| TaskCommandError::new("task_run.repository_invalid", error.to_string()))?;
    if !canonical_workspace_path_allowed(config, &repo) {
        return Err(TaskCommandError::new(
            "task_run.repository_outside_allowed_roots",
            "Git repository root is outside the Node workspace allow-list",
        ));
    }
    git_success(
        &repo,
        &[
            "cat-file",
            "-e",
            &format!("{}^{{commit}}", spec.base_revision),
        ],
    )?;
    let runs_root = repo.join(".uprava").join("runs");
    std::fs::create_dir_all(&runs_root).map_err(|error| {
        TaskCommandError::new("task_run.worktree_parent_failed", error.to_string())
    })?;
    let canonical_runs_root = std::fs::canonicalize(&runs_root).map_err(|error| {
        TaskCommandError::new("task_run.worktree_parent_invalid", error.to_string())
    })?;
    if !canonical_runs_root.starts_with(&repo) {
        return Err(TaskCommandError::new(
            "task_run.worktree_parent_escape",
            "Task worktree parent escapes the repository",
        ));
    }
    let path = canonical_runs_root.join(spec.task_run_id.as_str());
    if path.exists() {
        let canonical_path = std::fs::canonicalize(&path).map_err(|error| {
            TaskCommandError::new("task_run.worktree_invalid", error.to_string())
        })?;
        if !canonical_path.starts_with(&canonical_runs_root) {
            return Err(TaskCommandError::new(
                "task_run.worktree_escape",
                "Existing task worktree escapes its bounded parent",
            ));
        }
        let top = git_text(&canonical_path, &["rev-parse", "--show-toplevel"])?;
        let canonical_top = std::fs::canonicalize(top.trim()).map_err(|error| {
            TaskCommandError::new("task_run.worktree_invalid", error.to_string())
        })?;
        if canonical_top != canonical_path {
            return Err(TaskCommandError::new(
                "task_run.worktree_identity_mismatch",
                "Existing task directory is not the expected Git worktree",
            ));
        }
        let current_branch = git_text(&canonical_path, &["branch", "--show-current"])?;
        if current_branch.trim() != spec.branch {
            return Err(TaskCommandError::new(
                "task_run.worktree_branch_mismatch",
                "Existing task worktree is attached to a different branch",
            ));
        }
        let expected_common_dir = canonical_git_common_dir(&repo)?;
        let actual_common_dir = canonical_git_common_dir(&canonical_path)?;
        if actual_common_dir != expected_common_dir {
            return Err(TaskCommandError::new(
                "task_run.worktree_repository_mismatch",
                "Existing task worktree belongs to a different Git repository",
            ));
        }
        return Ok(TaskWorktree {
            path: canonical_path,
            branch: spec.branch.clone(),
        });
    }
    let created = StdCommand::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["worktree", "add", "-b", &spec.branch])
        .arg(&path)
        .arg(&spec.base_revision)
        .output()
        .map_err(|error| {
            TaskCommandError::new("task_run.worktree_create_failed", error.to_string())
        })?;
    if !created.status.success() {
        return Err(TaskCommandError::new(
            "task_run.worktree_create_failed",
            String::from_utf8_lossy(&created.stderr).trim().to_owned(),
        ));
    }
    let canonical_path = std::fs::canonicalize(&path)
        .map_err(|error| TaskCommandError::new("task_run.worktree_invalid", error.to_string()))?;
    Ok(TaskWorktree {
        path: canonical_path,
        branch: spec.branch.clone(),
    })
}

fn canonical_git_common_dir(path: &Path) -> Result<PathBuf, TaskCommandError> {
    let common_dir = git_text(path, &["rev-parse", "--git-common-dir"])?;
    let common_dir = Path::new(common_dir.trim());
    let candidate = if common_dir.is_absolute() {
        common_dir.to_path_buf()
    } else {
        path.join(common_dir)
    };
    std::fs::canonicalize(candidate).map_err(|error| {
        TaskCommandError::new("task_run.repository_identity_failed", error.to_string())
    })
}

fn collect_task_evidence(
    spec: &TaskRunSpec,
    worktree: &TaskWorktree,
) -> Result<TaskEvidence, TaskCommandError> {
    let final_revision = git_text(&worktree.path, &["rev-parse", "HEAD"])
        .ok()
        .map(|value| value.trim().to_owned());
    let (status, status_truncated) =
        git_output_capped(&worktree.path, &["status", "--short"], TASK_DIFF_BYTES)?;
    let (diff, diff_truncated) = git_diff_capped(&worktree.path, &spec.base_revision)?;
    let combined = if status.trim().is_empty() {
        diff
    } else {
        format!(
            "# git status --short\n{status}\n# git diff {}\n{diff}",
            spec.base_revision
        )
    };
    let (diff, cap_truncated) = cap_string(combined, TASK_DIFF_BYTES);
    let mut artifacts = Vec::with_capacity(spec.artifact_paths.len());
    let mut risks = Vec::new();
    for relative in &spec.artifact_paths {
        match hash_task_artifact(&worktree.path, relative) {
            Ok(evidence) => artifacts.push(evidence),
            Err(error) => risks.push(format!(
                "Artifact `{relative}` was not retained: {}",
                error.message
            )),
        }
    }
    Ok(TaskEvidence {
        final_revision,
        diff,
        diff_truncated: status_truncated || diff_truncated || cap_truncated,
        artifacts,
        risks,
    })
}

fn hash_task_artifact(
    worktree: &Path,
    relative: &str,
) -> Result<TaskArtifactEvidence, TaskCommandError> {
    let relative_path = safe_workspace_relative_path(relative)
        .map_err(|error| TaskCommandError::new(error.code, error.message))?;
    let candidate = worktree.join(&relative_path);
    let metadata = std::fs::symlink_metadata(&candidate).map_err(|error| {
        TaskCommandError::new("task_run.artifact_unavailable", error.to_string())
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(TaskCommandError::new(
            "task_run.artifact_type_rejected",
            "Artifact evidence must be a regular non-symlink file",
        ));
    }
    let canonical = std::fs::canonicalize(&candidate).map_err(|error| {
        TaskCommandError::new("task_run.artifact_unavailable", error.to_string())
    })?;
    if !canonical.starts_with(worktree) {
        return Err(TaskCommandError::new(
            "task_run.artifact_escape",
            "Artifact path escapes the task worktree",
        ));
    }
    let mut file = open_task_artifact(&candidate).map_err(|error| {
        TaskCommandError::new("task_run.artifact_unavailable", error.to_string())
    })?;
    let opened_metadata = file.metadata().map_err(|error| {
        TaskCommandError::new("task_run.artifact_unavailable", error.to_string())
    })?;
    if !opened_metadata.is_file() || opened_metadata.len() > MAX_TASK_ARTIFACT_BYTES {
        return Err(TaskCommandError::new(
            "task_run.artifact_too_large",
            "Artifact is not a bounded regular file",
        ));
    }
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    let mut size_bytes = 0_u64;
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            TaskCommandError::new("task_run.artifact_read_failed", error.to_string())
        })?;
        if read == 0 {
            break;
        }
        size_bytes = size_bytes.saturating_add(read as u64);
        if size_bytes > MAX_TASK_ARTIFACT_BYTES {
            return Err(TaskCommandError::new(
                "task_run.artifact_too_large",
                "Artifact grew beyond the evidence size limit while it was read",
            ));
        }
        digest.update(&buffer[..read]);
    }
    Ok(TaskArtifactEvidence {
        path: relative.to_owned(),
        size_bytes,
        sha256: format!("{:x}", digest.finalize()),
    })
}

#[cfg(unix)]
fn open_task_artifact(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(not(unix))]
fn open_task_artifact(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}

fn git_diff_capped(path: &Path, base: &str) -> Result<(String, bool), TaskCommandError> {
    git_output_capped(
        path,
        &["diff", "--no-ext-diff", "--binary", base],
        TASK_DIFF_BYTES,
    )
}

fn git_output_capped(
    path: &Path,
    args: &[&str],
    max_bytes: usize,
) -> Result<(String, bool), TaskCommandError> {
    let mut child = StdCommand::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| TaskCommandError::new("task_run.diff_failed", error.to_string()))?;
    let mut stdout = child.stdout.take().ok_or_else(|| {
        TaskCommandError::new("task_run.diff_failed", "Git diff stdout is unavailable")
    })?;
    let mut retained = Vec::with_capacity(max_bytes);
    let mut buffer = [0_u8; 16 * 1024];
    let mut truncated = false;
    loop {
        let read = stdout
            .read(&mut buffer)
            .map_err(|error| TaskCommandError::new("task_run.diff_failed", error.to_string()))?;
        if read == 0 {
            break;
        }
        let remaining = max_bytes.saturating_sub(retained.len());
        let take = remaining.min(read);
        retained.extend_from_slice(&buffer[..take]);
        if take < read {
            truncated = true;
            let _ = child.kill();
            break;
        }
    }
    let status = child
        .wait()
        .map_err(|error| TaskCommandError::new("task_run.diff_failed", error.to_string()))?;
    if !status.success() && !truncated {
        return Err(TaskCommandError::new(
            "task_run.diff_failed",
            "Git diff exited unsuccessfully",
        ));
    }
    Ok((String::from_utf8_lossy(&retained).into_owned(), truncated))
}

fn git_text(path: &Path, args: &[&str]) -> Result<String, TaskCommandError> {
    let output = StdCommand::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .map_err(|error| TaskCommandError::new("task_run.git_failed", error.to_string()))?;
    if !output.status.success() {
        return Err(TaskCommandError::new(
            "task_run.git_failed",
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn git_success(path: &Path, args: &[&str]) -> Result<(), TaskCommandError> {
    git_text(path, args).map(|_| ())
}

fn assemble_task_result(
    spec: &TaskRunSpec,
    worktree: &TaskWorktree,
    execution: TaskExecution,
    evidence: Result<TaskEvidence, TaskCommandError>,
    cleanup_state: TaskCleanupState,
) -> TaskRunResultPackage {
    let (state, terminal_reason, evidence) = match evidence {
        Ok(evidence) => (execution.state, execution.terminal_reason, evidence),
        Err(error) => (
            TaskRunState::Failed,
            Some(ScheduledMessageFailure {
                code: error.code.to_owned(),
                message: error.message.clone(),
            }),
            TaskEvidence {
                final_revision: None,
                diff: String::new(),
                diff_truncated: false,
                artifacts: vec![],
                risks: vec![error.message],
            },
        ),
    };
    TaskRunResultPackage {
        task_run_id: spec.task_run_id.clone(),
        state,
        cleanup_state,
        summary: execution.summary,
        base_revision: spec.base_revision.clone(),
        final_revision: evidence.final_revision,
        branch: worktree.branch.clone(),
        worktree_path: worktree.path.display().to_string(),
        runtime_image: spec.runtime_image.clone(),
        diff: evidence.diff,
        diff_truncated: evidence.diff_truncated,
        checks: execution.checks,
        artifacts: evidence.artifacts,
        unresolved_risks: evidence.risks,
        terminal_reason,
    }
}

fn failed_result(
    spec: &TaskRunSpec,
    worktree: &TaskWorktree,
    cleanup_state: TaskCleanupState,
    summary: &str,
    code: &str,
    message: &str,
) -> TaskRunResultPackage {
    TaskRunResultPackage {
        task_run_id: spec.task_run_id.clone(),
        state: TaskRunState::Failed,
        cleanup_state,
        summary: summary.to_owned(),
        base_revision: spec.base_revision.clone(),
        final_revision: None,
        branch: worktree.branch.clone(),
        worktree_path: worktree.path.display().to_string(),
        runtime_image: spec.runtime_image.clone(),
        diff: String::new(),
        diff_truncated: false,
        checks: vec![],
        artifacts: vec![],
        unresolved_risks: vec![],
        terminal_reason: Some(ScheduledMessageFailure {
            code: code.to_owned(),
            message: message.to_owned(),
        }),
    }
}

fn cancelled_before_sandbox(
    spec: &TaskRunSpec,
    worktree: &TaskWorktree,
    cleanup_state: TaskCleanupState,
) -> TaskRunResultPackage {
    TaskRunResultPackage {
        task_run_id: spec.task_run_id.clone(),
        state: TaskRunState::Cancelled,
        cleanup_state,
        summary: "Task was cancelled before sandbox creation".to_owned(),
        base_revision: spec.base_revision.clone(),
        final_revision: None,
        branch: worktree.branch.clone(),
        worktree_path: worktree.path.display().to_string(),
        runtime_image: spec.runtime_image.clone(),
        diff: String::new(),
        diff_truncated: false,
        checks: vec![],
        artifacts: vec![],
        unresolved_risks: vec![],
        terminal_reason: Some(ScheduledMessageFailure {
            code: "task_run.cancelled".to_owned(),
            message: "Task Run was cancelled".to_owned(),
        }),
    }
}

fn failed_execution(code: &str, message: &str) -> TaskExecution {
    TaskExecution {
        state: TaskRunState::Failed,
        summary: "Task sandbox execution failed".to_owned(),
        checks: vec![],
        terminal_reason: Some(ScheduledMessageFailure {
            code: code.to_owned(),
            message: message.to_owned(),
        }),
    }
}

fn timed_out_execution(message: &str, checks: Vec<TaskCheckResult>) -> TaskExecution {
    TaskExecution {
        state: TaskRunState::TimedOut,
        summary: message.to_owned(),
        checks,
        terminal_reason: Some(ScheduledMessageFailure {
            code: "task_run.timed_out".to_owned(),
            message: message.to_owned(),
        }),
    }
}

fn remaining_task_time(deadline: Instant) -> Option<Duration> {
    let remaining = deadline.checked_duration_since(Instant::now())?;
    (!remaining.is_zero()).then_some(remaining)
}

fn cancellation_requested(receiver: Option<&watch::Receiver<bool>>) -> bool {
    receiver.is_some_and(|receiver| *receiver.borrow())
}

async fn persist_task_mapping(
    state_store: Option<&NodeStateStore>,
    spec: &TaskRunSpec,
    worktree: &TaskWorktree,
    sandbox_id: Option<&str>,
) -> Result<(), TaskCommandError> {
    let Some(state_store) = state_store else {
        return Ok(());
    };
    state_store
        .upsert_task_runtime_mapping(TaskRuntimeMapping {
            task_run_id: spec.task_run_id.clone(),
            worktree_path: worktree.path.display().to_string(),
            branch: worktree.branch.clone(),
            sandbox_id: sandbox_id.map(str::to_owned),
            updated_at: Utc::now(),
        })
        .await
        .map_err(|error| {
            TaskCommandError::new(
                "task_run.mapping_persistence_failed",
                format!("Task runtime mapping could not be persisted: {error}"),
            )
        })
}

async fn remove_task_mapping(
    state_store: Option<&NodeStateStore>,
    task_run_id: &uprava_protocol::TaskRunId,
) -> Result<(), TaskCommandError> {
    let Some(state_store) = state_store else {
        return Ok(());
    };
    state_store
        .remove_task_runtime_mapping(task_run_id.clone())
        .await
        .map_err(|error| {
            TaskCommandError::new(
                "task_run.mapping_cleanup_failed",
                format!("Task runtime mapping could not be cleared: {error}"),
            )
        })
}

pub(crate) async fn reconcile_task_runtime_mappings(
    config: &NodeConfig,
    client: &reqwest::Client,
    state_store: &NodeStateStore,
) -> anyhow::Result<()> {
    let snapshot = state_store.snapshot().await?;
    if snapshot.task_runtime_mappings.is_empty() {
        return Ok(());
    }
    let Some(opensandbox_url) = config.opensandbox_url.as_ref() else {
        tracing::warn!(
            mappings = snapshot.task_runtime_mappings.len(),
            "orphaned task runtime mappings await OpenSandbox configuration"
        );
        return Ok(());
    };
    for mapping in snapshot.task_runtime_mappings.values() {
        if let Some(sandbox_id) = &mapping.sandbox_id {
            if let Err(error) = delete_sandbox(client, opensandbox_url, sandbox_id).await {
                tracing::warn!(
                    error = %error.message,
                    "orphaned task sandbox cleanup failed"
                );
                continue;
            }
        }
        state_store
            .remove_task_runtime_mapping(mapping.task_run_id.clone())
            .await?;
        tracing::info!("reconciled orphaned task runtime mapping; review worktree retained");
    }
    Ok(())
}

fn push_task_state(
    command: &CommandEnvelope,
    state: TaskRunState,
    cleanup_state: TaskCleanupState,
    message: Option<&str>,
    events: &mut Vec<EventEnvelope>,
    live_sender: Option<&ControlFrameSender>,
) {
    let Some(task_run_id) = command.target.task_run_id().cloned() else {
        return;
    };
    // Task scopes use stable sparse lifecycle ranks so a crash/retry cannot
    // assign the same sequence to a different state transition.
    let seq = task_state_seq(state);
    let event = EventEnvelope {
        event_id: EventId::from(format!(
            "task-run:{}:{}",
            task_run_id.as_str(),
            task_state_seq(state)
        )),
        command_id: Some(command.command_id.clone()),
        correlation_id: Some(command.correlation_id.clone()),
        actor_ref: ActorRef::Node {
            node_id: command.target.node_id().clone(),
        },
        scope_ref: ScopeRef::TaskRun {
            task_run_id: task_run_id.clone(),
        },
        node_id: Some(command.target.node_id().clone()),
        runtime_session_id: None,
        session_thread_id: None,
        turn_id: None,
        seq,
        session_projection_seq: None,
        kind: EventKind::TaskRunStateChanged,
        happened_at: Utc::now(),
        source_refs: vec![UpravaRef::Command {
            command_id: command.command_id.clone(),
        }],
        evidence_refs: vec![],
        cause_refs: vec![],
        result_refs: vec![UpravaRef::TaskRun {
            task_run_id: task_run_id.clone(),
        }],
        payload: EventPayload::from_json(
            EventKind::TaskRunStateChanged,
            serde_json::json!({
                "task_run_id": task_run_id,
                "state": state,
                "cleanup_state": cleanup_state,
                "message": message,
            }),
        ),
    };
    if let Some(sender) = live_sender {
        if let Err(error) = sender.try_send(ControlFrame::EventBatch {
            frame_id: Uuid::new_v4().to_string(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
            events: vec![event.clone()],
        }) {
            tracing::debug!(error = %error, "live task state event will be replayed durably");
        }
    }
    events.push(event);
}

const fn task_state_seq(state: TaskRunState) -> i64 {
    match state {
        TaskRunState::Queued => 10,
        TaskRunState::PreparingWorkspace => 20,
        TaskRunState::StartingRuntime => 30,
        TaskRunState::Running => 40,
        TaskRunState::Checking => 50,
        TaskRunState::CollectingEvidence => 60,
        TaskRunState::Succeeded => 70,
        TaskRunState::Failed => 80,
        TaskRunState::Cancelling => 90,
        TaskRunState::Cancelled => 100,
        TaskRunState::TimedOut => 110,
    }
}

fn append_capped(target: &mut String, value: &str, max: usize, truncated: &mut bool) {
    if target.len() >= max {
        *truncated = true;
        return;
    }
    let remaining = max - target.len();
    if value.len() <= remaining {
        target.push_str(value);
        return;
    }
    let boundary = value
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= remaining)
        .last()
        .unwrap_or(0);
    target.push_str(&value[..boundary]);
    *truncated = true;
}

fn cap_string(mut value: String, max: usize) -> (String, bool) {
    if value.len() <= max {
        return (value, false);
    }
    let boundary = value
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= max)
        .last()
        .unwrap_or(0);
    value.truncate(boundary);
    (value, true)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn last_nonempty_line(value: &str) -> Option<&str> {
    value.lines().rev().find(|line| !line.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_git(path: &Path, args: &[&str]) {
        let output = StdCommand::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .expect("git fixture command starts");
        assert!(
            output.status.success(),
            "git fixture command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn task_spec(base_revision: String) -> TaskRunSpec {
        TaskRunSpec {
            task_run_id: uprava_protocol::TaskRunId::from("task-run-worktree-test"),
            project_placement_id: ProjectPlacementId::from("placement-test"),
            provider: "codex".to_owned(),
            prompt: "Change the fixture".to_owned(),
            base_revision,
            branch: "uprava/task/task-run-worktree-test".to_owned(),
            checks: vec![],
            artifact_paths: vec![],
            timeout_seconds: 60,
            ttl_seconds: 120,
            resource_limits: uprava_protocol::TaskResourceLimits::default(),
            runtime_image: "uprava/codex-runtime:test".to_owned(),
        }
    }

    #[test]
    fn shell_quote_preserves_single_quotes_without_shell_interpolation() {
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn execd_frames_are_parsed_and_capped() {
        let mut outcome = ExecdOutcome::default();
        let mut frames = b"event: command\r\ndata: {\"type\":\"init\",\"text\":\"command-1\"}\r\n\r\ndata: {\"type\":\"stdout\",\"text\":\"ok\"}\n\n".to_vec();
        consume_execd_frames(&mut frames, &mut outcome);
        parse_execd_frame(br#"{"type":"stdout","text":"ok"}"#, &mut outcome);
        parse_execd_frame(
            br#"{"type":"execution_complete","exitCode":0}"#,
            &mut outcome,
        );
        assert_eq!(outcome.command_id.as_deref(), Some("command-1"));
        assert_eq!(outcome.stdout, "okok");
        assert_eq!(outcome.exit_code, Some(0));
    }

    #[cfg(unix)]
    #[test]
    fn task_artifact_hashing_rejects_symlinks() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!("uprava-task-artifact-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("artifact fixture creates");
        let root = std::fs::canonicalize(root).expect("artifact fixture canonicalizes");
        std::fs::write(root.join("result.txt"), "bounded evidence\n")
            .expect("artifact fixture writes");
        symlink(root.join("result.txt"), root.join("result-link.txt"))
            .expect("artifact symlink creates");

        let evidence = hash_task_artifact(&root, "result.txt").expect("regular artifact hashes");
        assert_eq!(evidence.size_bytes, 17);
        let error =
            hash_task_artifact(&root, "result-link.txt").expect_err("symlink artifact rejects");
        assert_eq!(error.code, "task_run.artifact_type_rejected");
        std::fs::remove_dir_all(root).expect("artifact fixture removes");
    }

    #[test]
    fn task_worktree_creation_is_idempotent_and_branch_bound() {
        let root = std::env::temp_dir().join(format!("uprava-task-worktree-{}", Uuid::new_v4()));
        let repo = root.join("project");
        std::fs::create_dir_all(&repo).expect("repository directory creates");
        run_git(&repo, &["init", "--quiet"]);
        std::fs::write(repo.join("README.md"), "fixture\n").expect("fixture file writes");
        run_git(&repo, &["add", "README.md"]);
        run_git(
            &repo,
            &[
                "-c",
                "user.name=Uprava Test",
                "-c",
                "user.email=uprava@example.invalid",
                "commit",
                "--quiet",
                "-m",
                "fixture",
            ],
        );
        let base_revision = git_text(&repo, &["rev-parse", "HEAD"])
            .expect("fixture revision resolves")
            .trim()
            .to_owned();
        let config = NodeConfig {
            core_url: "http://127.0.0.1:8080".parse().expect("core URL parses"),
            display_name: "Task Test Node".to_owned(),
            heartbeat_interval: Duration::from_secs(5),
            state_path: root.join("node.sqlite"),
            workspace_paths: vec![root.clone()],
            codex_binary: "codex".to_owned(),
            codex_version: Some("codex-cli 0.144.1".to_owned()),
            codex_managed_unavailable_reason: None,
            codex_ignore_user_config: true,
            codex_timeout: Duration::from_secs(60),
            opensandbox_url: None,
            task_runtime_image: "uprava/codex-runtime:test".to_owned(),
            toolhive_url: "http://127.0.0.1:18081"
                .parse()
                .expect("ToolHive URL parses"),
            toolhive_timeout: Duration::from_secs(5),
        };
        let spec = task_spec(base_revision);

        let created = prepare_task_worktree(&config, &repo.display().to_string(), &spec)
            .expect("task worktree creates");
        let retried = prepare_task_worktree(&config, &repo.display().to_string(), &spec)
            .expect("same task worktree is reused");
        assert_eq!(created.path, retried.path);
        assert_eq!(created.branch, spec.branch);

        let mut conflicting = spec;
        conflicting.branch = "uprava/task/different".to_owned();
        let error = prepare_task_worktree(&config, &repo.display().to_string(), &conflicting)
            .expect_err("task id cannot reuse another branch");
        assert_eq!(error.code, "task_run.worktree_branch_mismatch");
        std::fs::remove_dir_all(root).expect("task worktree fixture removes");
    }

    #[test]
    fn sandbox_readiness_distinguishes_pending_running_and_failed() {
        let inspect = |state: &str, message: Option<&str>| SandboxInspectResponse {
            status: SandboxStatusResponse {
                state: state.to_owned(),
                message: message.map(str::to_owned),
            },
        };

        assert!(!sandbox_readiness(&inspect("Pending", None)).expect("pending remains retryable"));
        assert!(sandbox_readiness(&inspect("Running", None)).expect("running is ready"));
        let error = sandbox_readiness(&inspect("Failed", Some("image pull failed")))
            .expect_err("failed lifecycle state is terminal");
        assert_eq!(error.code, "task_run.sandbox_start_failed");
        assert_eq!(error.message, "image pull failed");
    }

    #[tokio::test]
    async fn sandbox_readiness_honours_preexisting_cancellation() {
        let (_sender, mut receiver) = watch::channel(true);
        let error = wait_sandbox_ready(
            &reqwest::Client::new(),
            &"http://127.0.0.1:9".parse().expect("URL parses"),
            "sandbox-test",
            Some(&mut receiver),
        )
        .await
        .expect_err("cancelled readiness fails before transport");

        assert_eq!(error.code, "task_run.cancelled");
    }
}
