import type {
  AGENT_EXECUTION_PROFILE_VALUES,
  CLIENT_LOG_LEVEL_VALUES,
  ACTION_CAPABILITY_VALUES,
  COMMAND_KIND_VALUES,
  COMMAND_STATE_VALUES,
  DEPLOYMENT_PROFILE_VALUES,
  EVENT_KIND_VALUES,
  INTEGRATION_AUTH_STATE_VALUES,
  INTEGRATION_DESIRED_STATE_VALUES,
  MCP_DEPENDENCY_ACTUAL_STATE_VALUES,
  MESSAGE_ROLE_VALUES,
  NODE_PRESENCE_VALUES,
  OBSERVED_CAPABILITY_STATE_VALUES,
  PLACEMENT_STATE_VALUES,
  POLICY_DECISION_VALUES,
  RUNTIME_SESSION_STATE_VALUES,
  RUNTIME_ATTEMPT_STATE_VALUES,
  RUNTIME_RECOVERY_STATUS_VALUES,
  PROVIDER_RUNTIME_CAPABILITY_VALUES,
  PROVIDER_INTERACTION_KIND_VALUES,
  PROVIDER_INTERACTION_STATE_VALUES,
  SCHEDULED_MESSAGE_STATE_VALUES,
  SESSION_THREAD_STATE_VALUES,
  TOOL_AVAILABILITY_STATE_VALUES,
  TOOL_CALL_STATE_VALUES,
  TOOL_DEFINITION_STATE_VALUES,
  TOOL_EXECUTION_ERROR_CODE_VALUES,
  TOOL_EXECUTION_KIND_VALUES,
  TOOL_INVOCATION_MODE_VALUES,
  TOOL_RISK_LEVEL_VALUES,
  TOOL_SOURCE_KIND_VALUES,
  TOOL_UNAVAILABLE_REASON_VALUES,
  WARNING_SEVERITY_VALUES,
  WORKSPACE_COMMAND_INTENT_VALUES,
  WORKSPACE_ENTRY_KIND_VALUES,
  WORKSPACE_ENTRY_STATUS_VALUES,
  WORKSPACE_TERMINAL_STATE_VALUES,
} from "./literals";

export type DeploymentProfile = (typeof DEPLOYMENT_PROFILE_VALUES)[number];
export type AgentExecutionProfile =
  (typeof AGENT_EXECUTION_PROFILE_VALUES)[number];
export type RuntimeAttemptState = (typeof RUNTIME_ATTEMPT_STATE_VALUES)[number];
export type RuntimeRecoveryStatus =
  (typeof RUNTIME_RECOVERY_STATUS_VALUES)[number];
export type ProviderRuntimeCapability =
  (typeof PROVIDER_RUNTIME_CAPABILITY_VALUES)[number];
export type ProviderInteractionKind =
  (typeof PROVIDER_INTERACTION_KIND_VALUES)[number];
export type ProviderInteractionState =
  (typeof PROVIDER_INTERACTION_STATE_VALUES)[number];
export type NodePresence = (typeof NODE_PRESENCE_VALUES)[number];
export type RuntimeSessionState = (typeof RUNTIME_SESSION_STATE_VALUES)[number];
export type SessionThreadState = (typeof SESSION_THREAD_STATE_VALUES)[number];
export type PlacementState = (typeof PLACEMENT_STATE_VALUES)[number];
export type WarningSeverity = (typeof WARNING_SEVERITY_VALUES)[number];
export type ClientLogLevel = (typeof CLIENT_LOG_LEVEL_VALUES)[number];
export type CommandState = (typeof COMMAND_STATE_VALUES)[number];
export type CommandKind = (typeof COMMAND_KIND_VALUES)[number];
export type EventKind = (typeof EVENT_KIND_VALUES)[number];
export type ActionCapability = (typeof ACTION_CAPABILITY_VALUES)[number];
export type MessageRole = (typeof MESSAGE_ROLE_VALUES)[number];
export type ScheduledMessageState =
  (typeof SCHEDULED_MESSAGE_STATE_VALUES)[number];
export type JobRunState =
  | "queued"
  | "starting"
  | "running"
  | "succeeded"
  | "failed"
  | "cancelled"
  | "timed_out"
  | "skipped";
export type TaskRunState =
  | "queued"
  | "preparing_workspace"
  | "starting_runtime"
  | "running"
  | "checking"
  | "collecting_evidence"
  | "succeeded"
  | "failed"
  | "cancelling"
  | "cancelled"
  | "timed_out";
export type TaskCleanupState = "pending" | "completed" | "failed";
export type EnrollmentState =
  | "pending_user_approval"
  | "approved"
  | "registered"
  | "expired"
  | "rejected"
  | "revoked";

export type HealthResponse = {
  status: string;
  profile: DeploymentProfile;
  security?: SecurityStatus;
};

export type VersionResponse = {
  name: string;
  version: string;
  release_id: string;
  api_version: string;
  schema_version: number;
  profile: DeploymentProfile;
  security?: SecurityStatus;
};

export type SecurityMode = "hardened";

export type SecurityStatus = {
  mode: SecurityMode;
  web_auth_required: boolean;
  web_auth_configured: boolean;
  cookie_secure: boolean;
};

export type ApiError = {
  error_code: string;
  message: string;
  details?: unknown;
  retryable: boolean;
  correlation_id: string;
};

export type CapabilitySummary = {
  key: string;
  value: CapabilityValue;
};

export type CapabilityValue =
  | {
      kind: "provider";
      available: boolean;
      configured: boolean;
      mode: string;
      timeout_seconds: number | null;
      unavailable_reason: string | null;
    }
  | { kind: "workspace_validation"; mode: string }
  | { kind: "extension"; name: string; value: unknown };

export type NodeSummary = {
  node_id: string;
  display_name: string;
  presence: NodePresence;
  sleep_hint: string;
  heartbeat_age_seconds: number | null;
  active_runtime_count: number;
  capabilities: CapabilitySummary[];
  diagnostics: string;
};

export type ResourceBadge = {
  kind: string;
  severity: WarningSeverity;
  label: string;
};

export type ProjectPlacementSummary = {
  project_placement_id: string;
  project_id: string | null;
  node_id: string;
  display_name: string;
  workspace_path: string;
  state: PlacementState;
  resource_badges: ResourceBadge[];
  git_snapshot?: GitWorkspaceSnapshot | null;
  last_validated_at: string | null;
};

export type GitRepositoryState = "ready" | "not_repository" | "unavailable";
export type GitHeadState = "branch" | "detached" | "unborn";
export type GitWorktreeKind = "primary" | "linked";
export type GitOperation =
  | "merge"
  | "rebase"
  | "cherry_pick"
  | "revert"
  | "bisect";
export type GitChangeKind =
  | "added"
  | "modified"
  | "deleted"
  | "renamed"
  | "copied"
  | "untracked"
  | "unmerged"
  | "type_changed"
  | "unknown";

export type GitChangedFile = {
  path: string;
  previous_path: string | null;
  index_status: GitChangeKind | null;
  worktree_status: GitChangeKind | null;
  conflicted: boolean;
  binary: boolean;
};

