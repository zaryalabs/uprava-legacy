use std::sync::{Mutex, MutexGuard, OnceLock};

use axum::{
    body::{to_bytes, Body},
    http::Request,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tower::ServiceExt;

use super::*;

const CORE_CONFIG_ENV_VARS: &[&str] = &[
    "UPRAVA_CORE_BIND",
    "UPRAVA_ALLOWED_ORIGINS",
    "UPRAVA_DATABASE_URL",
    "UPRAVA_DEPLOYMENT_PROFILE",
    "UPRAVA_HEARTBEAT_STALE_SECONDS",
    "UPRAVA_HEARTBEAT_OFFLINE_SECONDS",
    "UPRAVA_ENROLLMENT_TTL_SECONDS",
    "UPRAVA_MAX_PENDING_ENROLLMENTS",
    "UPRAVA_RUNTIME_EXPIRY_SECONDS",
    "UPRAVA_AUTO_APPROVE_ENROLLMENTS",
    "UPRAVA_AUTO_APPROVE_NODE_NAME",
    "UPRAVA_CLIENT_LOG_FILE",
    "UPRAVA_WEB_AUTH",
    "UPRAVA_WEB_SESSION_TTL_SECONDS",
    "UPRAVA_COOKIE_SECURE",
    "UPRAVA_CORE_SHUTDOWN_TIMEOUT_SECONDS",
    "UPRAVA_PUBLIC_RATE_WINDOW_SECONDS",
    "UPRAVA_PUBLIC_GLOBAL_RATE_LIMIT",
    "UPRAVA_PUBLIC_PEER_RATE_LIMIT",
    "UPRAVA_GENERATED_UI_BUILDER_URL",
    "UPRAVA_GENERATED_UI_BUILDER_TIMEOUT_SECONDS",
];

#[test]
fn public_peer_rate_policy_keeps_sensitive_budgets_independent() {
    assert_eq!(
        public_peer_rate_policy("/api/v1/auth/status", 600),
        ("auth", 30)
    );
    assert_eq!(
        public_peer_rate_policy("/api/v1/node-enrollments", 600),
        ("enrollment", 30)
    );
    assert_eq!(
        public_peer_rate_policy("/api/v1/client/logs", 600),
        ("client_logs", 120)
    );
    assert_eq!(
        public_peer_rate_policy("/api/v1/node/heartbeat", 600),
        ("node", PUBLIC_NODE_RATE_LIMIT)
    );
    assert_eq!(
        public_peer_rate_policy("/api/v1/sessions/session-1/stream", 600),
        ("stream", PUBLIC_STREAM_RATE_LIMIT)
    );
    assert_eq!(
        public_peer_rate_policy("/api/v1/inventory", 600),
        ("ui", 600)
    );
}

async fn test_state() -> Arc<AppState> {
    test_state_with_runtime_expiry(86_400).await
}

async fn activate_test_connection(
    state: &AppState,
    node_id: NodeId,
) -> (NodeContext, mpsc::Receiver<ControlFrame>) {
    let (sender, receiver) = mpsc::channel(CONTROL_QUEUE_CAPACITY);
    let context = state.control_connections.context(node_id, sender);
    state.control_connections.activate(&context).await;
    (context, receiver)
}

async fn test_state_with_runtime_expiry(runtime_expiry_seconds: i64) -> Arc<AppState> {
    let pool = memory_pool().await;
    AppState::new(test_config(runtime_expiry_seconds), pool)
        .await
        .expect("state migrates")
}

async fn test_state_with_web_auth() -> Arc<AppState> {
    let pool = memory_pool().await;
    let mut config = test_config(86_400);
    config.profile = DeploymentProfile::ControlledDev;
    config.web_auth_required = true;
    AppState::new(config, pool).await.expect("state migrates")
}

async fn memory_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(":memory:")
                .busy_timeout(Duration::from_secs(5))
                .create_if_missing(true),
        )
        .await
        .expect("sqlite opens");
    pool
}

async fn sqlite_file_pool(path: &std::path::Path) -> SqlitePool {
    sqlite_file_pool_with_connections(path, 1).await
}

async fn sqlite_file_pool_with_connections(
    path: &std::path::Path,
    max_connections: u32,
) -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(max_connections)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .busy_timeout(Duration::from_secs(5))
                .create_if_missing(true),
        )
        .await
        .expect("sqlite file opens");
    pool
}

