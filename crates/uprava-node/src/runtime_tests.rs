use super::*;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::sync::{Mutex, MutexGuard, OnceLock};
use uprava_protocol::{CorrelationId, SessionThreadId};

const NODE_CONFIG_ENV_VARS: &[&str] = &[
    "UPRAVA_CORE_URL",
    "UPRAVA_NODE_DISPLAY_NAME",
    "UPRAVA_NODE_HEARTBEAT_SECONDS",
    "UPRAVA_NODE_STATE_PATH",
    "UPRAVA_NODE_WORKSPACES",
    "UPRAVA_CODEX_BINARY",
    "UPRAVA_CODEX_IGNORE_USER_CONFIG",
    "UPRAVA_CODEX_TIMEOUT_SECONDS",
    "UPRAVA_OPENSANDBOX_URL",
    "UPRAVA_TASK_RUNTIME_IMAGE",
    "UPRAVA_TOOLHIVE_URL",
    "UPRAVA_TOOLHIVE_TIMEOUT_SECONDS",
];

#[cfg(unix)]
fn fake_codex_success_binary() -> PathBuf {
    fake_codex_binary(
        r#"#!/bin/sh
output_path=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--output-last-message" ]; then
    shift
    output_path="$1"
  fi
  shift
done
if [ -z "$output_path" ]; then
  echo "missing --output-last-message" >&2
  exit 2
fi
printf '%s\n' 'Codex fake accepted' > "$output_path"
printf '%s\n' '{"type":"response.completed","session_id":"codex-session-1","resume_cursor":"cursor-1"}'
"#,
    )
}

#[cfg(unix)]
fn fake_codex_args_capture_binary(capture_path: &Path) -> PathBuf {
    fake_codex_binary(&format!(
        r#"#!/bin/sh
output_path=""
for arg in "$@"; do
  if [ "$arg" = "--output-last-message" ]; then
    capture_next=1
  elif [ "${{capture_next:-0}}" = "1" ]; then
    output_path="$arg"
    capture_next=0
  fi
done
if [ -z "$output_path" ]; then
  echo "missing --output-last-message" >&2
  exit 2
fi
printf '%s\n' "$@" > '{}'
printf '%s\n' 'Codex args accepted' > "$output_path"
printf '%s\n' '{{"type":"response.completed","session_id":"codex-session-1","resume_cursor":"cursor-1"}}'
"#,
        capture_path.display()
    ))
}

#[cfg(unix)]
fn fake_codex_prompt_capture_binary(capture_path: &Path) -> PathBuf {
    fake_codex_binary(&format!(
        r#"#!/bin/sh
output_path=""
last_arg=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--output-last-message" ]; then
    shift
    output_path="$1"
  else
    last_arg="$1"
  fi
  shift
done
if [ -z "$output_path" ]; then
  echo "missing --output-last-message" >&2
  exit 2
fi
printf '%s\n' "$last_arg" > '{}'
printf '%s\n' 'Codex contextual answer' > "$output_path"
printf '%s\n' '{{"type":"response.completed"}}'
"#,
        capture_path.display()
    ))
}

#[cfg(unix)]
fn fake_codex_resume_capture_binary(capture_path: &Path) -> PathBuf {
    fake_codex_binary(&format!(
        r#"#!/bin/sh
output_path=""
for arg in "$@"; do
  if [ "$arg" = "--output-last-message" ]; then
    capture_next=1
  elif [ "${{capture_next:-0}}" = "1" ]; then
    output_path="$arg"
    capture_next=0
  fi
done
if [ -z "$output_path" ]; then
  echo "missing --output-last-message" >&2
  exit 2
fi
printf '%s\n' "$@" > '{}'
printf '%s\n' 'Codex resume accepted' > "$output_path"
printf '%s\n' '{{"type":"response.completed","session_id":"codex-session-1","resume_cursor":"cursor-2"}}'
"#,
        capture_path.display()
    ))
}

