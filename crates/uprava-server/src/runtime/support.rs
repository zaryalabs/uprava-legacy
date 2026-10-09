//! Core serialization, security, logging and error-boundary helpers.

use super::*;

pub(crate) fn scope_key(scope_ref: &ScopeRef) -> String {
    match scope_ref {
        ScopeRef::Runtime { runtime_session_id } => format!("runtime:{}", runtime_session_id),
        ScopeRef::RuntimeAttempt { runtime_attempt_id } => {
            format!("runtime_attempt:{runtime_attempt_id}")
        }
        ScopeRef::ProviderInteraction {
            provider_interaction_id,
        } => format!("provider_interaction:{provider_interaction_id}"),
        ScopeRef::Session { session_thread_id } => format!("session:{}", session_thread_id),
        ScopeRef::Node { node_id } => format!("node:{}", node_id),
        ScopeRef::Placement {
            project_placement_id,
        } => {
            format!("placement:{project_placement_id}")
        }
        ScopeRef::TaskRun { task_run_id } => format!("task_run:{task_run_id}"),
        ScopeRef::Unknown { scope } => format!("unknown:{scope}"),
    }
}

#[cfg(test)]
pub(crate) fn compute_resource_badges(path: &str) -> Vec<ResourceBadge> {
    let mut badges = Vec::new();
    if path.contains("dirty") {
        badges.push(ResourceBadge {
            kind: "dirty_workspace".to_owned(),
            severity: WarningSeverity::Warning,
            label: "Dirty workspace".to_owned(),
        });
    }
    if path.contains("readonly") {
        badges.push(ResourceBadge {
            kind: "read_only_workspace".to_owned(),
            severity: WarningSeverity::HardBlock,
            label: "Read-only workspace".to_owned(),
        });
    }
    badges
}

#[cfg(test)]
pub(crate) fn workspace_snapshot_from_request(
    display_name: &str,
    workspace_path: &str,
    fallback_state: PlacementState,
) -> WorkspaceSnapshot {
    let path = std::path::Path::new(workspace_path);
    let (state, mut badges) = if !path.exists() {
        (
            PlacementState::Missing,
            vec![ResourceBadge {
                kind: "missing_workspace".to_owned(),
                severity: WarningSeverity::HardBlock,
                label: "Workspace missing".to_owned(),
            }],
        )
    } else if !path.is_dir() {
        (
            PlacementState::Missing,
            vec![ResourceBadge {
                kind: "workspace_not_directory".to_owned(),
                severity: WarningSeverity::HardBlock,
                label: "Workspace is not a directory".to_owned(),
            }],
        )
    } else if std::fs::metadata(path)
        .map(|metadata| metadata.permissions().readonly())
        .unwrap_or(false)
    {
        (
            PlacementState::ReadOnly,
            vec![ResourceBadge {
                kind: "read_only_workspace".to_owned(),
                severity: WarningSeverity::HardBlock,
                label: "Read-only workspace".to_owned(),
            }],
        )
    } else {
        (
            PlacementState::Validated,
            compute_resource_badges(workspace_path),
        )
    };
    if state == PlacementState::Validated
        && badges.is_empty()
        && fallback_state != PlacementState::Pending
    {
        badges.push(ResourceBadge {
            kind: "validated".to_owned(),
            severity: WarningSeverity::Info,
            label: "Validated".to_owned(),
        });
    }
    WorkspaceSnapshot {
        display_name: display_name.to_owned(),
        workspace_path: workspace_path.to_owned(),
        state,
        resource_badges: badges,
        git_snapshot: None,
        last_validated_at: Utc::now(),
    }
}

pub(crate) fn stable_placement_id(node_id: &NodeId, workspace_path: &str) -> ProjectPlacementId {
    let digest = Sha256::digest(format!("{}:{workspace_path}", node_id.as_str()).as_bytes());
    ProjectPlacementId::from(format!("placement-{}", hex_prefix(&digest, 16)))
}

pub(crate) fn stable_project_id(node_id: &NodeId, workspace_path: &str) -> ProjectId {
    let digest = Sha256::digest(format!("{}:{workspace_path}", node_id.as_str()).as_bytes());
    ProjectId::from(format!("project-{}", hex_prefix(&digest, 16)))
}

