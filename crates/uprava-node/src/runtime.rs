//! Uprava Node daemon composition root.
//!
//! The daemon owns local state, workspace/process access and PTY lifetimes. It
//! accepts only protocol v2 control traffic and persists command/event progress
//! before acknowledging delivery.

use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    fs::{self, OpenOptions},
    hash::Hash,
    io::{ErrorKind, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command as StdCommand, ExitStatus, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
#[cfg(unix)]
use std::os::unix::process::CommandExt;

use anyhow::Context;
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use pty_process::{Command as PtyCommand, Size as PtySize};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{sqlite::SqliteConnectOptions, Row, SqlitePool};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command as TokioCommand,
    sync::{mpsc, oneshot, watch, Mutex, RwLock, Semaphore},
    time::timeout,
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, http::HeaderValue, Message as WsMessage},
};
use uprava_logging::init_tracing;
#[cfg(test)]
use uprava_protocol::CommandTarget;
use uprava_protocol::{
    is_supported_protocol_version, serde_json_value::JsonValue, ActorRef, AgentExecutionProfile,
    ApiError, ApprovalId, CapabilitySummary, CapabilityValue, CommandEnvelope, CommandId,
    CommandKind, CommandPayload, CommandState, ControlFrame, CorrelationId, DeductionInputPackage,
    DeductionProviderOutput, DeductionProviderResult, EnrollmentId, EventEnvelope, EventId,
    EventKind, EventPayload, GitChangeKind, GitChangedFile, GitHeadState, GitOperation,
    GitRepositoryState, GitWorkspaceSnapshot, GitWorktreeKind, IntegrationDesiredState,
    IntegrationId, McpDependencyActualState, McpDependencyInstanceId, McpDependencyStatus,
    NodeEnrollmentClaimRequest, NodeEnrollmentClaimResponse, NodeEnrollmentRequest,
    NodeEnrollmentRequestedResponse, NodeHeartbeatRequest, NodeHeartbeatResponse, NodeId,
    ObservedCapability, ObservedCapabilityState, PlacementState, PolicyDecision,
    ProjectPlacementId, ProviderInteractionId, ProviderInteractionKind, ProviderMcpAccess,
    ProviderMcpAccessRequest, ProviderRuntimeCapability, ProviderSandboxMode, ResourceBadge,
    RuntimeAttemptActualState, RuntimeAttemptId, RuntimeAttemptState, RuntimePolicyHash,
    RuntimeSessionId, RuntimeSessionState, ScheduledMessageFailure, ScopeRef, SessionThreadId,
    SleepHint, TaskArtifactEvidence, TaskCheckResult, TaskCleanupState, TaskRunResultPackage,
    TaskRunSpec, TaskRunState, TerminalId, TextRange, ToolCallId, ToolDefinition,
    ToolDefinitionState, ToolExecutionError, ToolExecutionErrorCode, ToolExecutionKind, ToolId,
    ToolRedactionPolicy, ToolResultEnvelope, ToolRiskLevel, ToolSourceId, ToolSourceKind,
    ToolhiveBridgeAuthorizationResponse, ToolhiveBridgeMcpRequest, ToolhiveBridgeMcpResponse,
    ToolhiveBridgeVersionResponse, ToolhiveBridgeWorkloadRequest,
    ToolhiveBridgeWorkloadStatusResponse, ToolingCommandPayloadV1, ToolingCommandV1,
    ToolingEventPayloadV1, ToolingEventV1, TurnId, UpravaRef, WarningSeverity,
    WorkspaceCommandIntent, WorkspaceCommandRunRequest, WorkspaceCommandRunResponse,
    WorkspaceDiffHunk, WorkspaceDiffRequest, WorkspaceDiffResponse, WorkspaceDiffScope,
    WorkspaceEntry, WorkspaceEntryClassification, WorkspaceEntryKind, WorkspaceEntryStatus,
    WorkspaceFileContentResponse, WorkspaceFileWriteRequest, WorkspaceFileWriteResponse,
    WorkspaceSnapshot, WorkspaceTerminalOpenRequest, WorkspaceTerminalOpenResponse,
    WorkspaceTerminalOutputFrame, WorkspaceTerminalState, WorkspaceTerminalSummary,
    WorkspaceTreeResponse, CURRENT_PROTOCOL_VERSION as API_VERSION,
    TOOLHIVE_BRIDGE_CONTRACT_VERSION_V1, TOOLING_CONTRACT_VERSION_V1, TOOL_RESULT_MAX_BYTES,
};
use uuid::Uuid;

#[path = "config.rs"]
mod config;
#[path = "supervisor.rs"]
mod supervisor;

mod application;
mod managed_provider;
mod persistence;
mod provider;
mod support;
mod task;
mod terminal;
mod tool_runtime;
mod transport;
mod workspace;

use application::*;
use managed_provider::*;
use persistence::*;
use provider::*;
use support::*;
use task::*;
use terminal::*;
use tool_runtime::*;
use transport::*;
use workspace::*;

