//! Uprava Core composition and application boundary.
//!
//! Core is authoritative for identity, legal transitions, durable command and
//! event state. Node-originated effects are committed before publication; API
//! failures use the protocol error envelope and protocol v2 is the only 0.2.0
//! wire compatibility boundary.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    convert::Infallible,
    fs::OpenOptions,
    io::Write,
    net::SocketAddr,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
    time::Instant,
};

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    body::Body,
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        ConnectInfo, Path, Query, Request, State,
    },
    http::{
        header::{HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE, COOKIE, SET_COOKIE},
        HeaderMap, Method, Response, StatusCode,
    },
    middleware::{self, Next},
    response::{IntoResponse, Sse},
    routing::{get, patch, post, put},
    Json, Router,
};
use chrono::{DateTime, Duration as ChronoDuration, Timelike, Utc};
use futures_util::{SinkExt, Stream, StreamExt};
use jiff::{civil::Weekday, Timestamp};
use rand_core::{OsRng, RngCore};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection, SqlitePool};
use subtle::ConstantTimeEq;
use tokio::sync::{broadcast, mpsc, oneshot, Mutex, RwLock, Semaphore};
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    limit::RequestBodyLimitLayer,
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use tracing::Instrument;
use uprava_protocol::{
    is_supported_protocol_version, serde_json_value::JsonValue, AcknowledgeWarningRequest,
    ActionCapability, ActorRef, AgentExecutionProfile, AgentProjection, ApiError, ApprovalId,
    ApprovalState, ApproveNodeEnrollmentResponse, ArtifactId, BlockId, CapabilitySummary,
    CapabilityValue, CausalityLinks, ClientCreateNodeEnrollmentRequest, ClientLogLevel,
    ClientLogRequest, ClientLogResponse, CommandAcceptedResponse, CommandEnvelope, CommandId,
    CommandKind, CommandPayload, CommandState, CommandTarget, ControlFrame, CorrelationId,
    CreateDeductionRequest, CreateJobRequest, CreatePlacementRequest,
    CreateScheduledMessageRequest, CreateSessionRequest, CreateTaskRunRequest,
    DeductionAcceptedResponse, DeductionBlock, DeductionClassification, DeductionEvidenceEvent,
    DeductionId, DeductionInputPackage, DeductionProvenance, DeductionProviderOutput,
    DeductionProviderResult, DeductionRecord, DeductionState, EffectiveRuntimePolicy, EnrollmentId,
    EnrollmentState, EventEnvelope, EventId, EventKind, EventLogPage, EventPayload,
    EventPayloadKind, EvidenceId, GitWorkspaceSnapshot, HealthResponse, InventorySnapshot,
    JobDetail, JobId, JobOverlapPolicy, JobRunId, JobRunState, JobRunSummary, JobRunTrigger,
    JobSchedule, JobSummary, McpAccessToken, McpDependencyActualState, Message, MessageId,
    MessageRole, NodeCredentialRotationResponse, NodeDeletionResponse, NodeEnrollmentClaimRequest,
    NodeEnrollmentClaimResponse, NodeEnrollmentRequest, NodeEnrollmentRequestedResponse,
    NodeEnrollmentSummary, NodeHeartbeatRequest, NodeHeartbeatResponse, NodeId, NodePresence,
    NodeRevocationResponse, PersistDeductionResponse, PlacementDeletionResponse, PlacementState,
    PreviewSessionPolicyRequest, ProjectId, ProjectPlacementId, ProjectPlacementSummary,
    ProviderApprovalMode, ProviderInteractionId, ProviderInteractionKind, ProviderInteractionState,
    ProviderInteractionSummary, ProviderMcpAccess, ProviderMcpAccessRequest, ProviderQuotaState,
    ProviderQuotaStatus, ProviderRuntimeCapability, ProviderSandboxMode, ReferenceResolution,
    ReferenceResolutionStatus, ResolveApprovalRequest, ResolveReferenceRequest, ResourceBadge,
    RunJobRequest, RuntimeAttemptActualState, RuntimeAttemptId, RuntimeAttemptState,
    RuntimeAttemptSummary, RuntimeNetworkPosture, RuntimePolicyHash, RuntimeRecoveryStatus,
    RuntimeSessionId, RuntimeSessionState, RuntimeSummary, RuntimeToolExposureSummary,
    ScheduledMessageFailure, ScheduledMessageState, ScheduledSessionMessage, ScopeRef,
    SecurityMode, SecurityStatus, SendTurnRequest, SessionDetail, SessionEvidenceProjection,
    SessionEvidenceProjectionNode, SessionPolicyPreview, SessionSummary, SessionThreadId,
    SessionThreadState, SessionTraceProjection, SleepHint, SubmitProviderInputRequest,
    TaskCleanupState, TaskRunDetail, TaskRunId, TaskRunListResponse, TaskRunResultPackage,
    TaskRunSpec, TaskRunState, TaskRunSummary, TerminalId, TextRange, TracePrecision, TraceStep,
    TurnId, TurnState, UpdateJobRequest, UpdateScheduledMessageRequest, UpravaRef, VersionResponse,
    WarningAcknowledgementResponse, WarningSeverity, WebAuthLoginRequest, WebAuthResponse,
    WebAuthSetupRequest, WebAuthStatusResponse, WorkspaceCheckRunSummary,
    WorkspaceCommandHistoryItem, WorkspaceCommandHistoryResponse, WorkspaceCommandIntent,
    WorkspaceCommandRunRequest, WorkspaceCommandRunResponse, WorkspaceDiffRequest,
    WorkspaceDiffResponse, WorkspaceFileContentResponse, WorkspaceFileWriteRequest,
    WorkspaceFileWriteResponse, WorkspaceReviewProjection, WorkspaceSnapshot,
    WorkspaceTerminalClientFrame, WorkspaceTerminalListResponse, WorkspaceTerminalOpenRequest,
    WorkspaceTerminalOpenResponse, WorkspaceTerminalState, WorkspaceTerminalStreamFrame,
    WorkspaceTerminalSummary, WorkspaceTreeResponse, CURRENT_PROTOCOL_VERSION as API_VERSION,
};
use uuid::Uuid;