export type GitWorkspaceSnapshot = {
  state: GitRepositoryState;
  repo_id: string | null;
  head_state: GitHeadState | null;
  branch: string | null;
  commit: string | null;
  upstream: string | null;
  ahead: number;
  behind: number;
  worktree_kind: GitWorktreeKind | null;
  operation: GitOperation | null;
  changed_files: GitChangedFile[];
  staged_count: number;
  unstaged_count: number;
  untracked_count: number;
  conflicted_count: number;
  truncated: boolean;
  generated_at: string;
};

export type WorkspaceDiffHunk = {
  hunk_id: string;
  header: string;
  patch: string;
};

export type WorkspaceEntryKind = (typeof WORKSPACE_ENTRY_KIND_VALUES)[number];

export type WorkspaceEntryStatus =
  (typeof WORKSPACE_ENTRY_STATUS_VALUES)[number];

export type WorkspaceEntryClassification = "normal" | "generated" | "ignored";

export type WorkspaceEntry = {
  name: string;
  path: string;
  kind: WorkspaceEntryKind;
  status: WorkspaceEntryStatus;
  classification: WorkspaceEntryClassification;
  expandable: boolean;
  byte_len: number | null;
  modified_at: string | null;
  children: WorkspaceEntry[];
};

export type WorkspaceTreeResponse = {
  placement_id: string;
  root: WorkspaceEntry;
  truncated: boolean;
  total_entries: number | null;
  generated_at: string;
};

export type WorkspaceFileContentResponse = {
  placement_id: string;
  path: string;
  metadata: WorkspaceEntry;
  content: string | null;
  truncated: boolean;
  generated_at: string;
};

export type WorkspaceFileWriteRequest = {
  path: string;
  content: string;
  expected_content: string | null;
};

export type WorkspaceFileWriteResponse = {
  placement_id: string;
  path: string;
  metadata: WorkspaceEntry;
  edit_id: string;
  written_at: string;
};

export type WorkspaceCommandIntent =
  (typeof WORKSPACE_COMMAND_INTENT_VALUES)[number];

export type WorkspaceCommandRunRequest = {
  command: string;
  args: string[];
  intent: WorkspaceCommandIntent;
  label: string | null;
  timeout_seconds: number | null;
};

export type WorkspaceCommandRunResponse = {
  placement_id: string;
  terminal_command_id: string;
  command: string;
  args: string[];
  intent: WorkspaceCommandIntent;
  label: string | null;
  exit_code: number | null;
  success: boolean;
  stdout: string;
  stderr: string;
  stdout_truncated: boolean;
  stderr_truncated: boolean;
  duration_ms: number;
  started_at: string;
  completed_at: string;
};

export type WorkspaceDiffResponse = {
  placement_id: string;
  diff_id: string;
  git_snapshot: GitWorkspaceSnapshot;
  summary: string;
  diff: string;
  scope: WorkspaceDiffScope;
  path: string | null;
  changed_files: GitChangedFile[];
  hunks: WorkspaceDiffHunk[];
  original: string | null;
  modified: string | null;
  binary: boolean;
  summary_truncated: boolean;
  diff_truncated: boolean;
  generated_at: string;
};

export type WorkspaceDiffScope = "all" | "staged" | "unstaged";

export type WorkspaceCheckRunSummary = {
  command_id: string;
  state: CommandState;
  command: string;
  args: string[];
  label: string | null;
  success: boolean | null;
  exit_code: number | null;
  stdout: string | null;
  stderr: string | null;
  stdout_truncated: boolean;
  stderr_truncated: boolean;
  duration_ms: number | null;
  created_at: string;
  completed_at: string | null;
};

export type WorkspaceReviewProjection = {
  placement_id: string;
  git_snapshot: GitWorkspaceSnapshot;
  diff: WorkspaceDiffResponse;
  checks: WorkspaceCheckRunSummary[];
  generated_at: string;
};

export type WorkspaceCommandHistoryItem = {
  command_id: string;
  kind: CommandKind;
  state: CommandState;
  created_at: string;
  completed_at: string | null;
  payload: unknown;
  result_payload: unknown | null;
};

export type WorkspaceCommandHistoryResponse = {
  placement_id: string;
  commands: WorkspaceCommandHistoryItem[];
  generated_at: string;
};

export type WorkspaceTerminalState =
  (typeof WORKSPACE_TERMINAL_STATE_VALUES)[number];

export type WorkspaceTerminalOpenRequest = {
  shell_profile: string | null;
  cols: number;
  rows: number;
};

export type WorkspaceTerminalSummary = {
  placement_id: string;
  terminal_id: string;
  title: string;
  cwd: string;
  shell: string;
  cols: number;
  rows: number;
  state: WorkspaceTerminalState;
  exit_code: number | null;
  created_at: string;
  updated_at: string;
};

export type WorkspaceTerminalOutputFrame = {
  terminal_id: string;
  seq: number;
  data: string;
  sent_at: string;
};

export type WorkspaceTerminalOpenResponse = {
  placement_id: string;
  terminal: WorkspaceTerminalSummary;
  replay: WorkspaceTerminalOutputFrame[];
};

export type WorkspaceTerminalListResponse = {
  placement_id: string;
  terminals: WorkspaceTerminalSummary[];
  generated_at: string;
};

export type WorkspaceTerminalClientFrame =
  | { kind: "input"; data: string }
  | { kind: "resize"; cols: number; rows: number }
  | { kind: "close" }
  | { kind: "ping" };

export type WorkspaceTerminalStreamFrame =
  | {
      kind: "output";
      terminal_id: string;
      seq: number;
      data: string;
      sent_at: string;
    }
  | {
      kind: "status";
      terminal_id: string;
      state: WorkspaceTerminalState;
      exit_code: number | null;
      message: string | null;
      sent_at: string;
    }
  | { kind: "pong"; sent_at: string }
  | {
      kind: "error";
      terminal_id: string;
      message: string;
      sent_at: string;
    };

export type EffectiveRuntimePolicy = {
  contract_version: number;
  execution_profile: AgentExecutionProfile;
  provider: string;
  provider_version: string | null;
  provider_capabilities: ProviderRuntimeCapability[];
  sandbox_mode: "read-only" | "workspace-write" | "danger-full-access";
  approval_mode: "untrusted" | "on_failure" | "on_request" | "never";
  workspace_root: string;
  additional_writable_paths: string[];
  network_posture:
    | "denied"
    | "restricted"
    | "allowed"
    | "provider_default"
    | "unsupported";
  tool_exposure: {
    server_count: number;
    tool_count: number;
    server_names: string[];
  };
  credential_profile_ref: string | null;
  unsafe_override: {
    actor: ActorRef;
    reason: string;
    expires_at: string;
  } | null;
  capability_metadata: Record<string, string>;
};

export type RuntimeAttemptSummary = {
  runtime_attempt_id: string;
  state: RuntimeAttemptState;
  started_at: string;
  ready_at: string | null;
  stopped_at: string | null;
  start_reason: string;
  stop_reason: string | null;
  recovery_reason: string | null;
};