pub(crate) fn new_secret(prefix: &str) -> String {
    format!("{prefix}-{}-{}", Uuid::new_v4(), Uuid::new_v4())
}

pub(crate) fn hash_secret(secret: &str) -> String {
    let digest = Sha256::digest(secret.as_bytes());
    hex_prefix(&digest, digest.len())
}

pub(crate) fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| AppError::internal(format!("password hashing failed: {error}")))
}

pub(crate) fn verify_password(stored: &str, password: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

pub(crate) fn validate_local_password(password: &str) -> Result<(), AppError> {
    if password.chars().count() < MIN_LOCAL_PASSWORD_CHARS {
        return Err(AppError::bad_request(
            "auth.password_too_short",
            format!("Password must be at least {MIN_LOCAL_PASSWORD_CHARS} characters"),
        ));
    }
    Ok(())
}

pub(crate) fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len() && left.ct_eq(right).into()
}

pub(crate) fn hex_prefix(bytes: &[u8], len: usize) -> String {
    bytes
        .iter()
        .take(len)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

pub(crate) fn derive_presence(
    stored: NodePresence,
    heartbeat_age_seconds: Option<i64>,
    stale_after_seconds: i64,
    offline_after_seconds: i64,
) -> NodePresence {
    if stored == NodePresence::Revoked {
        return NodePresence::Revoked;
    }
    match heartbeat_age_seconds {
        Some(age) if age >= offline_after_seconds => NodePresence::Offline,
        Some(age) if age >= stale_after_seconds => NodePresence::Stale,
        Some(_) => NodePresence::Reachable,
        None => stored,
    }
}

pub(crate) fn parse_presence(value: &str) -> NodePresence {
    match value {
        "stale" => NodePresence::Stale,
        "offline" => NodePresence::Offline,
        "revoked" => NodePresence::Revoked,
        _ => NodePresence::Reachable,
    }
}

pub(crate) fn parse_enrollment_state(value: &str) -> EnrollmentState {
    match value {
        "approved" => EnrollmentState::Approved,
        "registered" => EnrollmentState::Registered,
        "expired" => EnrollmentState::Expired,
        "rejected" => EnrollmentState::Rejected,
        "revoked" => EnrollmentState::Revoked,
        "unregistered" => EnrollmentState::Unregistered,
        _ => EnrollmentState::PendingUserApproval,
    }
}

pub(crate) fn format_enrollment_state(value: &EnrollmentState) -> &'static str {
    match value {
        EnrollmentState::Unregistered => "unregistered",
        EnrollmentState::PendingUserApproval => "pending_user_approval",
        EnrollmentState::Approved => "approved",
        EnrollmentState::Registered => "registered",
        EnrollmentState::Expired => "expired",
        EnrollmentState::Rejected => "rejected",
        EnrollmentState::Revoked => "revoked",
    }
}

pub(crate) fn format_client_log_level(value: ClientLogLevel) -> &'static str {
    match value {
        ClientLogLevel::Debug => "debug",
        ClientLogLevel::Info => "info",
        ClientLogLevel::Warn => "warn",
        ClientLogLevel::Error => "error",
    }
}

pub(crate) fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    truncated.push_str("...");
    truncated
}

pub(crate) async fn append_jsonl_log(path: PathBuf, line: String) -> Result<(), AppError> {
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let line_bytes = line.as_bytes();
        let current_bytes = std::fs::metadata(&path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        if current_bytes.saturating_add(line_bytes.len() as u64 + 1) > MAX_CLIENT_LOG_BYTES {
            rotate_client_logs(&path)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        file.write_all(line_bytes)?;
        file.write_all(b"\n")?;
        file.flush()
    })
    .await??;
    Ok(())
}

pub(crate) fn rotate_client_logs(path: &std::path::Path) -> std::io::Result<()> {
    let oldest = PathBuf::from(format!("{}.{}", path.display(), MAX_CLIENT_LOG_FILES - 1));
    if oldest.exists() {
        std::fs::remove_file(oldest)?;
    }
    for index in (1..MAX_CLIENT_LOG_FILES - 1).rev() {
        let current = PathBuf::from(format!("{}.{}", path.display(), index));
        if current.exists() {
            std::fs::rename(
                current,
                PathBuf::from(format!("{}.{}", path.display(), index + 1)),
            )?;
        }
    }
    if path.exists() {
        std::fs::rename(path, PathBuf::from(format!("{}.1", path.display())))?;
    }
    Ok(())
}