fn remove_sqlite_file_set(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(std::path::PathBuf::from(format!(
        "{}-shm",
        path.to_string_lossy()
    )));
    let _ = std::fs::remove_file(std::path::PathBuf::from(format!(
        "{}-wal",
        path.to_string_lossy()
    )));
}

fn test_config(runtime_expiry_seconds: i64) -> AppConfig {
    AppConfig {
        bind_address: "127.0.0.1:0".to_owned(),
        database_url: "sqlite::memory:".to_owned(),
        profile: DeploymentProfile::ControlledDev,
        allowed_origins: default_allowed_origins(),
        stale_after_seconds: 15,
        offline_after_seconds: 45,
        enrollment_ttl_seconds: 600,
        max_pending_enrollments: 100,
        runtime_expiry_seconds,
        auto_approve_enrollments: false,
        auto_approve_node_name: None,
        client_log_file: std::env::temp_dir()
            .join(format!("uprava-client-log-{}.jsonl", Uuid::new_v4())),
        web_auth_required: false,
        web_session_ttl_seconds: 86_400,
        cookie_secure: false,
        core_shutdown_timeout_seconds: 5,
        public_rate_window_seconds: 60,
        public_global_rate_limit: 5_000,
        public_peer_rate_limit: 600,
        generated_ui_builder_url: "http://127.0.0.1:18082".to_owned(),
        generated_ui_builder_timeout_seconds: 2,
    }
}

fn set_cookie_header(response: &axum::response::Response) -> String {
    response
        .headers()
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split_once(';').map(|(cookie, _)| cookie.to_owned()))
        .collect::<Vec<_>>()
        .join("; ")
}