export type ProviderInteractionSummary = {
  provider_interaction_id: string;
  runtime_attempt_id: string;
  kind: ProviderInteractionKind;
  state: ProviderInteractionState;
  prompt: string;
  requested_at: string;
  resolved_at: string | null;
};

export type RuntimeSummary = {
  runtime_session_id: string;
  provider: string;
  execution_profile?: AgentExecutionProfile;
  state: RuntimeSessionState;
  resume_supported: boolean;
  degraded_reason: string | null;
  last_runtime_step_at: string | null;
  current_attempt?: RuntimeAttemptSummary | null;
  effective_policy?: EffectiveRuntimePolicy | null;
  effective_policy_hash?: string | null;
  recovery_status?: RuntimeRecoveryStatus;
};

export type SessionSummary = {
  session_thread_id: string;
  project_placement_id: string;
  runtime_session_id: string;
  title: string;
  state: SessionThreadState;
  runtime: RuntimeSummary;
  message_count: number;
  updated_at: string;
};

export type Message = {
  message_id: string;
  session_thread_id: string;
  turn_id: string | null;
  role: MessageRole;
  content: string;
  created_at: string;
  completed_at: string | null;
  source_event_id: string | null;
};

export type EventEnvelope = {
  event_id: string;
  command_id: string | null;
  correlation_id?: string | null;
  actor_ref: unknown;
  scope_ref: unknown;
  node_id: string | null;
  runtime_session_id: string | null;
  session_thread_id: string | null;
  turn_id: string | null;
  seq: number;
  session_projection_seq?: number | null;
  kind: EventKind;
  happened_at: string;
  source_refs: UpravaRef[];
  evidence_refs: UpravaRef[];
  cause_refs: UpravaRef[];
  result_refs: UpravaRef[];
  payload: EventPayload;
};

export type EventPayload =
  | {
      type:
        | "runtime_attempt_started"
        | "runtime_attempt_ready"
        | "runtime_attempt_disconnected"
        | "runtime_attempt_reconnecting"
        | "runtime_attempt_recovered"
        | "runtime_attempt_failed";
      runtime_attempt_id: string;
      state: RuntimeAttemptState;
      reason: string | null;
      code: string | null;
      message: string | null;
    }
  | ({ type: RuntimeStatePayloadType } & RuntimeStateEventData)
  | { type: "runtime_error"; code: string; message: string }
  | { type: "turn_started" }
  | { type: "turn_completed" }
  | {
      type: "turn_interrupted";
      provider: string | null;
      code: string | null;
      message: string | null;
    }
  | ({ type: "provider_activity" } & ProviderActivityEventData)
  | {
      type: "provider_interaction_requested";
      provider_interaction_id: string;
      runtime_attempt_id: string;
      interaction_kind: ProviderInteractionKind;
      prompt: string;
      expires_at: string | null;
    }
  | {
      type: "provider_interaction_resolved";
      provider_interaction_id: string;
      runtime_attempt_id: string;
      interaction_kind: ProviderInteractionKind;
      approved: boolean | null;
      answers: string[];
    }
  | { type: "provider_output_delta"; content: string }
  | { type: "provider_message_completed"; content: string }
  | {
      type: "approval_requested";
      approval_id: string;
      prompt: string;
      provider: string | null;
      provider_event_type: string | null;
      source: string | null;
    }
  | {
      type: "approval_resolved";
      approval_id: string;
      approved: boolean;
      message: string;
    }
  | {
      type: "runtime_policy_effective";
      policy: EffectiveRuntimePolicy | null;
      policy_hash: string | null;
    }
  | {
      type: "runtime_policy_override";
      policy_hash: string | null;
      policy_override: EffectiveRuntimePolicy["unsafe_override"];
    }
  | {
      type: "coordination_warning_acknowledged";
      warning_kind: string;
      message: string | null;
      affected_refs: UpravaRef[];
    }
  | ({ type: "workspace_validated" } & WorkspaceSnapshotEventData)
  | ({ type: "resource_snapshot_updated" } & WorkspaceSnapshotEventData)
  | {
      type: "workspace_file_written";
      placement_id: string;
      path: string;
      edit_id: string;
    }
  | {
      type: "workspace_command_completed";
      placement_id: string;
      terminal_command_id: string;
      success: boolean;
      exit_code: number | null;
      stdout_truncated: boolean;
      stderr_truncated: boolean;
    }
  | {
      type: "workspace_check_completed";
      placement_id: string;
      check_run_id: string;
      success: boolean;
      exit_code: number | null;
      stdout_truncated: boolean;
      stderr_truncated: boolean;
    }
  | {
      type: "workspace_diff_observed";
      placement_id: string;
      diff_id: string;
      summary_truncated: boolean;
      diff_truncated: boolean;
    }
  | {
      type: "deduction_requested";
      deduction_id: string;
      scope_ref: UpravaRef;
      question: string;
    }
  | { type: "deduction_completed"; deduction_id: string }
  | { type: "deduction_cancelled"; deduction_id: string }
  | {
      type: "task_run_state_changed";
      task_run_id: string;
      state: TaskRunState;
      cleanup_state: TaskCleanupState;
      message: string | null;
    }
  | {
      type: "deduction_invalid" | "deduction_failed";
      deduction_id: string;
      code: string;
      message: string;
    }
  | { type: "extension"; name: string; value: unknown };

type RuntimeStatePayloadType =
  | "runtime_starting"
  | "runtime_ready"
  | "runtime_running"
  | "runtime_blocked"
  | "runtime_expired"
  | "runtime_resuming"
  | "runtime_stopped";

type RuntimeStateEventData = {
  provider: string | null;
  mode: string | null;
  resume_source: string | null;
  provider_resume_ref: unknown | null;
  transcript_messages: number | null;
  reason: string | null;
  code: string | null;
  message: string | null;
  expiry_seconds: number | null;
};

type ProviderActivityEventData = {
  provider: string | null;
  source: string | null;
  provider_event_type: string | null;
  provider_item_id: string | null;
  provider_item_type: string | null;
  phase: string | null;
  status: string | null;
  summary: string | null;
  raw_event: unknown | null;
  raw_event_truncated: boolean | null;
  raw_event_original_chars: number | null;
  raw_event_preview: string | null;
  dropped_count: number | null;
  stream: string | null;
  limit_bytes: number | null;
  stdout_truncated: boolean | null;
  stderr_truncated: boolean | null;
  dropped_activity_count: number | null;
  max_process_output_bytes: number | null;
  max_activity_events: number | null;
  extension: unknown | null;
};

type WorkspaceSnapshotEventData = {
  placement_id: string;
  display_name: string;
  workspace_path: string;
  state: PlacementState;
  resource_badges: ResourceBadge[];
  git_snapshot: GitWorkspaceSnapshot | null;
};

export type InventorySnapshot = {
  nodes: NodeSummary[];
  placements: ProjectPlacementSummary[];
  sessions: SessionSummary[];
  generated_at: string;
};

export type SessionDetail = {
  session: SessionSummary;
  placement: ProjectPlacementSummary;
  messages: Message[];
  events: EventEnvelope[];
  scheduled_messages?: ScheduledSessionMessage[];
  pending_interactions?: ProviderInteractionSummary[];
};