pub(crate) async fn audit_security_event(
    state: &AppState,
    kind: &str,
    node_id: Option<&NodeId>,
    origin: Option<String>,
    outcome: &str,
    metadata: JsonValue,
) -> Result<(), AppError> {
    let now = Utc::now();
    let origin = origin.filter(|value| !value.is_empty());
    sqlx::query(
        r#"
        insert into security_audit_events (
            audit_event_id, kind, node_id, origin, outcome, metadata_json, happened_at
        )
        values (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(kind)
    .bind(node_id.map(NodeId::as_str))
    .bind(origin.as_deref())
    .bind(outcome)
    .bind(serde_json::to_string(&metadata.0)?)
    .bind(now)
    .execute(&state.pool)
    .await?;
    tracing::info!(
        security_event = kind,
        origin = origin.as_deref().unwrap_or("none"),
        outcome,
        "security audit event recorded"
    );
    Ok(())
}

pub(crate) fn format_sleep_hint(value: SleepHint) -> &'static str {
    match value {
        SleepHint::Unknown => "unknown",
        SleepHint::Awake => "awake",
        SleepHint::Suspending => "suspending",
        SleepHint::Sleeping => "sleeping",
        SleepHint::Woke => "woke",
    }
}

pub(crate) fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let cookie_header = headers.get(COOKIE)?.to_str().ok()?;
    cookie_header
        .split(';')
        .filter_map(|part| {
            let (cookie_name, cookie_value) = part.trim().split_once('=')?;
            (cookie_name == name && !cookie_value.is_empty()).then(|| cookie_value.to_owned())
        })
        .next()
}

pub(crate) fn origin_allowed(config: &AppConfig, origin: &str) -> bool {
    config
        .allowed_origins
        .iter()
        .filter_map(|allowed| allowed.to_str().ok())
        .any(|allowed| allowed == origin)
}

pub(crate) fn request_correlation_id(headers: &HeaderMap) -> CorrelationId {
    header_value(headers, CORRELATION_ID_HEADER)
        .or_else(|| header_value(headers, REQUEST_ID_HEADER))
        .map(|value| truncate_chars(value.trim(), 128))
        .filter(|value| !value.is_empty())
        .map(CorrelationId::from)
        .unwrap_or_default()
}

pub(crate) fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let authorization = header_value(headers, "authorization")?;
    authorization
        .strip_prefix("Bearer ")
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) fn parse_sleep_hint(value: &str) -> SleepHint {
    match value {
        "awake" => SleepHint::Awake,
        "suspending" => SleepHint::Suspending,
        "sleeping" => SleepHint::Sleeping,
        "woke" => SleepHint::Woke,
        _ => SleepHint::Unknown,
    }
}

pub(crate) fn parse_placement_state(value: &str) -> uprava_protocol::PlacementState {
    match value {
        "validated" => uprava_protocol::PlacementState::Validated,
        "missing" => uprava_protocol::PlacementState::Missing,
        "read_only" => uprava_protocol::PlacementState::ReadOnly,
        "error" => uprava_protocol::PlacementState::Error,
        _ => uprava_protocol::PlacementState::Pending,
    }
}

pub(crate) fn format_placement_state(value: PlacementState) -> &'static str {
    match value {
        PlacementState::Pending => "pending",
        PlacementState::Validated => "validated",
        PlacementState::Missing => "missing",
        PlacementState::ReadOnly => "read_only",
        PlacementState::Error => "error",
    }
}

pub(crate) fn parse_session_state(value: &str) -> SessionThreadState {
    match value {
        "active" => SessionThreadState::Active,
        "detached" => SessionThreadState::Detached,
        "stopped" => SessionThreadState::Stopped,
        "degraded" => SessionThreadState::Degraded,
        _ => SessionThreadState::Created,
    }
}

pub(crate) fn format_session_state(value: SessionThreadState) -> &'static str {
    match value {
        SessionThreadState::Created => "created",
        SessionThreadState::Active => "active",
        SessionThreadState::Detached => "detached",
        SessionThreadState::Stopped => "stopped",
        SessionThreadState::Degraded => "degraded",
    }
}