#[cfg(unix)]
fn fake_codex_deduction_binary(capture_path: &Path) -> PathBuf {
    fake_codex_binary(&format!(
        r#"#!/bin/sh
output_path=""
schema_path=""
capture_next=""
for arg in "$@"; do
  if [ "$capture_next" = "output" ]; then
    output_path="$arg"
    capture_next=""
  elif [ "$capture_next" = "schema" ]; then
    schema_path="$arg"
    capture_next=""
  elif [ "$arg" = "--output-last-message" ]; then
    capture_next="output"
  elif [ "$arg" = "--output-schema" ]; then
    capture_next="schema"
  fi
done
if [ -z "$output_path" ] || [ -z "$schema_path" ] || [ ! -f "$schema_path" ]; then
  echo "missing structured output paths" >&2
  exit 2
fi
printf '%s\n' "$@" > '{}'
printf '%s\n' '{{"title":"Root cause","conclusion":"The session evidence supports the result.","certainty":"high","steps":[{{"step_id":"step-1","classification":"observed","summary":"The session is the bounded scope.","support_refs":[{{"kind":"session","session_thread_id":"session-1"}}]}}],"assumptions":[],"unknowns":[],"alternatives":[]}}' > "$output_path"
printf '%s\n' '{{"type":"response.completed"}}'
"#,
        capture_path.display()
    ))
}

#[cfg(unix)]
fn fake_codex_approval_request_binary() -> PathBuf {
    fake_codex_binary(
        r#"#!/bin/sh
printf '%s\n' '{"type":"approval.requested","approval_id":"approval-codex-1","prompt":"Allow file edit?"}'
exit 0
"#,
    )
}

#[cfg(unix)]
fn fake_codex_empty_output_binary() -> PathBuf {
    fake_codex_binary(
        r#"#!/bin/sh
exit 0
"#,
    )
}

#[cfg(unix)]
fn fake_codex_failing_binary() -> PathBuf {
    fake_codex_binary(
        r#"#!/bin/sh
echo "provider crashed" >&2
exit 42
"#,
    )
}

#[cfg(unix)]
fn fake_codex_slow_binary() -> PathBuf {
    fake_codex_binary(
        r#"#!/bin/sh
sleep 2
"#,
    )
}

#[cfg(unix)]
fn fake_codex_binary(script: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("fake-codex-{}", Uuid::new_v4()));
    std::fs::write(&path, script).expect("codex fixture writes");
    let mut permissions = std::fs::metadata(&path)
        .expect("codex fixture metadata reads")
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&path, permissions).expect("codex fixture executable");
    path
}

fn config_fixture() -> NodeConfig {
    config_fixture_with_codex_binary("codex")
}

fn config_fixture_with_codex_binary(codex_binary: impl Into<String>) -> NodeConfig {
    NodeConfig {
        core_url: "http://127.0.0.1:8080"
            .parse()
            .expect("test core URL parses"),
        display_name: "Test Node".to_owned(),
        heartbeat_interval: Duration::from_secs(5),
        state_path: std::env::temp_dir().join(format!("uprava-node-{}.json", Uuid::new_v4())),
        workspace_paths: vec![std::env::temp_dir()],
        codex_binary: codex_binary.into(),
        codex_version: Some("codex-cli 0.144.1".to_owned()),
        codex_managed_unavailable_reason: None,
        codex_ignore_user_config: false,
        codex_timeout: Duration::from_secs(5),
        opensandbox_url: None,
        task_runtime_image: "uprava/codex-runtime:test".to_owned(),
        toolhive_url: "http://127.0.0.1:9".parse().expect("ToolHive fixture URL"),
        toolhive_timeout: Duration::from_secs(1),
    }
}

fn command_fixture(command_id: &str, kind: CommandKind) -> CommandEnvelope {
    command_fixture_with_content(command_id, kind, "hello")
}