export type ScheduledMessageFailure = {
  code: string;
  message: string;
};

export type ScheduledSessionMessage = {
  scheduled_message_id: string;
  session_thread_id: string;
  content: string;
  due_at: string;
  timezone: string;
  state: ScheduledMessageState;
  created_at: string;
  updated_at: string;
  sending_at: string | null;
  sent_at: string | null;
  cancelled_at: string | null;
  command_id: string | null;
  turn_id: string | null;
  failure: ScheduledMessageFailure | null;
};

export type CreatePlacementRequest = {
  node_id: string;
  display_name: string;
  workspace_path: string;
};

export type CreateSessionRequest = {
  project_placement_id: string;
  title?: string;
  provider: string;
  execution_profile?: AgentExecutionProfile;
  force?: boolean;
};

export type PreviewSessionPolicyRequest = {
  project_placement_id: string;
  provider: string;
  execution_profile: AgentExecutionProfile;
};

export type SessionPolicyPreview = {
  project_placement_id: string;
  node_id: string;
  effective_policy: EffectiveRuntimePolicy;
  effective_policy_hash: string;
};

export type JobSchedule =
  | { kind: "interval"; minutes: number }
  | { kind: "daily"; hour: number; minute: number }
  | { kind: "weekly"; weekday: number; hour: number; minute: number };

export type JobRunSummary = {
  job_run_id: string;
  job_id: string;
  trigger: "manual" | "scheduled";
  state: JobRunState;
  scheduled_for: string | null;
  queued_at: string;
  started_at: string | null;
  finished_at: string | null;
  session_thread_id: string | null;
  runtime_session_id: string | null;
  summary: string | null;
  terminal_reason: ScheduledMessageFailure | null;
  config_snapshot: unknown;
  force: boolean;
};

export type JobSummary = {
  job_id: string;
  name: string;
  project_placement_id: string;
  placement_name: string;
  provider: string;
  enabled: boolean;
  schedule: JobSchedule | null;
  timezone: string;
  overlap_policy: "skip";
  continue_after_error: boolean;
  next_run_at: string | null;
  paused_reason: string | null;
  latest_run: JobRunSummary | null;
  created_at: string;
  updated_at: string;
};

export type JobDetail = {
  job: JobSummary;
  prompt: string;
  runs: JobRunSummary[];
};

export type CreateJobRequest = {
  name: string;
  project_placement_id: string;
  prompt: string;
  provider: string;
  schedule: JobSchedule | null;
  timezone: string;
  continue_after_error?: boolean;
};

export type UpdateJobRequest = {
  name?: string;
  prompt?: string;
  provider?: string;
  schedule?: JobSchedule;
  clear_schedule?: boolean;
  timezone?: string;
  continue_after_error?: boolean;
};

export type TaskCheckSpec = {
  label: string;
  command: string;
  args: string[];
  timeout_seconds: number;
};

export type TaskResourceLimits = {
  cpu: string;
  memory: string;
};

export type CreateTaskRunRequest = {
  project_placement_id: string;
  prompt: string;
  base_revision: string | null;
  checks: TaskCheckSpec[];
  artifact_paths: string[];
  timeout_seconds: number;
  ttl_seconds: number;
  resource_limits: TaskResourceLimits;
  runtime_image: string | null;
};

export type TaskCheckResult = {
  label: string;
  command: string;
  success: boolean;
  exit_code: number | null;
  stdout: string;
  stderr: string;
  stdout_truncated: boolean;
  stderr_truncated: boolean;
  duration_ms: number;
};

export type TaskArtifactEvidence = {
  path: string;
  size_bytes: number;
  sha256: string;
};

export type TaskRunResultPackage = {
  task_run_id: string;
  state: TaskRunState;
  cleanup_state: TaskCleanupState;
  summary: string;
  base_revision: string;
  final_revision: string | null;
  branch: string;
  worktree_path: string;
  runtime_image: string;
  diff: string;
  diff_truncated: boolean;
  checks: TaskCheckResult[];
  artifacts: TaskArtifactEvidence[];
  unresolved_risks: string[];
  terminal_reason: ScheduledMessageFailure | null;
};

export type TaskRunSummary = {
  task_run_id: string;
  project_placement_id: string;
  placement_name: string;
  node_id: string;
  provider: string;
  state: TaskRunState;
  cleanup_state: TaskCleanupState;
  base_revision: string;
  branch: string;
  runtime_image: string;
  queued_at: string;
  started_at: string | null;
  finished_at: string | null;
  summary: string | null;
  terminal_reason: ScheduledMessageFailure | null;
};

export type TaskRunDetail = {
  task: TaskRunSummary;
  prompt: string;
  checks: TaskCheckSpec[];
  artifact_paths: string[];
  timeout_seconds: number;
  ttl_seconds: number;
  resource_limits: TaskResourceLimits;
  worktree_path: string | null;
  result: TaskRunResultPackage | null;
};

export type TaskRunListResponse = { items: TaskRunSummary[] };

export type ProviderQuotaStatus = {
  provider: string;
  state: "available" | "limited" | "unknown";
  five_hour_remaining_percent: number | null;
  weekly_remaining_percent: number | null;
  observed_at: string | null;
  unavailable_reason: string | null;
};

export type SendTurnRequest = {
  content: string;
};

export type CreateScheduledMessageRequest = {
  content: string;
  due_at: string;
  timezone: string;
};

export type UpdateScheduledMessageRequest = {
  content?: string;
  due_at?: string;
  timezone?: string;
};

export type ResolveApprovalRequest = {
  approved: boolean;
  message?: string | null;
};

export type SubmitProviderInputRequest = {
  answers: string[];
};

export type AcknowledgeWarningRequest = {
  message?: string | null;
};

export type ClientLogRequest = {
  level: ClientLogLevel;
  source: string;
  message: string;
  route?: string | null;
  user_agent?: string | null;
  occurred_at: string;
  detail?: unknown;
};

export type ClientLogResponse = {
  accepted: boolean;
};

export type WebAuthStatusResponse = {
  auth_required: boolean;
  setup_required: boolean;
  authenticated: boolean;
  profile: DeploymentProfile;
  security: SecurityStatus;
};

export type WebAuthSetupRequest = {
  password: string;
};

export type WebAuthLoginRequest = {
  password: string;
};

export type WebAuthResponse = {
  authenticated: boolean;
  setup_required: boolean;
  csrf_token: string | null;
  security: SecurityStatus;
};

export type ClientCreateNodeEnrollmentRequest = {
  display_name: string;
};

export type NodeEnrollmentRequestedResponse = {
  enrollment_id: string;
  pairing_code: string;
  status: EnrollmentState;
  expires_at: string;
};

export type NodeEnrollmentSummary = {
  enrollment_id: string;
  display_name: string;
  status: EnrollmentState;
  claimed_node_id: string | null;
  expires_at: string;
  created_at: string;
  approved_at: string | null;
};

export type ApproveNodeEnrollmentResponse = {
  enrollment: NodeEnrollmentSummary;
};

export type NodeRevocationResponse = {
  node_id: string;
  revoked: boolean;
};