pub(crate) fn parse_deduction_state(value: &str) -> DeductionState {
    match value {
        "running" => DeductionState::Running,
        "completed" => DeductionState::Completed,
        "invalid" => DeductionState::Invalid,
        "failed" => DeductionState::Failed,
        "cancelled" => DeductionState::Cancelled,
        _ => DeductionState::Requested,
    }
}

pub(crate) fn parse_runtime_state(value: &str) -> RuntimeSessionState {
    match value {
        "ready" => RuntimeSessionState::Ready,
        "running" => RuntimeSessionState::Running,
        "blocked" => RuntimeSessionState::Blocked,
        "stopping" => RuntimeSessionState::Stopping,
        "stopped" => RuntimeSessionState::Stopped,
        "interrupted" => RuntimeSessionState::Interrupted,
        "resuming" => RuntimeSessionState::Resuming,
        "stale" => RuntimeSessionState::Stale,
        "error" => RuntimeSessionState::Error,
        "expired" => RuntimeSessionState::Expired,
        _ => RuntimeSessionState::Starting,
    }
}

pub(crate) fn format_runtime_state(value: RuntimeSessionState) -> &'static str {
    match value {
        RuntimeSessionState::Starting => "starting",
        RuntimeSessionState::Ready => "ready",
        RuntimeSessionState::Running => "running",
        RuntimeSessionState::Blocked => "blocked",
        RuntimeSessionState::Stopping => "stopping",
        RuntimeSessionState::Stopped => "stopped",
        RuntimeSessionState::Interrupted => "interrupted",
        RuntimeSessionState::Resuming => "resuming",
        RuntimeSessionState::Stale => "stale",
        RuntimeSessionState::Error => "error",
        RuntimeSessionState::Expired => "expired",
    }
}

pub(crate) fn parse_execution_profile(value: &str) -> AgentExecutionProfile {
    match value {
        "managed" => AgentExecutionProfile::Managed,
        _ => AgentExecutionProfile::ExecCompatibility,
    }
}

pub(crate) const fn format_execution_profile(value: AgentExecutionProfile) -> &'static str {
    match value {
        AgentExecutionProfile::Managed => "managed",
        AgentExecutionProfile::ExecCompatibility => "exec_compatibility",
    }
}

pub(crate) fn parse_runtime_attempt_state(value: &str) -> RuntimeAttemptState {
    match value {
        "starting" => RuntimeAttemptState::Starting,
        "ready" => RuntimeAttemptState::Ready,
        "disconnected" => RuntimeAttemptState::Disconnected,
        "reconnecting" => RuntimeAttemptState::Reconnecting,
        "recovered" => RuntimeAttemptState::Recovered,
        "stopping" => RuntimeAttemptState::Stopping,
        "stopped" => RuntimeAttemptState::Stopped,
        "lost" => RuntimeAttemptState::Lost,
        _ => RuntimeAttemptState::Failed,
    }
}

pub(crate) const fn format_runtime_attempt_state(value: RuntimeAttemptState) -> &'static str {
    match value {
        RuntimeAttemptState::Starting => "starting",
        RuntimeAttemptState::Ready => "ready",
        RuntimeAttemptState::Disconnected => "disconnected",
        RuntimeAttemptState::Reconnecting => "reconnecting",
        RuntimeAttemptState::Recovered => "recovered",
        RuntimeAttemptState::Stopping => "stopping",
        RuntimeAttemptState::Stopped => "stopped",
        RuntimeAttemptState::Failed => "failed",
        RuntimeAttemptState::Lost => "lost",
    }
}

pub(crate) const fn format_provider_interaction_kind(
    value: ProviderInteractionKind,
) -> &'static str {
    match value {
        ProviderInteractionKind::Approval => "approval",
        ProviderInteractionKind::UserInput => "user_input",
    }
}