fn csrf_from_cookie_header(cookie_header: &str) -> String {
    cookie_header
        .split(';')
        .filter_map(|cookie| cookie.trim().split_once('='))
        .find_map(|(name, value)| (name == CSRF_COOKIE).then(|| value.to_owned()))
        .expect("csrf cookie exists")
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

async fn enroll_test_node(state: &Arc<AppState>) -> NodeEnrollmentClaimResponse {
    let requested = create_enrollment(
        state,
        "Test node",
        Some("0.1.0"),
        vec![CapabilitySummary {
            key: "provider.codex".to_owned(),
            value: CapabilityValue::provider(true),
        }],
    )
    .await
    .expect("enrollment creates");
    let _ = approve_node_enrollment(
        State(state.clone()),
        Path(requested.enrollment_id.to_string()),
    )
    .await
    .expect("enrollment approves");
    claim_enrollment(
        state,
        &NodeEnrollmentClaimRequest {
            enrollment_id: requested.enrollment_id,
            pairing_code: requested.pairing_code,
        },
    )
    .await
    .expect("enrollment claims")
}

async fn create_test_session(state: &Arc<AppState>) -> (NodeId, SessionDetail, std::path::PathBuf) {
    let claim = enroll_test_node(state).await;
    let node_id = claim.node_id.clone().expect("node id returned");
    heartbeat_test_node(state, node_id.clone(), claim.credential.clone()).await;
    let workspace_path = std::env::temp_dir().join(format!("uprava-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace dir creates");
    let placement = validate_placement(
        State(state.clone()),
        Json(CreatePlacementRequest {
            node_id: node_id.clone(),
            display_name: "workspace".to_owned(),
            workspace_path: workspace_path.display().to_string(),
        }),
    )
    .await
    .expect("placement validates")
    .0;
    accept_workspace_validation_event(
        state,
        &placement,
        node_id.clone(),
        PlacementState::Validated,
        vec![],
    )
    .await;
    let detail = create_session(
        State(state.clone()),
        Json(CreateSessionRequest {
            project_placement_id: placement.project_placement_id,
            title: Some("Session".to_owned()),
            provider: "codex".to_owned(),
            execution_profile: Some(AgentExecutionProfile::ExecCompatibility),
            force: false,
        }),
    )
    .await
    .expect("session creates")
    .0;
    (node_id, detail, workspace_path)
}

async fn heartbeat_test_node(state: &Arc<AppState>, node_id: NodeId, credential: Option<String>) {
    heartbeat_node(state, node_id, credential, "Test node", SleepHint::Awake, 0).await;
}

async fn heartbeat_node(
    state: &Arc<AppState>,
    node_id: NodeId,
    credential: Option<String>,
    display_name: &str,
    sleep_hint: SleepHint,
    active_runtime_count: i64,
) {
    let _ = node_heartbeat(
        State(state.clone()),
        credential.as_deref(),
        Json(NodeHeartbeatRequest {
            node_id: Some(node_id),
            display_name: display_name.to_owned(),
            daemon_version: "0.1.0".to_owned(),
            capabilities: vec![CapabilitySummary {
                key: "provider.codex".to_owned(),
                value: CapabilityValue::provider(true),
            }],
            observed_capabilities: vec![],
            dependency_statuses: vec![],
            diagnostics: None,
            active_runtime_count,
            sleep_hint,
            workspace_summaries: vec![],
        }),
    )
    .await
    .expect("heartbeat accepted");
}

async fn mark_node_offline(state: &Arc<AppState>, node_id: &NodeId) {
    age_node_heartbeat(state, node_id, state.config.offline_after_seconds + 1).await;
}

async fn age_node_heartbeat(state: &Arc<AppState>, node_id: &NodeId, age_seconds: i64) {
    let last_heartbeat_at = Utc::now() - chrono::Duration::seconds(age_seconds);
    sqlx::query(
            "update nodes set presence = 'reachable', last_heartbeat_at = ?1, updated_at = ?2 where node_id = ?3",
        )
        .bind(last_heartbeat_at)
        .bind(Utc::now())
        .bind(node_id.as_str())
        .execute(&state.pool)
        .await
        .expect("node heartbeat is aged");
}

async fn set_node_capabilities(
    state: &Arc<AppState>,
    node_id: &NodeId,
    capabilities: Vec<CapabilitySummary>,
) {
    let now = Utc::now();
    sqlx::query("update nodes set capabilities_json = ?1, updated_at = ?2 where node_id = ?3")
        .bind(serde_json::to_string(&capabilities).expect("capabilities serialize"))
        .bind(now)
        .bind(node_id.as_str())
        .execute(&state.pool)
        .await
        .expect("node capabilities update");
    replace_node_capabilities(state, node_id, &capabilities, now)
        .await
        .expect("normalized node capabilities update");
}

async fn set_session_runtime_state(
    state: &Arc<AppState>,
    detail: &SessionDetail,
    runtime_state: RuntimeSessionState,
) {
    sqlx::query("update runtime_sessions set state = ?1 where runtime_session_id = ?2")
        .bind(format_runtime_state(runtime_state))
        .bind(detail.session.runtime.runtime_session_id.as_str())
        .execute(&state.pool)
        .await
        .expect("runtime state updates");
}

async fn set_session_runtime_last_step(
    state: &Arc<AppState>,
    detail: &SessionDetail,
    last_runtime_step_at: DateTime<Utc>,
) {
    sqlx::query(
        "update runtime_sessions set last_runtime_step_at = ?1 where runtime_session_id = ?2",
    )
    .bind(last_runtime_step_at)
    .bind(detail.session.runtime.runtime_session_id.as_str())
    .execute(&state.pool)
    .await
    .expect("runtime last step updates");
}

async fn command_count(state: &Arc<AppState>) -> i64 {
    sqlx::query_scalar("select count(*) from commands")
        .fetch_one(&state.pool)
        .await
        .expect("command count loads")
}

async fn turn_id_for_command(state: &Arc<AppState>, command_id: &CommandId) -> TurnId {
    let turn_id: String = sqlx::query_scalar("select turn_id from turns where command_id = ?1")
        .bind(command_id.as_str())
        .fetch_one(&state.pool)
        .await
        .expect("turn id loads");
    TurnId::from(turn_id)
}

async fn session_message_count(state: &Arc<AppState>, detail: &SessionDetail) -> i64 {
    sqlx::query_scalar("select count(*) from messages where session_thread_id = ?1")
        .bind(detail.session.session_thread_id.as_str())
        .fetch_one(&state.pool)
        .await
        .expect("message count loads")
}

async fn accept_workspace_validation_event(
    state: &Arc<AppState>,
    placement: &ProjectPlacementSummary,
    node_id: NodeId,
    placement_state: PlacementState,
    resource_badges: Vec<ResourceBadge>,
) {
    accept_placement_snapshot_event(
        state,
        placement,
        node_id,
        EventKind::WorkspaceValidated,
        placement_state,
        resource_badges,
    )
    .await;
}

async fn accept_placement_snapshot_event(
    state: &Arc<AppState>,
    placement: &ProjectPlacementSummary,
    node_id: NodeId,
    kind: EventKind,
    placement_state: PlacementState,
    resource_badges: Vec<ResourceBadge>,
) {
    accept_placement_snapshot_event_with_git(
        state,
        placement,
        node_id,
        kind,
        placement_state,
        resource_badges,
        None,
    )
    .await;
}

async fn accept_placement_snapshot_event_with_git(
    state: &Arc<AppState>,
    placement: &ProjectPlacementSummary,
    node_id: NodeId,
    kind: EventKind,
    placement_state: PlacementState,
    resource_badges: Vec<ResourceBadge>,
    git_snapshot: Option<GitWorkspaceSnapshot>,
) {
    accept_node_event(
        state,
        EventEnvelope {
            event_id: EventId::new(),
            command_id: None,
            correlation_id: None,
            actor_ref: ActorRef::Node {
                node_id: node_id.clone(),
            },
            scope_ref: ScopeRef::Placement {
                project_placement_id: placement.project_placement_id.clone(),
            },
            node_id: Some(node_id),
            runtime_session_id: None,
            session_thread_id: None,
            turn_id: None,
            seq: 1,
            session_projection_seq: None,
            kind,
            happened_at: Utc::now(),
            source_refs: vec![],
            evidence_refs: vec![],
            cause_refs: vec![],
            result_refs: vec![],
            payload: EventPayload::from_json(
                kind,
                json!({
                    "placement_id": placement.project_placement_id.as_str(),
                    "display_name": placement.display_name.as_str(),
                    "workspace_path": placement.workspace_path.as_str(),
                    "state": placement_state,
                    "resource_badges": resource_badges,
                    "git_snapshot": git_snapshot,
                }),
            ),
        },
    )
    .await
    .expect("workspace validation event accepts");
}

fn command_fixture(command_id: CommandId, node_id: NodeId) -> CommandEnvelope {
    CommandEnvelope {
        command_id,
        kind: CommandKind::RefreshResourceSnapshot,
        target: CommandTarget::Node { node_id },
        actor_ref: ActorRef::local_user(),
        source_refs: vec![],
        cause_refs: vec![],
        issued_at: Utc::now(),
        correlation_id: CorrelationId::from("correlation-1"),
        payload: CommandPayload::RefreshResourceSnapshot {
            display_name: "fixture".to_owned(),
            workspace_path: "/workspace".to_owned(),
        },
    }
}

fn node_event_fixture(
    detail: &SessionDetail,
    node_id: NodeId,
    event_id: &str,
    seq: i64,
    kind: EventKind,
    payload: serde_json::Value,
) -> EventEnvelope {
    let runtime_session_id = detail.session.runtime.runtime_session_id.clone();
    EventEnvelope {
        event_id: EventId::from(event_id),
        command_id: None,
        correlation_id: None,
        actor_ref: ActorRef::Provider {
            provider: "codex".to_owned(),
        },
        scope_ref: ScopeRef::Runtime {
            runtime_session_id: runtime_session_id.clone(),
        },
        node_id: Some(node_id),
        runtime_session_id: Some(runtime_session_id),
        session_thread_id: Some(detail.session.session_thread_id.clone()),
        turn_id: Some(TurnId::from("turn-1")),
        seq,
        session_projection_seq: None,
        kind,
        happened_at: Utc::now(),
        source_refs: vec![],
        evidence_refs: vec![],
        cause_refs: vec![],
        result_refs: vec![],
        payload: EventPayload::from_json(kind, payload),
    }
}

#[path = "runtime/tests/artifacts.rs"]
mod artifacts;
#[path = "runtime/tests/control.rs"]
mod control;
#[path = "runtime/tests/dynamic_ui.rs"]
mod dynamic_ui;
#[path = "runtime/tests/event.rs"]
mod event;
#[path = "runtime/tests/foundation.rs"]
mod foundation;
#[path = "runtime/tests/http.rs"]
mod http;
#[path = "runtime/tests/node.rs"]
mod node;
#[path = "runtime/tests/persistence.rs"]
mod persistence;
#[path = "runtime/tests/plugins.rs"]
mod plugins;
#[path = "runtime/tests/runtime.rs"]
mod runtime;
#[path = "runtime/tests/scheduling.rs"]
mod scheduling;
#[path = "runtime/tests/session.rs"]
mod session;
#[path = "runtime/tests/task.rs"]
mod task;
#[path = "runtime/tests/tooling.rs"]
mod tooling;
#[path = "runtime/tests/workspace.rs"]
mod workspace;
