use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandEnvelope {
    pub command_id: CommandId,
    pub kind: CommandKind,
    pub target: CommandTarget,
    pub actor_ref: ActorRef,
    #[serde(default)]
    pub source_refs: Vec<UpravaRef>,
    #[serde(default)]
    pub cause_refs: Vec<UpravaRef>,
    pub issued_at: DateTime<Utc>,
    pub correlation_id: CorrelationId,
    pub payload: CommandPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommandPayload {
    StartRuntime {
        provider: String,
        workspace_path: String,
        #[serde(default)]
        execution_profile: AgentExecutionProfile,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effective_policy: Option<EffectiveRuntimePolicy>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effective_policy_hash: Option<RuntimePolicyHash>,
    },
    ResumeRuntime {
        provider: String,
        workspace_path: String,
        provider_resume_ref: Option<serde_json_value::JsonValue>,
        #[serde(default)]
        execution_profile: AgentExecutionProfile,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effective_policy: Option<EffectiveRuntimePolicy>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effective_policy_hash: Option<RuntimePolicyHash>,
    },
    SendTurn {
        content: String,
        turn_id: TurnId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        collaboration_mode: Option<String>,
    },
    ResolveApproval {
        approval_id: ApprovalId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_interaction_id: Option<ProviderInteractionId>,
        approved: bool,
        message: Option<String>,
    },
    SubmitUserInput {
        provider_interaction_id: ProviderInteractionId,
        answers: Vec<String>,
    },
    InterruptRuntime {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        runtime_attempt_id: Option<RuntimeAttemptId>,
    },
    StopRuntime {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        runtime_attempt_id: Option<RuntimeAttemptId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    ValidateWorkspace {
        display_name: String,
        workspace_path: String,
    },
    RefreshResourceSnapshot {
        display_name: String,
        workspace_path: String,
    },
    ListWorkspaceTree {
        workspace_path: String,
        path: String,
    },
    ReadWorkspaceFile {
        workspace_path: String,
        path: String,
    },
    WriteWorkspaceFile {
        workspace_path: String,
        request: WorkspaceFileWriteRequest,
    },
    RunWorkspaceCommand {
        workspace_path: String,
        request: WorkspaceCommandRunRequest,
    },
    ReadWorkspaceDiff {
        workspace_path: String,
        #[serde(default)]
        request: WorkspaceDiffRequest,
    },
    OpenWorkspaceTerminal {
        workspace_path: String,
        request: WorkspaceTerminalOpenRequest,
    },
    AttachWorkspaceTerminal {
        request: WorkspaceTerminalAttachRequest,
    },
    ResizeWorkspaceTerminal {
        request: WorkspaceTerminalResizeRequest,
    },
    WriteWorkspaceTerminal {
        request: WorkspaceTerminalWriteRequest,
    },
    CloseWorkspaceTerminal {
        request: WorkspaceTerminalCloseRequest,
    },
    RequestDeduction {
        package: Box<DeductionInputPackage>,
    },
    CancelDeduction {
        deduction_id: DeductionId,
    },
    RunTask {
        workspace_path: String,
        spec: Box<TaskRunSpec>,
    },
    CancelTaskRun {
        task_run_id: TaskRunId,
    },
    Tooling {
        command: Box<ToolingCommandV1>,
    },
    Extension {
        name: String,
        value: serde_json_value::JsonValue,
    },
}

impl CommandPayload {
    #[must_use]
    pub fn matches_kind(&self, kind: CommandKind) -> bool {
        matches!(
            (kind, self),
            (CommandKind::StartRuntime, Self::StartRuntime { .. })
                | (CommandKind::ResumeRuntime, Self::ResumeRuntime { .. })
                | (CommandKind::SendTurn, Self::SendTurn { .. })
                | (CommandKind::ResolveApproval, Self::ResolveApproval { .. })
                | (CommandKind::SubmitUserInput, Self::SubmitUserInput { .. })
                | (CommandKind::InterruptRuntime, Self::InterruptRuntime { .. })
                | (CommandKind::StopRuntime, Self::StopRuntime { .. })
                | (
                    CommandKind::ValidateWorkspace,
                    Self::ValidateWorkspace { .. }
                )
                | (
                    CommandKind::RefreshResourceSnapshot,
                    Self::RefreshResourceSnapshot { .. }
                )
                | (
                    CommandKind::ListWorkspaceTree,
                    Self::ListWorkspaceTree { .. }
                )
                | (
                    CommandKind::ReadWorkspaceFile,
                    Self::ReadWorkspaceFile { .. }
                )
                | (
                    CommandKind::WriteWorkspaceFile,
                    Self::WriteWorkspaceFile { .. }
                )
                | (
                    CommandKind::RunWorkspaceCommand,
                    Self::RunWorkspaceCommand { .. }
                )
                | (
                    CommandKind::ReadWorkspaceDiff,
                    Self::ReadWorkspaceDiff { .. }
                )
                | (
                    CommandKind::OpenWorkspaceTerminal,
                    Self::OpenWorkspaceTerminal { .. }
                )
                | (
                    CommandKind::AttachWorkspaceTerminal,
                    Self::AttachWorkspaceTerminal { .. }
                )
                | (
                    CommandKind::ResizeWorkspaceTerminal,
                    Self::ResizeWorkspaceTerminal { .. }
                )
                | (
                    CommandKind::WriteWorkspaceTerminal,
                    Self::WriteWorkspaceTerminal { .. }
                )
                | (
                    CommandKind::CloseWorkspaceTerminal,
                    Self::CloseWorkspaceTerminal { .. }
                )
                | (CommandKind::RequestDeduction, Self::RequestDeduction { .. })
                | (CommandKind::CancelDeduction, Self::CancelDeduction { .. })
                | (CommandKind::RunTask, Self::RunTask { .. })
                | (CommandKind::CancelTaskRun, Self::CancelTaskRun { .. })
                | (CommandKind::Tooling, Self::Tooling { .. })
                | (CommandKind::Extension, Self::Extension { .. })
        )
    }

    #[must_use]
    pub fn provider(&self) -> Option<&str> {
        match self {
            Self::StartRuntime { provider, .. } | Self::ResumeRuntime { provider, .. } => {
                Some(provider)
            }
            _ => None,
        }
    }

    #[must_use]
    pub fn workspace_path(&self) -> Option<&str> {
        match self {
            Self::StartRuntime { workspace_path, .. }
            | Self::ResumeRuntime { workspace_path, .. }
            | Self::ValidateWorkspace { workspace_path, .. }
            | Self::RefreshResourceSnapshot { workspace_path, .. }
            | Self::ListWorkspaceTree { workspace_path, .. }
            | Self::ReadWorkspaceFile { workspace_path, .. }
            | Self::WriteWorkspaceFile { workspace_path, .. }
            | Self::RunWorkspaceCommand { workspace_path, .. }
            | Self::ReadWorkspaceDiff { workspace_path, .. }
            | Self::OpenWorkspaceTerminal { workspace_path, .. } => Some(workspace_path),
            Self::RunTask { workspace_path, .. } => Some(workspace_path),
            _ => None,
        }
    }

    #[must_use]
    pub fn display_name(&self) -> Option<&str> {
        match self {
            Self::ValidateWorkspace { display_name, .. }
            | Self::RefreshResourceSnapshot { display_name, .. } => Some(display_name),
            _ => None,
        }
    }

    #[must_use]
    pub fn path(&self) -> Option<&str> {
        match self {
            Self::ListWorkspaceTree { path, .. } | Self::ReadWorkspaceFile { path, .. } => {
                Some(path)
            }
            Self::WriteWorkspaceFile { request, .. } => Some(&request.path),
            _ => None,
        }
    }

    #[must_use]
    pub fn provider_resume_ref(&self) -> Option<&serde_json_value::JsonValue> {
        match self {
            Self::ResumeRuntime {
                provider_resume_ref,
                ..
            } => provider_resume_ref.as_ref(),
            _ => None,
        }
    }

    pub fn workspace_request<T: serde::de::DeserializeOwned>(&self) -> Option<T> {
        let value = match self {
            Self::ListWorkspaceTree { path, .. } | Self::ReadWorkspaceFile { path, .. } => {
                serde_json::json!({ "path": path })
            }
            Self::WriteWorkspaceFile { request, .. } => serde_json::to_value(request).ok()?,
            Self::RunWorkspaceCommand { request, .. } => serde_json::to_value(request).ok()?,
            Self::ReadWorkspaceDiff { request, .. } => serde_json::to_value(request).ok()?,
            Self::OpenWorkspaceTerminal { request, .. } => serde_json::to_value(request).ok()?,
            _ => return None,
        };
        serde_json::from_value(value).ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandTarget {
    Node {
        node_id: NodeId,
    },
    Placement {
        node_id: NodeId,
        project_placement_id: ProjectPlacementId,
    },
    SessionRuntime {
        node_id: NodeId,
        project_placement_id: ProjectPlacementId,
        session_thread_id: SessionThreadId,
        runtime_session_id: RuntimeSessionId,
    },
    TaskRun {
        node_id: NodeId,
        project_placement_id: ProjectPlacementId,
        task_run_id: TaskRunId,
    },
}

impl CommandTarget {
    #[must_use]
    pub fn node_id(&self) -> &NodeId {
        match self {
            Self::Node { node_id }
            | Self::Placement { node_id, .. }
            | Self::SessionRuntime { node_id, .. }
            | Self::TaskRun { node_id, .. } => node_id,
        }
    }

    #[must_use]
    pub fn project_placement_id(&self) -> Option<&ProjectPlacementId> {
        match self {
            Self::Node { .. } => None,
            Self::Placement {
                project_placement_id,
                ..
            }
            | Self::SessionRuntime {
                project_placement_id,
                ..
            }
            | Self::TaskRun {
                project_placement_id,
                ..
            } => Some(project_placement_id),
        }
    }

    #[must_use]
    pub fn session_thread_id(&self) -> Option<&SessionThreadId> {
        match self {
            Self::SessionRuntime {
                session_thread_id, ..
            } => Some(session_thread_id),
            Self::Node { .. } | Self::Placement { .. } | Self::TaskRun { .. } => None,
        }
    }

    #[must_use]
    pub fn runtime_session_id(&self) -> Option<&RuntimeSessionId> {
        match self {
            Self::SessionRuntime {
                runtime_session_id, ..
            } => Some(runtime_session_id),
            Self::Node { .. } | Self::Placement { .. } | Self::TaskRun { .. } => None,
        }
    }

    #[must_use]
    pub fn task_run_id(&self) -> Option<&TaskRunId> {
        match self {
            Self::TaskRun { task_run_id, .. } => Some(task_run_id),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub event_id: EventId,
    pub command_id: Option<CommandId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<CorrelationId>,
    pub actor_ref: ActorRef,
    pub scope_ref: ScopeRef,
    pub node_id: Option<NodeId>,
    pub runtime_session_id: Option<RuntimeSessionId>,
    pub session_thread_id: Option<SessionThreadId>,
    pub turn_id: Option<TurnId>,
    pub seq: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_projection_seq: Option<i64>,
    pub kind: EventKind,
    pub happened_at: DateTime<Utc>,
    #[serde(default)]
    pub source_refs: Vec<UpravaRef>,
    #[serde(default)]
    pub evidence_refs: Vec<UpravaRef>,
    #[serde(default)]
    pub cause_refs: Vec<UpravaRef>,
    #[serde(default)]
    pub result_refs: Vec<UpravaRef>,
    pub payload: EventPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RuntimeStateEventData {
    pub provider: Option<String>,
    pub mode: Option<String>,
    pub resume_source: Option<String>,
    pub provider_resume_ref: Option<serde_json_value::JsonValue>,
    pub transcript_messages: Option<u64>,
    pub reason: Option<String>,
    pub code: Option<String>,
    pub message: Option<String>,
    pub expiry_seconds: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProviderActivityEventData {
    pub provider: Option<String>,
    pub source: Option<String>,
    pub provider_event_type: Option<String>,
    pub provider_item_id: Option<String>,
    pub provider_item_type: Option<String>,
    pub phase: Option<String>,
    pub status: Option<String>,
    pub summary: Option<String>,
    pub raw_event: Option<serde_json_value::JsonValue>,
    pub raw_event_truncated: Option<bool>,
    pub raw_event_original_chars: Option<u64>,
    pub raw_event_preview: Option<String>,
    pub dropped_count: Option<u64>,
    pub stream: Option<String>,
    pub limit_bytes: Option<u64>,
    pub stdout_truncated: Option<bool>,
    pub stderr_truncated: Option<bool>,
    pub dropped_activity_count: Option<u64>,
    pub max_process_output_bytes: Option<u64>,
    pub max_activity_events: Option<u64>,
    pub extension: Option<serde_json_value::JsonValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventPayloadKind {
    RuntimeAttemptStarted {
        runtime_attempt_id: RuntimeAttemptId,
        state: RuntimeAttemptState,
        reason: Option<String>,
        code: Option<String>,
        message: Option<String>,
    },
    RuntimeAttemptReady {
        runtime_attempt_id: RuntimeAttemptId,
        state: RuntimeAttemptState,
        reason: Option<String>,
        code: Option<String>,
        message: Option<String>,
    },
    RuntimeAttemptDisconnected {
        runtime_attempt_id: RuntimeAttemptId,
        state: RuntimeAttemptState,
        reason: Option<String>,
        code: Option<String>,
        message: Option<String>,
    },
    RuntimeAttemptReconnecting {
        runtime_attempt_id: RuntimeAttemptId,
        state: RuntimeAttemptState,
        reason: Option<String>,
        code: Option<String>,
        message: Option<String>,
    },
    RuntimeAttemptRecovered {
        runtime_attempt_id: RuntimeAttemptId,
        state: RuntimeAttemptState,
        reason: Option<String>,
        code: Option<String>,
        message: Option<String>,
    },
    RuntimeAttemptFailed {
        runtime_attempt_id: RuntimeAttemptId,
        state: RuntimeAttemptState,
        reason: Option<String>,
        code: Option<String>,
        message: Option<String>,
    },
    RuntimeStarting {
        #[serde(flatten)]
        data: RuntimeStateEventData,
    },
    RuntimeReady {
        #[serde(flatten)]
        data: RuntimeStateEventData,
    },
    RuntimeRunning {
        #[serde(flatten)]
        data: RuntimeStateEventData,
    },
    RuntimeBlocked {
        #[serde(flatten)]
        data: RuntimeStateEventData,
    },
    RuntimeExpired {
        #[serde(flatten)]
        data: RuntimeStateEventData,
    },
    RuntimeResuming {
        #[serde(flatten)]
        data: RuntimeStateEventData,
    },
    RuntimeStopped {
        #[serde(flatten)]
        data: RuntimeStateEventData,
    },
    RuntimeError {
        code: String,
        message: String,
    },
    TurnStarted,
    TurnCompleted,
    TurnInterrupted {
        provider: Option<String>,
        code: Option<String>,
        message: Option<String>,
    },
    ProviderActivity {
        #[serde(flatten)]
        data: ProviderActivityEventData,
    },
    ProviderInteractionRequested {
        provider_interaction_id: ProviderInteractionId,
        runtime_attempt_id: RuntimeAttemptId,
        interaction_kind: ProviderInteractionKind,
        prompt: String,
        expires_at: Option<DateTime<Utc>>,
    },
    ProviderInteractionResolved {
        provider_interaction_id: ProviderInteractionId,
        runtime_attempt_id: RuntimeAttemptId,
        interaction_kind: ProviderInteractionKind,
        approved: Option<bool>,
        answers: Vec<String>,
    },
    ProviderOutputDelta {
        content: String,
    },
    ProviderMessageCompleted {
        content: String,
    },
    ApprovalRequested {
        approval_id: ApprovalId,
        prompt: String,
        provider: Option<String>,
        provider_event_type: Option<String>,
        source: Option<String>,
    },
    ApprovalResolved {
        approval_id: ApprovalId,
        approved: bool,
        message: String,
    },
    RuntimePolicyEffective {
        policy: Option<EffectiveRuntimePolicy>,
        policy_hash: Option<RuntimePolicyHash>,
    },
    RuntimePolicyOverride {
        policy_hash: Option<RuntimePolicyHash>,
        policy_override: Option<RuntimePolicyOverride>,
    },
    CoordinationWarningAcknowledged {
        warning_kind: String,
        message: Option<String>,
        affected_refs: Vec<UpravaRef>,
    },
    WorkspaceValidated {
        placement_id: ProjectPlacementId,
        display_name: String,
        workspace_path: String,
        state: PlacementState,
        resource_badges: Vec<ResourceBadge>,
        git_snapshot: Option<GitWorkspaceSnapshot>,
    },
    ResourceSnapshotUpdated {
        placement_id: ProjectPlacementId,
        display_name: String,
        workspace_path: String,
        state: PlacementState,
        resource_badges: Vec<ResourceBadge>,
        git_snapshot: Option<GitWorkspaceSnapshot>,
    },
    WorkspaceFileWritten {
        placement_id: ProjectPlacementId,
        path: String,
        edit_id: String,
    },
    WorkspaceCommandCompleted {
        placement_id: ProjectPlacementId,
        terminal_command_id: String,
        success: bool,
        exit_code: Option<i32>,
        stdout_truncated: bool,
        stderr_truncated: bool,
    },
    WorkspaceCheckCompleted {
        placement_id: ProjectPlacementId,
        check_run_id: String,
        success: bool,
        exit_code: Option<i32>,
        stdout_truncated: bool,
        stderr_truncated: bool,
    },
    WorkspaceDiffObserved {
        placement_id: ProjectPlacementId,
        diff_id: String,
        summary_truncated: bool,
        diff_truncated: bool,
    },
    DeductionRequested {
        deduction_id: DeductionId,
        scope_ref: UpravaRef,
        question: String,
    },
    DeductionCompleted {
        deduction_id: DeductionId,
    },
    DeductionInvalid {
        deduction_id: DeductionId,
        code: String,
        message: String,
    },
    DeductionFailed {
        deduction_id: DeductionId,
        code: String,
        message: String,
    },
    DeductionCancelled {
        deduction_id: DeductionId,
    },
    TaskRunStateChanged {
        task_run_id: TaskRunId,
        state: TaskRunState,
        cleanup_state: TaskCleanupState,
        message: Option<String>,
    },
    Extension {
        name: String,
        value: serde_json_value::JsonValue,
    },
}

#[derive(Debug, Clone)]
pub struct EventPayload {
    kind: EventPayloadKind,
    json: serde_json_value::JsonValue,
}

impl EventPayload {
    #[must_use]
    pub fn kind(&self) -> &EventPayloadKind {
        &self.kind
    }

    #[must_use]
    pub fn matches_kind(&self, kind: EventKind) -> bool {
        matches!(
            (kind, &self.kind),
            (
                EventKind::RuntimeAttemptStarted,
                EventPayloadKind::RuntimeAttemptStarted { .. }
            ) | (
                EventKind::RuntimeAttemptReady,
                EventPayloadKind::RuntimeAttemptReady { .. }
            ) | (
                EventKind::RuntimeAttemptDisconnected,
                EventPayloadKind::RuntimeAttemptDisconnected { .. }
            ) | (
                EventKind::RuntimeAttemptReconnecting,
                EventPayloadKind::RuntimeAttemptReconnecting { .. }
            ) | (
                EventKind::RuntimeAttemptRecovered,
                EventPayloadKind::RuntimeAttemptRecovered { .. }
            ) | (
                EventKind::RuntimeAttemptFailed,
                EventPayloadKind::RuntimeAttemptFailed { .. }
            ) | (
                EventKind::RuntimeStarting,
                EventPayloadKind::RuntimeStarting { .. }
            ) | (
                EventKind::RuntimeReady,
                EventPayloadKind::RuntimeReady { .. }
            ) | (
                EventKind::RuntimeRunning,
                EventPayloadKind::RuntimeRunning { .. }
            ) | (
                EventKind::RuntimeBlocked,
                EventPayloadKind::RuntimeBlocked { .. }
            ) | (
                EventKind::RuntimeExpired,
                EventPayloadKind::RuntimeExpired { .. }
            ) | (
                EventKind::RuntimeResuming,
                EventPayloadKind::RuntimeResuming { .. }
            ) | (
                EventKind::RuntimeStopped,
                EventPayloadKind::RuntimeStopped { .. }
            ) | (
                EventKind::RuntimeError,
                EventPayloadKind::RuntimeError { .. }
            ) | (EventKind::TurnStarted, EventPayloadKind::TurnStarted)
                | (EventKind::TurnCompleted, EventPayloadKind::TurnCompleted)
                | (
                    EventKind::TurnInterrupted,
                    EventPayloadKind::TurnInterrupted { .. }
                )
                | (
                    EventKind::ProviderActivity,
                    EventPayloadKind::ProviderActivity { .. }
                )
                | (
                    EventKind::ProviderInteractionRequested,
                    EventPayloadKind::ProviderInteractionRequested { .. }
                )
                | (
                    EventKind::ProviderInteractionResolved,
                    EventPayloadKind::ProviderInteractionResolved { .. }
                )
                | (
                    EventKind::ProviderOutputDelta,
                    EventPayloadKind::ProviderOutputDelta { .. }
                )
                | (
                    EventKind::ProviderMessageCompleted,
                    EventPayloadKind::ProviderMessageCompleted { .. }
                )
                | (
                    EventKind::ApprovalRequested,
                    EventPayloadKind::ApprovalRequested { .. }
                )
                | (
                    EventKind::ApprovalResolved,
                    EventPayloadKind::ApprovalResolved { .. }
                )
                | (
                    EventKind::RuntimePolicyEffective,
                    EventPayloadKind::RuntimePolicyEffective { .. }
                )
                | (
                    EventKind::RuntimePolicyOverride,
                    EventPayloadKind::RuntimePolicyOverride { .. }
                )
                | (
                    EventKind::CoordinationWarningAcknowledged,
                    EventPayloadKind::CoordinationWarningAcknowledged { .. }
                )
                | (
                    EventKind::WorkspaceValidated,
                    EventPayloadKind::WorkspaceValidated { .. }
                )
                | (
                    EventKind::ResourceSnapshotUpdated,
                    EventPayloadKind::ResourceSnapshotUpdated { .. }
                )
                | (
                    EventKind::WorkspaceFileWritten,
                    EventPayloadKind::WorkspaceFileWritten { .. }
                )
                | (
                    EventKind::WorkspaceCommandCompleted,
                    EventPayloadKind::WorkspaceCommandCompleted { .. }
                )
                | (
                    EventKind::WorkspaceCheckCompleted,
                    EventPayloadKind::WorkspaceCheckCompleted { .. }
                )
                | (
                    EventKind::WorkspaceDiffObserved,
                    EventPayloadKind::WorkspaceDiffObserved { .. }
                )
                | (
                    EventKind::DeductionRequested,
                    EventPayloadKind::DeductionRequested { .. }
                )
                | (
                    EventKind::DeductionCompleted,
                    EventPayloadKind::DeductionCompleted { .. }
                )
                | (
                    EventKind::DeductionInvalid,
                    EventPayloadKind::DeductionInvalid { .. }
                )
                | (
                    EventKind::DeductionFailed,
                    EventPayloadKind::DeductionFailed { .. }
                )
                | (
                    EventKind::DeductionCancelled,
                    EventPayloadKind::DeductionCancelled { .. }
                )
                | (
                    EventKind::TaskRunStateChanged,
                    EventPayloadKind::TaskRunStateChanged { .. }
                )
                | (EventKind::Extension, EventPayloadKind::Extension { .. })
        )
    }

    pub fn from_json(kind: EventKind, value: serde_json::Value) -> Self {
        let runtime = || serde_json::from_value(value.clone()).unwrap_or_default();
        let text = |key: &str| {
            value
                .get(key)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let attempt_fields = || {
            (
                RuntimeAttemptId::from(text("runtime_attempt_id")),
                value
                    .get("state")
                    .cloned()
                    .and_then(|value| serde_json::from_value(value).ok())
                    .unwrap_or(RuntimeAttemptState::Failed),
                optional_text(&value, "reason"),
                optional_text(&value, "code"),
                optional_text(&value, "message"),
            )
        };
        let payload = match kind {
            EventKind::RuntimeAttemptStarted => {
                let (runtime_attempt_id, state, reason, code, message) = attempt_fields();
                EventPayloadKind::RuntimeAttemptStarted {
                    runtime_attempt_id,
                    state,
                    reason,
                    code,
                    message,
                }
            }
            EventKind::RuntimeAttemptReady => {
                let (runtime_attempt_id, state, reason, code, message) = attempt_fields();
                EventPayloadKind::RuntimeAttemptReady {
                    runtime_attempt_id,
                    state,
                    reason,
                    code,
                    message,
                }
            }
            EventKind::RuntimeAttemptDisconnected => {
                let (runtime_attempt_id, state, reason, code, message) = attempt_fields();
                EventPayloadKind::RuntimeAttemptDisconnected {
                    runtime_attempt_id,
                    state,
                    reason,
                    code,
                    message,
                }
            }
            EventKind::RuntimeAttemptReconnecting => {
                let (runtime_attempt_id, state, reason, code, message) = attempt_fields();
                EventPayloadKind::RuntimeAttemptReconnecting {
                    runtime_attempt_id,
                    state,
                    reason,
                    code,
                    message,
                }
            }
            EventKind::RuntimeAttemptRecovered => {
                let (runtime_attempt_id, state, reason, code, message) = attempt_fields();
                EventPayloadKind::RuntimeAttemptRecovered {
                    runtime_attempt_id,
                    state,
                    reason,
                    code,
                    message,
                }
            }
            EventKind::RuntimeAttemptFailed => {
                let (runtime_attempt_id, state, reason, code, message) = attempt_fields();
                EventPayloadKind::RuntimeAttemptFailed {
                    runtime_attempt_id,
                    state,
                    reason,
                    code,
                    message,
                }
            }
            EventKind::RuntimeStarting => EventPayloadKind::RuntimeStarting { data: runtime() },
            EventKind::RuntimeReady => EventPayloadKind::RuntimeReady { data: runtime() },
            EventKind::RuntimeRunning => EventPayloadKind::RuntimeRunning { data: runtime() },
            EventKind::RuntimeBlocked => EventPayloadKind::RuntimeBlocked { data: runtime() },
            EventKind::RuntimeExpired => EventPayloadKind::RuntimeExpired { data: runtime() },
            EventKind::RuntimeResuming => EventPayloadKind::RuntimeResuming { data: runtime() },
            EventKind::RuntimeStopped => EventPayloadKind::RuntimeStopped { data: runtime() },
            EventKind::RuntimeError => EventPayloadKind::RuntimeError {
                code: text("code"),
                message: text("message"),
            },
            EventKind::TurnStarted => EventPayloadKind::TurnStarted,
            EventKind::TurnCompleted => EventPayloadKind::TurnCompleted,
            EventKind::TurnInterrupted => EventPayloadKind::TurnInterrupted {
                provider: optional_text(&value, "provider"),
                code: optional_text(&value, "code"),
                message: optional_text(&value, "message"),
            },
            EventKind::ProviderActivity => {
                let mut data: ProviderActivityEventData =
                    serde_json::from_value(value.clone()).unwrap_or_default();
                if let Some(mut extension) = value.as_object().cloned() {
                    for key in [
                        "provider",
                        "source",
                        "provider_event_type",
                        "provider_item_id",
                        "provider_item_type",
                        "phase",
                        "status",
                        "summary",
                        "raw_event",
                        "raw_event_truncated",
                        "raw_event_original_chars",
                        "raw_event_preview",
                        "dropped_count",
                        "stream",
                        "limit_bytes",
                        "stdout_truncated",
                        "stderr_truncated",
                        "dropped_activity_count",
                        "max_process_output_bytes",
                        "max_activity_events",
                    ] {
                        extension.remove(key);
                    }
                    if !extension.is_empty() {
                        data.extension = Some(serde_json::Value::Object(extension).into());
                    }
                }
                EventPayloadKind::ProviderActivity { data }
            }
            EventKind::ProviderInteractionRequested => {
                EventPayloadKind::ProviderInteractionRequested {
                    provider_interaction_id: ProviderInteractionId::from(text(
                        "provider_interaction_id",
                    )),
                    runtime_attempt_id: RuntimeAttemptId::from(text("runtime_attempt_id")),
                    interaction_kind: value
                        .get("interaction_kind")
                        .cloned()
                        .and_then(|value| serde_json::from_value(value).ok())
                        .unwrap_or(ProviderInteractionKind::Approval),
                    prompt: text("prompt"),
                    expires_at: value
                        .get("expires_at")
                        .cloned()
                        .and_then(|value| serde_json::from_value(value).ok()),
                }
            }
            EventKind::ProviderInteractionResolved => {
                EventPayloadKind::ProviderInteractionResolved {
                    provider_interaction_id: ProviderInteractionId::from(text(
                        "provider_interaction_id",
                    )),
                    runtime_attempt_id: RuntimeAttemptId::from(text("runtime_attempt_id")),
                    interaction_kind: value
                        .get("interaction_kind")
                        .cloned()
                        .and_then(|value| serde_json::from_value(value).ok())
                        .unwrap_or(ProviderInteractionKind::Approval),
                    approved: value.get("approved").and_then(serde_json::Value::as_bool),
                    answers: value
                        .get("answers")
                        .cloned()
                        .and_then(|value| serde_json::from_value(value).ok())
                        .unwrap_or_default(),
                }
            }
            EventKind::ProviderOutputDelta => EventPayloadKind::ProviderOutputDelta {
                content: text("content"),
            },
            EventKind::ProviderMessageCompleted => EventPayloadKind::ProviderMessageCompleted {
                content: text("content"),
            },
            EventKind::ApprovalRequested => EventPayloadKind::ApprovalRequested {
                approval_id: ApprovalId::from(text("approval_id")),
                prompt: text("prompt"),
                provider: optional_text(&value, "provider"),
                provider_event_type: optional_text(&value, "provider_event_type"),
                source: optional_text(&value, "source"),
            },
            EventKind::ApprovalResolved => EventPayloadKind::ApprovalResolved {
                approval_id: ApprovalId::from(text("approval_id")),
                approved: value
                    .get("approved")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                message: text("message"),
            },
            EventKind::RuntimePolicyEffective => EventPayloadKind::RuntimePolicyEffective {
                policy: value
                    .get("policy")
                    .cloned()
                    .and_then(|value| serde_json::from_value(value).ok()),
                policy_hash: value
                    .get("policy_hash")
                    .cloned()
                    .and_then(|value| serde_json::from_value(value).ok()),
            },
            EventKind::RuntimePolicyOverride => EventPayloadKind::RuntimePolicyOverride {
                policy_hash: value
                    .get("policy_hash")
                    .cloned()
                    .and_then(|value| serde_json::from_value(value).ok()),
                policy_override: value
                    .get("policy_override")
                    .cloned()
                    .and_then(|value| serde_json::from_value(value).ok()),
            },
            EventKind::CoordinationWarningAcknowledged => {
                EventPayloadKind::CoordinationWarningAcknowledged {
                    warning_kind: text("warning_kind"),
                    message: optional_text(&value, "message"),
                    affected_refs: value
                        .get("affected_refs")
                        .cloned()
                        .and_then(|value| serde_json::from_value(value).ok())
                        .unwrap_or_default(),
                }
            }
            EventKind::WorkspaceValidated | EventKind::ResourceSnapshotUpdated => {
                let placement_id = ProjectPlacementId::from(text("placement_id"));
                let display_name = text("display_name");
                let workspace_path = text("workspace_path");
                let state = value
                    .get("state")
                    .cloned()
                    .and_then(|value| serde_json::from_value(value).ok())
                    .unwrap_or(PlacementState::Error);
                let resource_badges = value
                    .get("resource_badges")
                    .cloned()
                    .and_then(|value| serde_json::from_value(value).ok())
                    .unwrap_or_default();
                let git_snapshot = value
                    .get("git_snapshot")
                    .cloned()
                    .and_then(|value| serde_json::from_value(value).ok());
                if kind == EventKind::WorkspaceValidated {
                    EventPayloadKind::WorkspaceValidated {
                        placement_id,
                        display_name,
                        workspace_path,
                        state,
                        resource_badges,
                        git_snapshot,
                    }
                } else {
                    EventPayloadKind::ResourceSnapshotUpdated {
                        placement_id,
                        display_name,
                        workspace_path,
                        state,
                        resource_badges,
                        git_snapshot,
                    }
                }
            }
            EventKind::WorkspaceFileWritten => EventPayloadKind::WorkspaceFileWritten {
                placement_id: ProjectPlacementId::from(text("placement_id")),
                path: text("path"),
                edit_id: text("edit_id"),
            },
            EventKind::WorkspaceCommandCompleted => EventPayloadKind::WorkspaceCommandCompleted {
                placement_id: ProjectPlacementId::from(text("placement_id")),
                terminal_command_id: text("terminal_command_id"),
                success: bool_field(&value, "success"),
                exit_code: i32_field(&value, "exit_code"),
                stdout_truncated: bool_field(&value, "stdout_truncated"),
                stderr_truncated: bool_field(&value, "stderr_truncated"),
            },
            EventKind::WorkspaceCheckCompleted => EventPayloadKind::WorkspaceCheckCompleted {
                placement_id: ProjectPlacementId::from(text("placement_id")),
                check_run_id: optional_text(&value, "check_run_id")
                    .unwrap_or_else(|| text("terminal_command_id")),
                success: bool_field(&value, "success"),
                exit_code: i32_field(&value, "exit_code"),
                stdout_truncated: bool_field(&value, "stdout_truncated"),
                stderr_truncated: bool_field(&value, "stderr_truncated"),
            },
            EventKind::WorkspaceDiffObserved => EventPayloadKind::WorkspaceDiffObserved {
                placement_id: ProjectPlacementId::from(text("placement_id")),
                diff_id: text("diff_id"),
                summary_truncated: bool_field(&value, "summary_truncated"),
                diff_truncated: bool_field(&value, "diff_truncated"),
            },
            EventKind::DeductionRequested => EventPayloadKind::DeductionRequested {
                deduction_id: DeductionId::from(text("deduction_id")),
                scope_ref: value
                    .get("scope_ref")
                    .cloned()
                    .and_then(|value| serde_json::from_value(value).ok())
                    .unwrap_or_else(|| UpravaRef::Unknown {
                        ref_type: "deduction_scope".to_owned(),
                        locator: serde_json_value::JsonValue::default(),
                    }),
                question: text("question"),
            },
            EventKind::DeductionCompleted => EventPayloadKind::DeductionCompleted {
                deduction_id: DeductionId::from(text("deduction_id")),
            },
            EventKind::DeductionInvalid => EventPayloadKind::DeductionInvalid {
                deduction_id: DeductionId::from(text("deduction_id")),
                code: text("code"),
                message: text("message"),
            },
            EventKind::DeductionFailed => EventPayloadKind::DeductionFailed {
                deduction_id: DeductionId::from(text("deduction_id")),
                code: text("code"),
                message: text("message"),
            },
            EventKind::DeductionCancelled => EventPayloadKind::DeductionCancelled {
                deduction_id: DeductionId::from(text("deduction_id")),
            },
            EventKind::TaskRunStateChanged => EventPayloadKind::TaskRunStateChanged {
                task_run_id: TaskRunId::from(text("task_run_id")),
                state: value
                    .get("state")
                    .cloned()
                    .and_then(|value| serde_json::from_value(value).ok())
                    .unwrap_or(TaskRunState::Failed),
                cleanup_state: value
                    .get("cleanup_state")
                    .cloned()
                    .and_then(|value| serde_json::from_value(value).ok())
                    .unwrap_or(TaskCleanupState::Pending),
                message: optional_text(&value, "message"),
            },
            EventKind::Extension => EventPayloadKind::Extension {
                name: optional_text(&value, "name").unwrap_or_else(|| "unknown".to_owned()),
                value: value.get("value").cloned().unwrap_or_default().into(),
            },
        };
        payload.into()
    }
}

impl From<EventPayloadKind> for EventPayload {
    fn from(kind: EventPayloadKind) -> Self {
        let json = serde_json::to_value(&kind)
            .expect("EventPayloadKind serialization is infallible for JSON-compatible fields")
            .into();
        Self { kind, json }
    }
}

impl std::ops::Deref for EventPayload {
    type Target = serde_json_value::JsonValue;

    fn deref(&self) -> &Self::Target {
        &self.json
    }
}

impl PartialEq for EventPayload {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

impl Eq for EventPayload {}

impl Serialize for EventPayload {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.kind.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for EventPayload {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        EventPayloadKind::deserialize(deserializer).map(Into::into)
    }
}

fn optional_text(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

fn bool_field(value: &serde_json::Value, key: &str) -> bool {
    value
        .get(key)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn i32_field(value: &serde_json::Value, key: &str) -> Option<i32> {
    value
        .get(key)
        .and_then(serde_json::Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ControlFrame {
    Hello {
        frame_id: String,
        protocol_version: String,
        sent_at: DateTime<Utc>,
        node_id: NodeId,
        daemon_version: String,
        active_runtime_ids: Vec<RuntimeSessionId>,
        #[serde(default)]
        actual_runtime_attempts: Vec<RuntimeAttemptActualState>,
    },
    HelloAck {
        frame_id: String,
        protocol_version: String,
        sent_at: DateTime<Utc>,
    },
    CommandDispatch {
        frame_id: String,
        protocol_version: String,
        sent_at: DateTime<Utc>,
        command: Box<CommandEnvelope>,
    },
    CommandAck {
        frame_id: String,
        protocol_version: String,
        sent_at: DateTime<Utc>,
        command_id: CommandId,
        status: CommandState,
    },
    CommandResult {
        frame_id: String,
        protocol_version: String,
        sent_at: DateTime<Utc>,
        command_id: CommandId,
        status: CommandState,
        payload: serde_json_value::JsonValue,
    },
    EventBatch {
        frame_id: String,
        protocol_version: String,
        sent_at: DateTime<Utc>,
        events: Vec<EventEnvelope>,
    },
    EventBatchAck {
        frame_id: String,
        protocol_version: String,
        sent_at: DateTime<Utc>,
        accepted_event_ids: Vec<EventId>,
    },
    WorkspaceTerminalAttach {
        frame_id: String,
        protocol_version: String,
        sent_at: DateTime<Utc>,
        terminal_id: TerminalId,
    },
    WorkspaceTerminalInput {
        frame_id: String,
        protocol_version: String,
        sent_at: DateTime<Utc>,
        terminal_id: TerminalId,
        data: String,
    },
    WorkspaceTerminalResize {
        frame_id: String,
        protocol_version: String,
        sent_at: DateTime<Utc>,
        terminal_id: TerminalId,
        cols: u16,
        rows: u16,
    },
    WorkspaceTerminalClose {
        frame_id: String,
        protocol_version: String,
        sent_at: DateTime<Utc>,
        terminal_id: TerminalId,
    },
    WorkspaceTerminalOutput {
        frame_id: String,
        protocol_version: String,
        sent_at: DateTime<Utc>,
        terminal_id: TerminalId,
        seq: u64,
        data: String,
    },
    WorkspaceTerminalStatus {
        frame_id: String,
        protocol_version: String,
        sent_at: DateTime<Utc>,
        terminal_id: TerminalId,
        state: WorkspaceTerminalState,
        exit_code: Option<i32>,
        message: Option<String>,
    },
    Ping {
        frame_id: String,
        protocol_version: String,
        sent_at: DateTime<Utc>,
    },
    Pong {
        frame_id: String,
        protocol_version: String,
        sent_at: DateTime<Utc>,
    },
    ControlError {
        frame_id: String,
        protocol_version: String,
        sent_at: DateTime<Utc>,
        error: ApiError,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub error_code: String,
    pub message: String,
    #[serde(default)]
    pub details: serde_json_value::JsonValue,
    pub retryable: bool,
    pub correlation_id: CorrelationId,
}