#[path = "config.rs"]
mod config;
#[path = "domain.rs"]
mod domain;
#[path = "observability.rs"]
mod observability;
#[path = "persistence.rs"]
mod persistence;

mod application;
mod support;
mod transport;

use application::*;
use persistence::*;
use support::*;
use transport::*;

pub use support::AppError;
pub use transport::{build_router, shutdown_signal};

#[cfg(test)]
use config::default_allowed_origins;
pub use config::{AppConfig, ConfigError};
use domain::PlacementIdentity;
use observability::CoreMetrics;
use persistence::{CORE_STATE_SLOT, SCHEMA_VERSION};
#[cfg(test)]
use uprava_protocol::DeploymentProfile;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const CORRELATION_ID_HEADER: &str = "x-correlation-id";
const REQUEST_ID_HEADER: &str = "x-request-id";
const CSRF_HEADER: &str = "x-uprava-csrf";
const SESSION_COOKIE: &str = "uprava_session";
const CSRF_COOKIE: &str = "uprava_csrf";
const MAX_CLIENT_LOG_FIELD_CHARS: usize = 2_000;
const MAX_CLIENT_LOG_DETAIL_CHARS: usize = 8_000;
const MAX_CLIENT_LOG_BYTES: u64 = 5 * 1024 * 1024;
const DEFAULT_EVENT_LOG_LIMIT: usize = 100;
const MAX_EVENT_LOG_LIMIT: usize = 500;
const MAX_DEDUCTION_EVENTS: usize = 100;
const MAX_DEDUCTION_TRACE_STEPS: usize = 80;
const MAX_DEDUCTION_QUESTION_CHARS: usize = 2_000;
const MAX_DEDUCTION_RAW_CHARS: usize = 32_000;
const DEDUCTION_SCHEMA_VERSION: &str = "uprava.deduction.v1";
const MAX_CLIENT_LOG_FILES: usize = 3;
const MIN_LOCAL_PASSWORD_CHARS: usize = 12;
const AUTH_FAILURE_LIMIT: usize = 10;
const AUTH_FAILURE_WINDOW_SECONDS: i64 = 60;
const PUBLIC_NODE_RATE_LIMIT: usize = 600;
const PUBLIC_STREAM_RATE_LIMIT: usize = 60;
const PUBLIC_CONCURRENCY_LIMIT: usize = 64;
const WORKSPACE_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const WORKSPACE_INTERVENTION_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_TERMINAL_INPUT_CHARS: usize = 16_384;
const MIN_TERMINAL_COLS: u16 = 20;
const MAX_TERMINAL_COLS: u16 = 300;
const MIN_TERMINAL_ROWS: u16 = 5;
const MAX_TERMINAL_ROWS: u16 = 120;
const HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(130);
const MAX_HTTP_REQUEST_BODY_BYTES: usize = 1024 * 1024;
const CONTROL_QUEUE_CAPACITY: usize = 256;
const MAX_CONTROL_FRAME_BYTES: usize = 1024 * 1024;
const MAX_EVENT_BATCH_ITEMS: usize = 128;
const MAX_TERMINAL_OUTPUT_CHARS: usize = 65_536;
const MAX_CONTROL_JSON_DEPTH: usize = 32;
const MAX_CONTROL_STRING_CHARS: usize = 65_536;
const SCHEDULED_MESSAGE_TICK: Duration = Duration::from_millis(250);
const MAX_SCHEDULED_MESSAGE_CONTENT_CHARS: usize = 65_536;
const MAX_SCHEDULED_MESSAGE_TIMEZONE_CHARS: usize = 100;
const JOB_SCHEDULER_TICK: Duration = Duration::from_millis(500);
const MAX_JOB_NAME_CHARS: usize = 200;
const MAX_JOB_PROMPT_CHARS: usize = 65_536;
const MAX_JOB_TIMEZONE_CHARS: usize = 100;
const PROVIDER_QUOTA_FRESH_SECONDS: i64 = 300;
const PROVIDER_QUOTA_BLOCK_PERCENT: u8 = 5;