fn command_fixture_with_content(
    command_id: &str,
    kind: CommandKind,
    content: &str,
) -> CommandEnvelope {
    let payload = match kind {
        CommandKind::StartRuntime => CommandPayload::StartRuntime {
            provider: "codex".to_owned(),
            workspace_path: std::env::temp_dir().display().to_string(),
            execution_profile: AgentExecutionProfile::ExecCompatibility,
            effective_policy: None,
            effective_policy_hash: None,
        },
        CommandKind::ResumeRuntime => CommandPayload::ResumeRuntime {
            provider: "codex".to_owned(),
            workspace_path: std::env::temp_dir().display().to_string(),
            provider_resume_ref: None,
            execution_profile: AgentExecutionProfile::ExecCompatibility,
            effective_policy: None,
            effective_policy_hash: None,
        },
        CommandKind::SendTurn => CommandPayload::SendTurn {
            turn_id: TurnId::from("turn-1"),
            content: content.to_owned(),
            collaboration_mode: None,
        },
        CommandKind::ResolveApproval => CommandPayload::ResolveApproval {
            approval_id: ApprovalId::from("approval-1"),
            provider_interaction_id: None,
            approved: true,
            message: Some("approved".to_owned()),
        },
        CommandKind::SubmitUserInput => CommandPayload::SubmitUserInput {
            provider_interaction_id: ProviderInteractionId::from("interaction-1"),
            answers: vec!["fixture".to_owned()],
        },
        CommandKind::InterruptRuntime => CommandPayload::InterruptRuntime {
            runtime_attempt_id: None,
        },
        CommandKind::StopRuntime => CommandPayload::StopRuntime {
            runtime_attempt_id: None,
            reason: None,
        },
        _ => CommandPayload::Extension {
            name: "test.fixture".to_owned(),
            value: JsonValue(serde_json::json!({})),
        },
    };
    CommandEnvelope {
        command_id: CommandId::from(command_id),
        kind,
        target: CommandTarget::SessionRuntime {
            node_id: NodeId::from("node-1"),
            project_placement_id: ProjectPlacementId::from("placement-1"),
            session_thread_id: SessionThreadId::from("session-1"),
            runtime_session_id: RuntimeSessionId::from("runtime-1"),
        },
        actor_ref: ActorRef::local_user(),
        source_refs: vec![],
        cause_refs: vec![],
        issued_at: Utc::now(),
        correlation_id: CorrelationId::from("correlation-1"),
        payload,
    }
}

fn placement_command_fixture(
    command_id: &str,
    placement_id: &str,
    display_name: &str,
    workspace_path: &str,
) -> CommandEnvelope {
    CommandEnvelope {
        command_id: CommandId::from(command_id),
        kind: CommandKind::ValidateWorkspace,
        target: CommandTarget::Placement {
            node_id: NodeId::from("node-1"),
            project_placement_id: ProjectPlacementId::from(placement_id),
        },
        actor_ref: ActorRef::local_user(),
        source_refs: vec![],
        cause_refs: vec![],
        issued_at: Utc::now(),
        correlation_id: CorrelationId::from("correlation-1"),
        payload: CommandPayload::ValidateWorkspace {
            display_name: display_name.to_owned(),
            workspace_path: workspace_path.to_owned(),
        },
    }
}

fn env_lock() -> MutexGuard<'static, ()> {
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("env lock is not poisoned")
}

struct EnvGuard {
    values: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    fn cleared(names: &[&'static str]) -> Self {
        let values = names
            .iter()
            .map(|name| {
                let value = std::env::var(name).ok();
                std::env::remove_var(name);
                (*name, value)
            })
            .collect();
        Self { values }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (name, value) in self.values.drain(..) {
            if let Some(value) = value {
                std::env::set_var(name, value);
            } else {
                std::env::remove_var(name);
            }
        }
    }
}

fn event_ids(events: &[EventEnvelope]) -> Vec<EventId> {
    events.iter().map(|event| event.event_id.clone()).collect()
}

fn event_kinds(events: &[EventEnvelope]) -> Vec<EventKind> {
    events.iter().map(|event| event.kind).collect()
}

fn badge_kinds(badges: &[ResourceBadge]) -> Vec<&str> {
    badges.iter().map(|badge| badge.kind.as_str()).collect()
}

#[path = "runtime/tests/dispatch.rs"]
mod dispatch;
#[path = "runtime/tests/provider.rs"]
mod provider;
#[path = "runtime/tests/reliability.rs"]
mod reliability;
#[path = "runtime/tests/state.rs"]
mod state;
#[path = "runtime/tests/terminal.rs"]
mod terminal;
#[path = "runtime/tests/tooling.rs"]
mod tooling;
#[path = "runtime/tests/transport.rs"]
mod transport;
#[path = "runtime/tests/workspace.rs"]
mod workspace;