pub(crate) fn parse_runtime_recovery_status(value: &str) -> RuntimeRecoveryStatus {
    match value {
        "live" => RuntimeRecoveryStatus::Live,
        "reconnecting" => RuntimeRecoveryStatus::Reconnecting,
        "recovered" => RuntimeRecoveryStatus::Recovered,
        "provider_resumable" => RuntimeRecoveryStatus::ProviderResumable,
        "degraded" => RuntimeRecoveryStatus::Degraded,
        "lost" => RuntimeRecoveryStatus::Lost,
        "failed" => RuntimeRecoveryStatus::Failed,
        _ => RuntimeRecoveryStatus::NotRequired,
    }
}

pub(crate) fn format_turn_state(value: TurnState) -> &'static str {
    match value {
        TurnState::Created => "created",
        TurnState::Dispatched => "dispatched",
        TurnState::Running => "running",
        TurnState::BlockedOnApproval => "blocked_on_approval",
        TurnState::Completed => "completed",
        TurnState::Interrupted => "interrupted",
        TurnState::Failed => "failed",
    }
}

pub(crate) fn parse_scheduled_message_state(value: &str) -> ScheduledMessageState {
    match value {
        "sending" => ScheduledMessageState::Sending,
        "sent" => ScheduledMessageState::Sent,
        "failed" => ScheduledMessageState::Failed,
        "cancelled" => ScheduledMessageState::Cancelled,
        _ => ScheduledMessageState::Scheduled,
    }
}

pub(crate) fn format_approval_state(value: ApprovalState) -> &'static str {
    match value {
        ApprovalState::Requested => "requested",
        ApprovalState::Resolved => "resolved",
        ApprovalState::Expired => "expired",
        ApprovalState::Cancelled => "cancelled",
    }
}

pub(crate) fn parse_command_kind(value: &str) -> CommandKind {
    match value {
        "StartRuntime" => CommandKind::StartRuntime,
        "ResumeRuntime" => CommandKind::ResumeRuntime,
        "SendTurn" => CommandKind::SendTurn,
        "ResolveApproval" => CommandKind::ResolveApproval,
        "SubmitUserInput" => CommandKind::SubmitUserInput,
        "InterruptRuntime" => CommandKind::InterruptRuntime,
        "StopRuntime" => CommandKind::StopRuntime,
        "ValidateWorkspace" => CommandKind::ValidateWorkspace,
        "RefreshResourceSnapshot" => CommandKind::RefreshResourceSnapshot,
        "ListWorkspaceTree" => CommandKind::ListWorkspaceTree,
        "ReadWorkspaceFile" => CommandKind::ReadWorkspaceFile,
        "WriteWorkspaceFile" => CommandKind::WriteWorkspaceFile,
        "RunWorkspaceCommand" => CommandKind::RunWorkspaceCommand,
        "ReadWorkspaceDiff" => CommandKind::ReadWorkspaceDiff,
        "OpenWorkspaceTerminal" => CommandKind::OpenWorkspaceTerminal,
        "AttachWorkspaceTerminal" => CommandKind::AttachWorkspaceTerminal,
        "ResizeWorkspaceTerminal" => CommandKind::ResizeWorkspaceTerminal,
        "WriteWorkspaceTerminal" => CommandKind::WriteWorkspaceTerminal,
        "CloseWorkspaceTerminal" => CommandKind::CloseWorkspaceTerminal,
        _ => CommandKind::RefreshResourceSnapshot,
    }
}

pub(crate) fn parse_command_state(value: &str) -> CommandState {
    match value {
        "recorded" => CommandState::Recorded,
        "pending_dispatch" => CommandState::PendingDispatch,
        "dispatched" => CommandState::Dispatched,
        "acknowledged" => CommandState::Acknowledged,
        "completed" => CommandState::Completed,
        "failed" => CommandState::Failed,
        "blocked" => CommandState::Blocked,
        "expired" => CommandState::Expired,
        _ => CommandState::Recorded,
    }
}

pub(crate) fn format_command_state(value: CommandState) -> &'static str {
    match value {
        CommandState::Recorded => "recorded",
        CommandState::PendingDispatch => "pending_dispatch",
        CommandState::Dispatched => "dispatched",
        CommandState::Acknowledged => "acknowledged",
        CommandState::Completed => "completed",
        CommandState::Failed => "failed",
        CommandState::Blocked => "blocked",
        CommandState::Expired => "expired",
    }
}