export type NodeCredentialRotationResponse = {
  node_id: string;
  credential: string;
  rotated_at: string;
};

export type NodeDeletionResponse = {
  node_id: string;
  deleted: boolean;
};

export type PlacementDeletionResponse = {
  project_placement_id: string;
  deleted: boolean;
};

export type CommandAcceptedResponse = {
  command_id: string;
  session: SessionDetail | null;
};

export type WarningAcknowledgementResponse = {
  event_id: string;
  session: SessionDetail;
};

export type UpravaRef =
  | { kind: "node"; node_id: string }
  | { kind: "project"; project_id: string }
  | { kind: "placement"; placement_id: string }
  | { kind: "workspace"; placement_id: string }
  | { kind: "session"; session_thread_id: string }
  | { kind: "runtime"; runtime_session_id: string }
  | { kind: "task_run"; task_run_id: string }
  | { kind: "turn"; turn_id: string }
  | { kind: "message"; message_id: string }
  | { kind: "message_range"; message_id: string; range: TextRange }
  | { kind: "block"; block_id: string }
  | { kind: "artifact"; artifact_id: string }
  | { kind: "artifact_version"; artifact_id: string; version: number }
  | { kind: "deduction"; deduction_id: string }
  | { kind: "event"; event_id: string; scope_ref: unknown; seq: number }
  | { kind: "command"; command_id: string }
  | { kind: "approval"; approval_id: string }
  | { kind: "warning"; warning_kind: string; command_id?: string | null }
  | { kind: "tool_call"; tool_call_id: string }
  | {
      kind: "file";
      placement_id: string;
      path: string;
      version?: string | null;
    }
  | {
      kind: "file_range";
      placement_id: string;
      path: string;
      range: TextRange;
      version?: string | null;
    }
  | { kind: "terminal"; terminal_id: string; placement_id: string }
  | {
      kind: "terminal_command";
      terminal_command_id: string;
      terminal_id?: string | null;
    }
  | {
      kind: "terminal_output_range";
      terminal_command_id: string;
      range: TextRange;
    }
  | { kind: "diff_hunk"; diff_id: string; hunk_id: string }
  | { kind: "workspace_diff"; diff_id: string; placement_id: string }
  | { kind: "check_result"; check_run_id: string; failure_id?: string | null }
  | {
      kind: "workspace_edit";
      edit_id: string;
      placement_id?: string | null;
      path?: string | null;
    }
  | { kind: "trace_event"; trace_event_id: string }
  | { kind: "external_entity"; integration_kind: string; external_id: string }
  | { kind: "unknown"; ref_type: string; locator: Record<string, unknown> }
  | { kind: string; [key: string]: unknown };

export type TextRange = {
  start_line?: number | null;
  end_line?: number | null;
  start_offset?: number | null;
  end_offset?: number | null;
};

export type TracePrecision = "exact" | "coarse" | "agent_authored" | "unknown";

export type ReferenceResolutionStatus =
  | "resolved"
  | "missing"
  | "offline"
  | "redacted"
  | "unsupported"
  | "raw_only";

export type CausalityLinks = {
  source_refs: UpravaRef[];
  evidence_refs: UpravaRef[];
  cause_refs: UpravaRef[];
  result_refs: UpravaRef[];
  raw_refs: UpravaRef[];
};

export type TraceStep = CausalityLinks & {
  block_id: string;
  title: string;
  summary: string;
  actor_ref: unknown;
  started_at: string;
  completed_at: string | null;
  precision: TracePrecision;
  primary_ref: UpravaRef;
};

export type SessionTraceProjection = {
  session_thread_id: string;
  precision: TracePrecision;
  steps: TraceStep[];
  raw_event_count: number;
  generated_at: string;
};

export type ReferenceResolution = CausalityLinks & {
  reference: UpravaRef;
  status: ReferenceResolutionStatus;
  title: string;
  summary: string | null;
  raw_payload: unknown | null;
  raw_truncated: boolean;
  unavailable_reason: string | null;
};

export type EventLogPage = {
  events: EventEnvelope[];
  next_cursor: string | null;
};

export type DeductionState =
  | "requested"
  | "running"
  | "completed"
  | "invalid"
  | "failed"
  | "cancelled";

export type DeductionClassification =
  | "observed"
  | "inference"
  | "assumption"
  | "unknown"
  | "alternative";

export type DeductionCertainty = "high" | "medium" | "low" | "unknown";

export type DeductionStep = {
  step_id: string;
  classification: DeductionClassification;
  summary: string;
  support_refs: UpravaRef[];
};

export type DeductionProviderResult = {
  title: string;
  conclusion: string;
  certainty: DeductionCertainty;
  steps: DeductionStep[];
  assumptions: string[];
  unknowns: string[];
  alternatives: string[];
};

export type DeductionBlock = DeductionProviderResult & {
  deduction_id: string;
  scope_ref: UpravaRef;
  provenance: {
    provider: string;
    model: string | null;
    session_thread_id: string;
    schema_version: string;
    evidence_snapshot_hash: string;
    generated_at: string;
  };
};

export type CreateDeductionRequest = {
  scope_ref: UpravaRef;
  question?: string | null;
};

export type DeductionAcceptedResponse = {
  deduction_id: string;
  command_id: string;
};

export type DeductionRecord = {
  deduction_id: string;
  session_thread_id: string;
  scope_ref: UpravaRef;
  question: string;
  state: DeductionState;
  command_id: string;
  block: DeductionBlock | null;
  raw_fallback: string | null;
  raw_truncated: boolean;
  error_code: string | null;
  error_message: string | null;
  artifact_id: string | null;
  created_at: string;
  updated_at: string;
};

export type PersistDeductionResponse = {
  deduction_id: string;
  artifact_id: string;
  version: number;
};

export type SessionEvidenceProjectionNode = {
  evidence_id: string;
  label: string;
  primary_ref: UpravaRef;
  source_refs: UpravaRef[];
  evidence_refs: UpravaRef[];
  cause_refs: UpravaRef[];
  children: SessionEvidenceProjectionNode[];
};

export type SessionEvidenceProjection = {
  session_thread_id: string;
  root: SessionEvidenceProjectionNode;
  generated_at: string;
};

export type AgentProjection = {
  session_thread_id: string;
  project_placement: ProjectPlacementSummary;
  runtime_summary: RuntimeSummary;
  current_turn: string | null;
  pending_approvals: string[];
  pending_interactions?: ProviderInteractionSummary[];
  active_warnings: ResourceBadge[];
  recent_turn_summaries: string[];
  recent_message_refs: UpravaRef[];
  evidence_projection_summary: string;
  available_block_types: string[];
  available_commands: ActionCapability[];
  visible_refs: UpravaRef[];
  source_cause_summary: string;
  resume_context: string;
  generated_at: string;
};

export type ActorRef =
  | { kind: "local_user"; actor_id: string | null }
  | { kind: "system" }
  | { kind: "node"; node_id: string }
  | { kind: "provider"; provider: string }
  | { kind: "unknown" };