tokio::task_local! {
    static REQUEST_CORRELATION_ID: CorrelationId;
}

pub struct AppState {
    config: AppConfig,
    pool: SqlitePool,
    control_connections: ConnectionRegistry,
    event_tx: broadcast::Sender<EventEnvelope>,
    event_ingest_lock: Mutex<()>,
    enrollment_create_lock: Mutex<()>,
    client_log_lock: Mutex<()>,
    event_publish_lock: Mutex<()>,
    command_result_tx: broadcast::Sender<CommandResultNotice>,
    command_waiters: StdMutex<HashMap<String, oneshot::Sender<CommandResultNotice>>>,
    terminal_hub: TerminalHub,
    workspace_terminals: RwLock<HashMap<String, WorkspaceTerminalSummary>>,
    auth_failures: RwLock<HashMap<String, Vec<DateTime<Utc>>>>,
    public_requests: RwLock<HashMap<String, Vec<DateTime<Utc>>>>,
    public_concurrency: Arc<Semaphore>,
    core_metrics: Arc<CoreMetrics>,
    mcp_lease_signing_key: [u8; 32],
}

impl AppState {
    pub async fn new(config: AppConfig, pool: SqlitePool) -> Result<Arc<Self>, AppError> {
        let state = Self::build_uninitialized(config, pool);
        state.migrate().await?;
        bootstrap_bundled_plugins(&state).await?;
        revoke_all_mcp_leases_for_credential_rotation(&state).await?;
        seed_uprava_native_tools(&state).await?;
        seed_external_tool_sources(&state).await?;
        recover_interrupted_scheduled_messages(&state).await?;
        spawn_scheduled_message_dispatcher(Arc::downgrade(&state));
        recover_job_runs(&state).await?;
        spawn_job_scheduler(Arc::downgrade(&state));
        Ok(state)
    }

    fn build_uninitialized(config: AppConfig, pool: SqlitePool) -> Arc<Self> {
        let mut mcp_lease_signing_key = [0u8; 32];
        OsRng.fill_bytes(&mut mcp_lease_signing_key);
        Arc::new(Self {
            config,
            pool,
            control_connections: ConnectionRegistry::new(),
            event_tx: broadcast::channel(256).0,
            event_ingest_lock: Mutex::new(()),
            enrollment_create_lock: Mutex::new(()),
            client_log_lock: Mutex::new(()),
            event_publish_lock: Mutex::new(()),
            command_result_tx: broadcast::channel(256).0,
            command_waiters: StdMutex::new(HashMap::new()),
            terminal_hub: TerminalHub::new(),
            workspace_terminals: RwLock::new(HashMap::new()),
            auth_failures: RwLock::new(HashMap::new()),
            public_requests: RwLock::new(HashMap::new()),
            public_concurrency: Arc::new(Semaphore::new(PUBLIC_CONCURRENCY_LIMIT)),
            core_metrics: Arc::new(CoreMetrics::default()),
            mcp_lease_signing_key,
        })
    }

    /// Issues and rotates a short-lived session-scoped credential for the
    /// Uprava MCP endpoint. The returned token must stay outside model input
    /// and transcripts.
    pub async fn create_mcp_access_lease(
        &self,
        session_id: &SessionThreadId,
        actor_ref: ActorRef,
    ) -> Result<(String, uprava_protocol::McpAccessLeaseClaims), AppError> {
        issue_mcp_access_lease(self, session_id, actor_ref).await
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
