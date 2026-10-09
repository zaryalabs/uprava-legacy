use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpravaRef {
    Node {
        node_id: NodeId,
    },
    Project {
        project_id: ProjectId,
    },
    Placement {
        placement_id: ProjectPlacementId,
    },
    Workspace {
        placement_id: ProjectPlacementId,
    },
    Session {
        session_thread_id: SessionThreadId,
    },
    Runtime {
        runtime_session_id: RuntimeSessionId,
    },
    RuntimeAttempt {
        runtime_attempt_id: RuntimeAttemptId,
    },
    ProviderInteraction {
        provider_interaction_id: ProviderInteractionId,
    },
    TaskRun {
        task_run_id: TaskRunId,
    },
    Turn {
        turn_id: TurnId,
    },
    Message {
        message_id: MessageId,
    },
    MessageRange {
        message_id: MessageId,
        range: TextRange,
    },
    Block {
        block_id: BlockId,
    },
    Artifact {
        artifact_id: ArtifactId,
    },
    ArtifactVersion {
        artifact_id: ArtifactId,
        version: u64,
    },
    Deduction {
        deduction_id: DeductionId,
    },
    Event {
        event_id: EventId,
        scope_ref: Box<ScopeRef>,
        seq: i64,
    },
    Command {
        command_id: CommandId,
    },
    Approval {
        approval_id: ApprovalId,
    },
    Warning {
        warning_kind: String,
        command_id: Option<CommandId>,
    },
    ToolCall {
        tool_call_id: String,
    },
    File {
        placement_id: ProjectPlacementId,
        path: String,
        version: Option<String>,
    },
    FileRange {
        placement_id: ProjectPlacementId,
        path: String,
        range: TextRange,
        version: Option<String>,
    },
    Terminal {
        terminal_id: String,
        placement_id: ProjectPlacementId,
    },
    TerminalCommand {
        terminal_command_id: String,
        terminal_id: Option<String>,
    },
    TerminalOutputRange {
        terminal_command_id: String,
        range: TextRange,
    },
    DiffHunk {
        diff_id: String,
        hunk_id: String,
    },
    WorkspaceDiff {
        diff_id: String,
        placement_id: ProjectPlacementId,
    },
    CheckResult {
        check_run_id: String,
        failure_id: Option<String>,
    },
    WorkspaceEdit {
        edit_id: String,
        placement_id: Option<ProjectPlacementId>,
        path: Option<String>,
    },
    TraceEvent {
        trace_event_id: String,
    },
    ExternalEntity {
        integration_kind: String,
        external_id: String,
    },
    Unknown {
        ref_type: String,
        locator: serde_json_value::JsonValue,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextRange {
    pub start_line: Option<i64>,
    pub end_line: Option<i64>,
    pub start_offset: Option<i64>,
    pub end_offset: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TracePrecision {
    Exact,
    Coarse,
    AgentAuthored,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceResolutionStatus {
    Resolved,
    Missing,
    Offline,
    Redacted,
    Unsupported,
    RawOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CausalityLinks {
    #[serde(default)]
    pub source_refs: Vec<UpravaRef>,
    #[serde(default)]
    pub evidence_refs: Vec<UpravaRef>,
    #[serde(default)]
    pub cause_refs: Vec<UpravaRef>,
    #[serde(default)]
    pub result_refs: Vec<UpravaRef>,
    #[serde(default)]
    pub raw_refs: Vec<UpravaRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceStep {
    pub block_id: BlockId,
    pub title: String,
    pub summary: String,
    pub actor_ref: ActorRef,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub precision: TracePrecision,
    pub primary_ref: UpravaRef,
    #[serde(flatten)]
    pub links: CausalityLinks,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTraceProjection {
    pub session_thread_id: SessionThreadId,
    pub precision: TracePrecision,
    #[serde(default)]
    pub steps: Vec<TraceStep>,
    pub raw_event_count: u64,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceResolution {
    pub reference: UpravaRef,
    pub status: ReferenceResolutionStatus,
    pub title: String,
    pub summary: Option<String>,
    #[serde(flatten)]
    pub links: CausalityLinks,
    pub raw_payload: Option<serde_json_value::JsonValue>,
    pub raw_truncated: bool,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventLogPage {
    #[serde(default)]
    pub events: Vec<EventEnvelope>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeductionState {
    Requested,
    Running,
    Completed,
    Invalid,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeductionClassification {
    Observed,
    Inference,
    Assumption,
    Unknown,
    Alternative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeductionCertainty {
    High,
    Medium,
    Low,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeductionStep {
    pub step_id: String,
    pub classification: DeductionClassification,
    pub summary: String,
    #[serde(default)]
    pub support_refs: Vec<UpravaRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeductionProviderResult {
    pub title: String,
    pub conclusion: String,
    pub certainty: DeductionCertainty,
    #[serde(default)]
    pub steps: Vec<DeductionStep>,
    #[serde(default)]
    pub assumptions: Vec<String>,
    #[serde(default)]
    pub unknowns: Vec<String>,
    #[serde(default)]
    pub alternatives: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeductionProvenance {
    pub provider: String,
    pub model: Option<String>,
    pub session_thread_id: SessionThreadId,
    pub schema_version: String,
    pub evidence_snapshot_hash: String,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeductionBlock {
    pub deduction_id: DeductionId,
    pub scope_ref: UpravaRef,
    #[serde(flatten)]
    pub result: DeductionProviderResult,
    pub provenance: DeductionProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeductionEvidenceEvent {
    pub event_ref: UpravaRef,
    pub kind: EventKind,
    pub summary: String,
    pub happened_at: DateTime<Utc>,
    #[serde(flatten)]
    pub links: CausalityLinks,
    pub raw_excerpt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeductionInputPackage {
    pub deduction_id: DeductionId,
    pub session_thread_id: SessionThreadId,
    pub scope_ref: UpravaRef,
    pub question: String,
    pub evidence_snapshot_hash: String,
    #[serde(default)]
    pub trace_steps: Vec<TraceStep>,
    #[serde(default)]
    pub events: Vec<DeductionEvidenceEvent>,
    #[serde(default)]
    pub allowed_refs: Vec<UpravaRef>,
    pub truncated: bool,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeductionProviderOutput {
    pub deduction_id: DeductionId,
    pub provider: String,
    pub model: Option<String>,
    pub schema_version: String,
    pub evidence_snapshot_hash: String,
    pub result: Option<DeductionProviderResult>,
    pub raw_text: String,
    pub raw_truncated: bool,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionEvidenceProjectionNode {
    pub evidence_id: EvidenceId,
    pub label: String,
    pub primary_ref: UpravaRef,
    #[serde(default)]
    pub source_refs: Vec<UpravaRef>,
    #[serde(default)]
    pub evidence_refs: Vec<UpravaRef>,
    #[serde(default)]
    pub cause_refs: Vec<UpravaRef>,
    #[serde(default)]
    pub children: Vec<SessionEvidenceProjectionNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionEvidenceProjection {
    pub session_thread_id: SessionThreadId,
    pub root: SessionEvidenceProjectionNode,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentProjection {
    pub session_thread_id: SessionThreadId,
    pub project_placement: ProjectPlacementSummary,
    pub runtime_summary: RuntimeSummary,
    pub current_turn: Option<TurnId>,
    #[serde(default)]
    pub pending_approvals: Vec<ApprovalId>,
    #[serde(default)]
    pub pending_interactions: Vec<ProviderInteractionSummary>,
    #[serde(default)]
    pub active_warnings: Vec<ResourceBadge>,
    #[serde(default)]
    pub recent_turn_summaries: Vec<String>,
    #[serde(default)]
    pub recent_message_refs: Vec<UpravaRef>,
    pub evidence_projection_summary: String,
    #[serde(default)]
    pub available_block_types: Vec<String>,
    #[serde(default)]
    pub available_commands: Vec<ActionCapability>,
    #[serde(default)]
    pub visible_refs: Vec<UpravaRef>,
    pub source_cause_summary: String,
    pub resume_context: String,
    pub generated_at: DateTime<Utc>,
}