export type ToolSourceKind = (typeof TOOL_SOURCE_KIND_VALUES)[number];
export type ToolExecutionKind = (typeof TOOL_EXECUTION_KIND_VALUES)[number];
export type ToolRiskLevel = (typeof TOOL_RISK_LEVEL_VALUES)[number];
export type ToolDefinitionState = (typeof TOOL_DEFINITION_STATE_VALUES)[number];
export type ToolAvailabilityState =
  (typeof TOOL_AVAILABILITY_STATE_VALUES)[number];
export type ToolUnavailableReason =
  (typeof TOOL_UNAVAILABLE_REASON_VALUES)[number];
export type ObservedCapabilityState =
  (typeof OBSERVED_CAPABILITY_STATE_VALUES)[number];
export type IntegrationDesiredState =
  (typeof INTEGRATION_DESIRED_STATE_VALUES)[number];
export type IntegrationAuthState =
  (typeof INTEGRATION_AUTH_STATE_VALUES)[number];
export type McpDependencyActualState =
  (typeof MCP_DEPENDENCY_ACTUAL_STATE_VALUES)[number];
export type PolicyDecision = (typeof POLICY_DECISION_VALUES)[number];
export type ToolCallState = (typeof TOOL_CALL_STATE_VALUES)[number];
export type ToolInvocationMode = (typeof TOOL_INVOCATION_MODE_VALUES)[number];
export type ToolExecutionErrorCode =
  (typeof TOOL_EXECUTION_ERROR_CODE_VALUES)[number];

export type ToolRedactionPolicy = {
  argument_json_pointers: string[];
  result_json_pointers: string[];
  redact_all_arguments: boolean;
  redact_all_result: boolean;
  max_summary_bytes: number;
};

export type ToolDefinition = {
  tool_id: string;
  source_id: string;
  source_kind: ToolSourceKind;
  source_tool_name: string;
  version: number;
  display_name: string;
  short_description: string;
  documentation_url: string | null;
  input_schema: unknown;
  output_schema: unknown | null;
  schema_hash: string;
  risk_level: ToolRiskLevel;
  required_permissions: string[];
  execution_kind: ToolExecutionKind;
  approval_policy: PolicyDecision;
  redaction: ToolRedactionPolicy;
  state: ToolDefinitionState;
  created_at: string;
  updated_at: string;
};

export type ToolScope = {
  actor_ref: ActorRef;
  node_id: string | null;
  project_id: string | null;
  project_placement_id: string | null;
  session_thread_id: string | null;
};

export type ToolAvailability = {
  tool_id: string;
  scope: ToolScope;
  state: ToolAvailabilityState;
  reason: ToolUnavailableReason | null;
  backend_ref: string | null;
  dependency_instance_id: string | null;
  schema_hash: string;
  policy_version: string;
  observed_at: string;
};

export type ObservedCapability = {
  node_id: string;
  capability_key: string;
  display_name: string;
  state: ObservedCapabilityState;
  version: string | null;
  safe_authentication_state: string | null;
  observed_at: string;
};

export type IntegrationConnectionSummary = {
  integration_id: string;
  source_id: string;
  provider: string;
  display_name: string;
  desired_state: IntegrationDesiredState;
  auth_state: IntegrationAuthState;
  node_id: string | null;
  authenticated_actor_label: string | null;
  connected_at: string | null;
  updated_at: string;
  error_code: string | null;
};

export type McpDependencyStatus = {
  dependency_instance_id: string;
  integration_id: string;
  node_id: string;
  desired_state: IntegrationDesiredState;
  actual_state: McpDependencyActualState;
  runtime_name: string;
  runtime_version: string | null;
  upstream_identity: string | null;
  schema_set_hash: string | null;
  error_code: string | null;
  observed_at: string;
};

export type ToolSearchFilters = {
  source_kinds: ToolSourceKind[];
  risk_levels: ToolRiskLevel[];
  availability_states: ToolAvailabilityState[];
};

export type SearchToolsRequest = {
  scope: ToolScope;
  query: string;
  filters: ToolSearchFilters;
  cursor: string | null;
  limit: number | null;
};

export type ToolSearchResult = {
  tool_id: string;
  display_name: string;
  short_description: string;
  source_kind: ToolSourceKind;
  risk_level: ToolRiskLevel;
  availability_state: ToolAvailabilityState;
  unavailable_reason: ToolUnavailableReason | null;
  schema_hash: string;
};

export type SearchToolsResponse = {
  items: ToolSearchResult[];
  next_cursor: string | null;
};

export type InspectToolRequest = {
  scope: ToolScope;
  tool_id: string;
};

export type InspectToolResponse = {
  definition: ToolDefinition;
  availability: ToolAvailability;
  invocation_mode: ToolInvocationMode;
};

export type ExecuteToolRequest = {
  scope: ToolScope;
  tool_id: string;
  arguments: unknown;
};

export type ToolExecutionError = {
  code: ToolExecutionErrorCode;
  message: string;
  retryable: boolean;
  redacted_details: unknown;
};

export type ToolResultEnvelope = {
  content: unknown;
  summary: string | null;
  truncated: boolean;
  original_size_bytes: number | null;
  artifact_refs: UpravaRef[];
};

export type ExecuteToolResponse = {
  tool_call_id: string;
  state: ToolCallState;
  result: ToolResultEnvelope | null;
  error: ToolExecutionError | null;
};

export type ToolCallSummary = {
  tool_call_id: string;
  tool_id: string;
  schema_hash: string;
  actor_ref: ActorRef;
  scope: ToolScope;
  source_kind: ToolSourceKind;
  state: ToolCallState;
  policy_decision: PolicyDecision;
  route: string;
  requested_at: string;
  started_at: string | null;
  completed_at: string | null;
  correlation_id: string;
};

export type ToolCallDetail = {
  summary: ToolCallSummary;
  command_id: string | null;
  integration_id: string | null;
  dependency_instance_id: string | null;
  policy_version: string;
  redacted_arguments_summary: string | null;
  redacted_result_summary: string | null;
  argument_hash: string | null;
  result_hash: string | null;
  result_size_bytes: number | null;
  trace_refs: UpravaRef[];
  result_refs: UpravaRef[];
  error: ToolExecutionError | null;
};

export type ToolingCommandV1 = {
  contract_version: 1;
  payload:
    | {
        type: "execute_external_tool";
        tool_call_id: string;
        tool_id: string;
        schema_hash: string;
        integration_id: string;
        dependency_instance_id: string;
        scope: ToolScope;
        arguments: unknown;
        deadline_at: string;
        max_result_bytes: number;
      }
    | { type: "cancel_tool_call"; tool_call_id: string; reason: string | null }
    | {
        type: "update_dependency_desired_state";
        dependency_instance_id: string;
        integration_id: string;
        desired_state: IntegrationDesiredState;
        credential_ref: string | null;
      };
};