pub(crate) fn parse_message_role(value: &str) -> MessageRole {
    match value {
        "assistant" => MessageRole::Assistant,
        "system" => MessageRole::System,
        "runtime" => MessageRole::Runtime,
        "approval" => MessageRole::Approval,
        _ => MessageRole::User,
    }
}

pub(crate) fn format_message_role(value: MessageRole) -> &'static str {
    match value {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::System => "system",
        MessageRole::Runtime => "runtime",
        MessageRole::Approval => "approval",
    }
}

pub(crate) fn incompatible_state_error(message: impl Into<String>) -> AppError {
    AppError::Database(sqlx::Error::Protocol(format!(
        "incompatible Core state: {}",
        message.into()
    )))
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("background task error: {0}")]
    TaskJoin(#[from] tokio::task::JoinError),
    #[error("internal error: {message}")]
    Internal { code: &'static str, message: String },
    #[error("not found: {message}")]
    NotFound { code: &'static str, message: String },
    #[error("bad request: {message}")]
    BadRequest { code: &'static str, message: String },
    #[error("conflict: {message}")]
    Conflict { code: &'static str, message: String },
    #[error("auth error: {message}")]
    Auth { code: &'static str, message: String },
    #[error("rate limited: {message}")]
    RateLimited { code: &'static str, message: String },
}

impl AppError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Database(_) => "internal.database",
            Self::Serialization(_) => "internal.serialization",
            Self::Io(_) => "internal.io",
            Self::TaskJoin(_) => "internal.task_join",
            Self::Internal { code, .. }
            | Self::NotFound { code, .. }
            | Self::BadRequest { code, .. }
            | Self::Conflict { code, .. }
            | Self::Auth { code, .. }
            | Self::RateLimited { code, .. } => code,
        }
    }

    pub(crate) fn not_found(code: &'static str, message: impl Into<String>) -> Self {
        Self::NotFound {
            code,
            message: message.into(),
        }
    }

    pub(crate) fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::BadRequest {
            code,
            message: message.into(),
        }
    }

    pub(crate) fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::Conflict {
            code,
            message: message.into(),
        }
    }

    pub(crate) fn auth(code: &'static str, message: impl Into<String>) -> Self {
        Self::Auth {
            code,
            message: message.into(),
        }
    }

    pub(crate) fn rate_limited(code: &'static str, message: impl Into<String>) -> Self {
        Self::RateLimited {
            code,
            message: message.into(),
        }
    }

    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            code: "internal.error",
            message: message.into(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let correlation_id = REQUEST_CORRELATION_ID
            .try_with(Clone::clone)
            .unwrap_or_default();
        let (status, error_code, message, retryable) = match self {
            Self::Database(error) => {
                tracing::error!(%correlation_id, error = %error, "database error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal.database",
                    "Core database operation failed".to_owned(),
                    true,
                )
            }
            Self::Serialization(error) => {
                tracing::error!(%correlation_id, error = %error, "serialization error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal.serialization",
                    "Core serialization failed".to_owned(),
                    false,
                )
            }
            Self::Io(error) => {
                tracing::error!(%correlation_id, error = %error, "io error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal.io",
                    "Core IO operation failed".to_owned(),
                    true,
                )
            }
            Self::TaskJoin(error) => {
                tracing::error!(%correlation_id, error = %error, "background task error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal.task_join",
                    "Core background task failed".to_owned(),
                    true,
                )
            }
            Self::Internal { code, message } => {
                tracing::error!(%correlation_id, %message, "internal Core error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    code,
                    "Core encountered an internal error".to_owned(),
                    true,
                )
            }
            Self::NotFound { code, message } => (StatusCode::NOT_FOUND, code, message, false),
            Self::BadRequest { code, message } => (StatusCode::BAD_REQUEST, code, message, false),
            Self::Conflict { code, message } => (StatusCode::CONFLICT, code, message, false),
            Self::Auth { code, message } => (StatusCode::UNAUTHORIZED, code, message, false),
            Self::RateLimited { code, message } => {
                (StatusCode::TOO_MANY_REQUESTS, code, message, true)
            }
        };
        let body = ApiError {
            error_code: error_code.to_owned(),
            message,
            details: JsonValue::default(),
            retryable,
            correlation_id,
        };
        (status, Json(body)).into_response()
    }
}