use config::NodeConfig;
use supervisor::NodeSupervisor;

type ControlFrameSender = mpsc::Sender<ControlFrame>;
type TerminalSenderRoute = Arc<RwLock<Option<ControlFrameSender>>>;

const MAX_EVENT_OUTBOX_EVENTS: usize = 1024;
const MAX_EVENT_OUTBOX_BYTES: usize = 4 * 1024 * 1024;
const MAX_EVENT_OUTBOX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const MAX_RETAINED_COMMANDS: usize = 1024;
const MAX_CODEX_TRANSCRIPT_MESSAGES: usize = 20;
const MAX_WORKSPACE_DIRECTORY_ENTRIES: usize = 100;
const MAX_WORKSPACE_TEXT_BYTES: u64 = 256 * 1024;
const MAX_WORKSPACE_COMMAND_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_WORKSPACE_COMMAND_ARGS: usize = 32;
const MAX_WORKSPACE_COMMAND_ARG_CHARS: usize = 512;
const MAX_WORKSPACE_COMMAND_SECONDS: u64 = 120;
const ALLOWED_WORKSPACE_COMMANDS: &[&str] = &[
    "cargo", "git", "make", "node", "npm", "pnpm", "bun", "rustc",
];
const WORKSPACE_DIFF_BYTES: usize = 128 * 1024;
const MAX_GIT_STATUS_BYTES: usize = 512 * 1024;
const MAX_GIT_CHANGED_FILES: usize = 1_000;
const MAX_WORKSPACE_DIFF_HUNKS: usize = 256;
const MAX_WORKSPACE_TERMINAL_REPLAY_FRAMES: usize = 256;
const MAX_WORKSPACE_TERMINAL_REPLAY_BYTES: usize = 256 * 1024;
const MAX_WORKSPACE_TERMINAL_INPUT_CHARS: usize = 16_384;
const WORKSPACE_TERMINAL_READ_BYTES: usize = 4 * 1024;
const WORKSPACE_TERMINAL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);
const MIN_WORKSPACE_TERMINAL_COLS: u16 = 20;
const MAX_WORKSPACE_TERMINAL_COLS: u16 = 300;
const MIN_WORKSPACE_TERMINAL_ROWS: u16 = 5;
const MAX_WORKSPACE_TERMINAL_ROWS: u16 = 120;
const MAX_CODEX_TRANSCRIPT_CHARS: usize = 12_000;
const MAX_PROVIDER_ACTIVITY_RAW_CHARS: usize = 16_000;
const MAX_PROVIDER_ACTIVITY_SUMMARY_CHARS: usize = 1_200;
const MAX_PROVIDER_ACTIVITY_LINE_CHARS: usize = 4_000;
const MAX_PROVIDER_ACTIVITY_EVENTS: usize = 512;
const MAX_PROVIDER_PROCESS_OUTPUT_BYTES: usize = 256 * 1024;
const MAX_PROVIDER_APPROVAL_REQUESTS: usize = 16;
const MAX_DEDUCTION_RAW_CHARS: usize = 32_000;
const MAX_CANCELLED_DEDUCTION_TOMBSTONES: usize = 1_024;
const DEDUCTION_SCHEMA_VERSION: &str = "uprava.deduction.v1";
const PROVIDER_PROCESS_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);
const NODE_STATE_SLOT: &str = "0.2.0";
const NODE_STATE_SCHEMA_VERSION: u32 = 1;
const NODE_STATE_STORE_QUEUE_CAPACITY: usize = 256;
const CONTROL_WRITER_QUEUE_CAPACITY: usize = 512;
const NODE_COMMAND_DISPATCH_QUEUE_CAPACITY: usize = 128;
const NODE_PRIORITY_COMMAND_DISPATCH_QUEUE_CAPACITY: usize = 64;
const NODE_COMMAND_DISPATCH_CONCURRENCY: usize = 8;

fn daemon_version() -> String {
    match option_env!("UPRAVA_BUILD_GIT_SHA") {
        Some(release_id) if !release_id.is_empty() && release_id != "unknown" => {
            format!("{}+{release_id}", env!("CARGO_PKG_VERSION"))
        }
        _ => env!("CARGO_PKG_VERSION").to_owned(),
    }
}

/// Start the composed Node daemon until shutdown.
pub async fn run() -> anyhow::Result<()> {
    let _log_path = init_tracing("node", "UPRAVA_NODE_LOG_FILE", ".local/logs/node.log")?;

    let config = NodeConfig::from_env()?;
    let client = reqwest::Client::new();
    let mut local_state = NodeLocalState::load_async(&config.state_path).await?;
    if reconcile_managed_attempt_descriptors(&mut local_state) {
        local_state.save_async(&config.state_path).await?;
    }
    let state_store = NodeStateStore::new(local_state, config.state_path.clone());
    tracing::info!("starting uprava node");

    NodeSupervisor::new(config, client, state_store).run().await
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