export type ToolingEventV1 = {
  contract_version: 1;
  payload:
    | { type: "dependency_actual_state_reported"; status: McpDependencyStatus }
    | {
        type: "tool_definitions_discovered";
        dependency_instance_id: string;
        definitions: ToolDefinition[];
        schema_set_hash: string;
      }
    | { type: "tool_call_started"; tool_call_id: string; started_at: string }
    | {
        type: "tool_call_completed";
        tool_call_id: string;
        result: ToolResultEnvelope;
        completed_at: string;
      }
    | {
        type: "tool_call_failed";
        tool_call_id: string;
        error: ToolExecutionError;
        failed_at: string;
      }
    | {
        type: "tool_call_denied";
        tool_call_id: string;
        error: ToolExecutionError;
        denied_at: string;
      }
    | { type: "tool_availability_changed"; availability: ToolAvailability };
};

export type McpAccessLeaseClaims = {
  lease_id: string;
  audience: string;
  actor_ref: ActorRef;
  session_thread_id: string;
  project_id: string | null;
  project_placement_id: string;
  node_id: string;
  issued_at: string;
  expires_at: string;
  credential_version: number;
  resume_command_id: string | null;
};

export type IntegrationConnectRequest = {
  integration_id: string;
  project_id: string | null;
  node_id: string;
};

export type IntegrationConnectResponse = {
  connection: IntegrationConnectionSummary;
  authorization_url: string;
  expires_at: string;
};

export type IntegrationDisconnectRequest = {
  revoke_remote: boolean;
};

export type IntegrationDisconnectResponse = {
  connection: IntegrationConnectionSummary;
  remote_revocation_confirmed: boolean;
};

export type ToolDefinitionsResponse = {
  items: ToolDefinition[];
  next_cursor: string | null;
};

export type ToolAvailabilityResponse = {
  items: ToolAvailability[];
  generated_at: string;
};

export type ObservedCapabilitiesResponse = {
  items: ObservedCapability[];
  generated_at: string;
};

export type IntegrationConnectionsResponse = {
  items: IntegrationConnectionSummary[];
};

export type McpDependencyStatusesResponse = {
  items: McpDependencyStatus[];
  generated_at: string;
};

export type ToolCallsResponse = {
  items: ToolCallSummary[];
  next_cursor: string | null;
};

export type ToolingContractFixture = {
  tool_definition: ToolDefinition;
  availability: ToolAvailability;
  observed_capability: ObservedCapability;
  integration: IntegrationConnectionSummary;
  dependency: McpDependencyStatus;
  search_request: SearchToolsRequest;
  search_response: SearchToolsResponse;
  inspect_request: InspectToolRequest;
  inspect_response: InspectToolResponse;
  execute_request: ExecuteToolRequest;
  execute_response: ExecuteToolResponse;
  tool_call_detail: ToolCallDetail;
  node_command: ToolingCommandV1;
  node_event: ToolingEventV1;
  lease_claims: McpAccessLeaseClaims;
  tool_definitions: ToolDefinitionsResponse;
  tool_availability: ToolAvailabilityResponse;
  observed_capabilities: ObservedCapabilitiesResponse;
  integration_connections: IntegrationConnectionsResponse;
  dependency_statuses: McpDependencyStatusesResponse;
  tool_calls: ToolCallsResponse;
  integration_connect_request: IntegrationConnectRequest;
  integration_connect_response: IntegrationConnectResponse;
  integration_disconnect_request: IntegrationDisconnectRequest;
  integration_disconnect_response: IntegrationDisconnectResponse;
};

export type PluginTrustLevel =
  | "data_only"
  | "trusted_bundled"
  | "sandboxed_web"
  | "sandboxed_node"
  | "external_service";
export type PluginInstallSource =
  | "bundled"
  | "local"
  | "team_catalog"
  | "community_catalog";
export type PluginDesiredState = "disabled" | "enabled";
export type PluginEffectiveState =
  | "disabled"
  | "active"
  | "incompatible"
  | "degraded"
  | "error";
export type PluginCompatibilityState = "compatible" | "incompatible";
export type ThemeKind = "light" | "dark" | "high_contrast";
export type ThemeColorScheme = "light" | "dark";

export type ThemeContributionV1 = {
  theme_id: string;
  label: string;
  kind: ThemeKind;
  color_scheme: ThemeColorScheme;
  semantic_tokens: Record<string, string>;
  monaco: { base: string; colors: Record<string, string> };
  terminal: { colors: Record<string, string> };
};

export type VisualRendererContributionV1 = {
  renderer_id: string;
  implementation_id: string;
  renderer_kind: "content" | "inline_fragment" | "block" | "artifact_viewer";
  accepted_source_kinds: string[];
  render_scopes: VisualRenderScope[];
  allowed_surfaces: string[];
  fallback_strategy: "plain_text" | "source" | "metadata";
  source_matcher: VisualSourceMatcherV1 | null;
  visual_kinds: string[];
  actions: string[];
};

export type VisualRenderScope =
  | "content_enhancement"
  | "inline_fragment"
  | "block"
  | "artifact_viewer"
  | "detail_view";

export type VisualSourceMatcherV1 =
  | { kind: "fenced_language"; language_ids: string[] }
  | { kind: "strict_color_literal"; formats: string[] };

export type ArtifactTypeContributionV1 = {
  artifact_type_id: string;
  display_name: string;
  description: string;
  schema_version: number;
  fallback_strategy: "plain_text" | "source" | "metadata";
};

export type GeneratedUiLayoutIntent = "inline" | "panel" | "canvas";
export type GeneratedUiCapability =
  | "persist_state"
  | "send_agent_input"
  | "open_reference"
  | "request_layout_change";
export type GeneratedUiActionKind =
  | "update_artifact_state"
  | "send_agent_input"
  | "open_reference";

export type GeneratedUiRuntimeContributionV1 = {
  runtime_id: string;
  implementation_id: string;
  runtime_version: string;
  sdk_id: string;
  action_bridge_id: string;
  supported_sdk_versions: string[];
  supported_layouts: GeneratedUiLayoutIntent[];
  sandbox_capabilities: GeneratedUiCapability[];
  allowed_imports: string[];
  max_source_bytes: number;
  max_bundle_bytes: number;
};

export type GeneratedUiSdkContributionV1 = {
  sdk_id: string;
  package_name: string;
  api_version: string;
  design_token_version: string;
  api_schema: unknown;
};

export type GeneratedUiActionBridgeContributionV1 = {
  bridge_id: string;
  supported_actions: GeneratedUiActionKind[];
};

export type PluginContribution =
  | {
      kind: "ui_theme";
      contribution_id: string;
      contract_version: number;
      contribution: ThemeContributionV1;
    }
  | {
      kind: "visual_renderer";
      contribution_id: string;
      contract_version: number;
      contribution: VisualRendererContributionV1;
    }
  | {
      kind: "agent_tool";
      contribution_id: string;
      contract_version: number;
      tool_id: string;
    }
  | {
      kind: "artifact_type";
      contribution_id: string;
      contract_version: number;
      contribution: ArtifactTypeContributionV1;
    }
  | {
      kind: "generated_ui_runtime";
      contribution_id: string;
      contract_version: number;
      contribution: GeneratedUiRuntimeContributionV1;
    }
  | {
      kind: "generated_ui_sdk";
      contribution_id: string;
      contract_version: number;
      contribution: GeneratedUiSdkContributionV1;
    }
  | {
      kind: "generated_ui_action_bridge";
      contribution_id: string;
      contract_version: number;
      contribution: GeneratedUiActionBridgeContributionV1;
    };

export type PluginCompatibility = {
  state: PluginCompatibilityState;
  diagnostics: string[];
};

export type PluginPackageSummary = {
  plugin_id: string;
  version: string;
  manifest_hash: string;
  manifest_version: number;
  display_name: string;
  description: string;
  publisher: string;
  install_source: PluginInstallSource;
  trust_level: PluginTrustLevel;
  requested_permissions: string[];
  contributions: PluginContribution[];
  discovered_at: string;
};

export type PluginInstallationSummary = {
  package: PluginPackageSummary;
  desired_state: PluginDesiredState;
  effective_state: PluginEffectiveState;
  compatibility: PluginCompatibility;
  configuration_revision: number;
  granted_permissions: string[];
  installed_at: string;
  updated_at: string;
  last_error_code: string | null;
};

export type PluginListResponse = { items: PluginInstallationSummary[] };

export type ContributionTarget =
  | { kind: "ui_theme"; theme_id: string }
  | {
      kind: "visual_renderer";
      source_kind: string;
      surface: string;
      render_scope: VisualRenderScope;
      selector?: string | null;
    }
  | { kind: "artifact_type"; artifact_type: string }
  | { kind: "generated_ui_runtime"; runtime_id: string }
  | { kind: "generated_ui_sdk"; sdk_id: string }
  | { kind: "generated_ui_action_bridge"; bridge_id: string };

export type ContributionRef = {
  plugin_id: string;
  contribution_id: string;
};

export type EffectiveContribution = {
  plugin_id: string;
  plugin_version: string;
  contribution_id: string;
  extension_point: string;
  contract_version: number;
  target: ContributionTarget;
  effective_state: "available" | "disabled";
  contribution: PluginContribution;
};

export type ContributionTargetResolution = {
  target_id: string;
  extension_point: string;
  mode: "exclusive" | "ordered";
  target: ContributionTarget;
  revision: number;
  conflict: boolean;
  contributions: EffectiveContribution[];
};

export type UpdateContributionTargetPreferencesRequest = {
  expected_revision: number;
  ordered_contributions: ContributionRef[];
  disabled_contributions: ContributionRef[];
};

export type EffectivePluginSnapshot = {
  contributions: EffectiveContribution[];
  resolutions: ContributionTargetResolution[];
  generated_at: string;
};

export type PluginContractFixture = {
  plugins: PluginListResponse;
  effective_snapshot: EffectivePluginSnapshot;
};

export type ScopeRef =
  | { kind: "runtime"; runtime_session_id: string }
  | { kind: "runtime_attempt"; runtime_attempt_id: string }
  | { kind: "provider_interaction"; provider_interaction_id: string }
  | { kind: "session"; session_thread_id: string }
  | { kind: "node"; node_id: string }
  | { kind: "placement"; project_placement_id: string }
  | { kind: "task_run"; task_run_id: string }
  | { kind: "unknown"; scope: string };

export type ArtifactState = "active" | "stale" | "archived";

export type ArtifactSummary = {
  artifact_id: string;
  artifact_type: string;
  title: string;
  scope_ref: ScopeRef;
  owner_plugin_id: string;
  current_version: number;
  state: ArtifactState;
  created_by: ActorRef;
  created_at: string;
  updated_at: string;
};

export type ArtifactVersion = {
  artifact_id: string;
  version: number;
  schema_version: number;
  payload: unknown;
  fallback_text: string;
  source_version: string | null;
  source_refs: UpravaRef[];
  evidence_refs: UpravaRef[];
  cause_refs: UpravaRef[];
  trace_refs: UpravaRef[];
  provenance: unknown;
  created_at: string;
};

export type ArtifactDetail = {
  artifact: ArtifactSummary;
  version: ArtifactVersion;
};

export type ArtifactListResponse = { items: ArtifactSummary[] };

export type CreateArtifactRequest = {
  artifact_type: string;
  title: string;
  scope_ref: ScopeRef;
  schema_version: number;
  payload: unknown;
  fallback_text: string;
  source_version: string | null;
  source_refs: UpravaRef[];
  evidence_refs: UpravaRef[];
  cause_refs: UpravaRef[];
  trace_refs: UpravaRef[];
  provenance: unknown;
};

export type CreateArtifactVersionRequest = Omit<
  CreateArtifactRequest,
  "artifact_type" | "title" | "scope_ref"
> & { expected_current_version: number };

export type GeneratedUiActionDefinition = {
  action_id: string;
  kind: GeneratedUiActionKind;
  label: string;
  input_schema: unknown;
  required_capabilities: GeneratedUiCapability[];
  confirmation_required: boolean;
};

export type GeneratedUiArtifactPayload = {
  description: string | null;
  runtime_id: string;
  sdk_version: string;
  layout_intent: GeneratedUiLayoutIntent;
  source_blob_hash: string;
  data_model: unknown;
  actions: GeneratedUiActionDefinition[];
  granted_capabilities: GeneratedUiCapability[];
  fallback_snapshot: string | null;
};

export type GeneratedUiBuildDiagnostic = {
  severity: "error" | "warning" | "info";
  message: string;
  line: number | null;
  column: number | null;
};

export type GeneratedUiBuild = {
  build_id: string;
  artifact_id: string;
  artifact_version: number;
  state: "pending" | "ready" | "failed" | "fallback_only";
  runtime_id: string;
  runtime_version: string;
  sdk_version: string;
  source_blob_hash: string;
  bundle_blob_hash: string | null;
  dependency_lock: unknown;
  diagnostics: GeneratedUiBuildDiagnostic[];
  created_at: string;
  completed_at: string | null;
};

export type GeneratedUiState = {
  artifact_id: string;
  revision: number;
  values: unknown;
  updated_at: string;
};

export type GeneratedUiRuntimeDetail = {
  artifact: ArtifactDetail;
  build: GeneratedUiBuild;
  state: GeneratedUiState;
};

export type CreateDynamicUiProposalRequest = {
  title: string;
  description: string | null;
  scope_ref: ScopeRef;
  runtime_id: string;
  sdk_version: string;
  layout_intent: GeneratedUiLayoutIntent;
  source: string;
  data_model: unknown;
  actions: GeneratedUiActionDefinition[];
  requested_capabilities: GeneratedUiCapability[];
  fallback_markdown: string;
  fallback_snapshot: string | null;
  source_refs: UpravaRef[];
  evidence_refs: UpravaRef[];
  cause_refs: UpravaRef[];
  trace_refs: UpravaRef[];
};

export type UpdateGeneratedUiStateRequest = {
  expected_revision: number;
  values: unknown;
};

export type InvokeGeneratedUiActionRequest = {
  artifact_version: number;
  idempotency_key: string;
  input: unknown;
  confirmed: boolean;
};

export type GeneratedUiActionResult = {
  action_request_id: string;
  artifact_id: string;
  action_id: string;
  kind: GeneratedUiActionKind;
  result: unknown;
  state: GeneratedUiState | null;
  command_id: string | null;
  completed_at: string;
};
