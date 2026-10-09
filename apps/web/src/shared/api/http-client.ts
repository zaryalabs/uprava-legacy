import type {
  AcknowledgeWarningRequest,
  ArtifactDetail,
  ArtifactListResponse,
  ApiError,
  ApproveNodeEnrollmentResponse,
  ClientCreateNodeEnrollmentRequest,
  CommandAcceptedResponse,
  CommandState,
  CreatePlacementRequest,
  CreateArtifactRequest,
  CreateArtifactVersionRequest,
  CreateDynamicUiProposalRequest,
  CreateJobRequest,
  CreateSessionRequest,
  CreateDeductionRequest,
  DeductionAcceptedResponse,
  DeductionRecord,
  EventLogPage,
  GeneratedUiActionResult,
  GeneratedUiRuntimeDetail,
  GeneratedUiState,
  HealthResponse,
  NodeCredentialRotationResponse,
  NodeDeletionResponse,
  NodeEnrollmentRequestedResponse,
  NodeEnrollmentSummary,
  NodeRevocationResponse,
  PlacementDeletionResponse,
  PreviewSessionPolicyRequest,
  ResolveApprovalRequest,
  SendTurnRequest,
  CreateScheduledMessageRequest,
  ScheduledSessionMessage,
  SessionPolicyPreview,
  SubmitProviderInputRequest,
  JobDetail,
  JobRunSummary,
  JobSummary,
  IntegrationConnectRequest,
  IntegrationConnectResponse,
  IntegrationConnectionsResponse,
  IntegrationDisconnectResponse,
  InvokeGeneratedUiActionRequest,
  McpDependencyStatusesResponse,
  PluginInstallationSummary,
  PluginListResponse,
  EffectivePluginSnapshot,
  ContributionTargetResolution,
  UpdateContributionTargetPreferencesRequest,
  UpdateGeneratedUiStateRequest,
  ObservedCapabilitiesResponse,
  ProviderQuotaStatus,
  PersistDeductionResponse,
  ReferenceResolution,
  UpdateJobRequest,
  SessionDetail,
  SessionTraceProjection,
  ToolAvailabilityResponse,
  ToolCallDetail,
  ToolCallsResponse,
  ToolDefinition,
  ToolDefinitionsResponse,
  CreateTaskRunRequest,
  TaskRunDetail,
  TaskRunListResponse,
  UpravaRef,
  VersionResponse,
  WebAuthLoginRequest,
  WebAuthResponse,
  WebAuthSetupRequest,
  WebAuthStatusResponse,
  WarningAcknowledgementResponse,
  WorkspaceCommandHistoryResponse,
  WorkspaceCommandHistoryItem,
  WorkspaceCommandRunRequest,
  WorkspaceCommandRunResponse,
  WorkspaceDiffResponse,
  WorkspaceDiffScope,
  WorkspaceReviewProjection,
  WorkspaceFileContentResponse,
  WorkspaceFileWriteRequest,
  WorkspaceFileWriteResponse,
  WorkspaceTerminalListResponse,
  WorkspaceTerminalOpenRequest,
  WorkspaceTerminalOpenResponse,
  WorkspaceTreeResponse,
} from "../protocol/types";
import {
  commandAcceptedResponseSchema,
  artifactDetailSchema,
  artifactListResponseSchema,
  formatProtocolIssues,
  parseProtocolPayload,
  type ProtocolSchema,
  workspaceCommandHistoryItemSchema,
  workspaceCommandHistoryResponseSchema,
  workspaceCommandRunResponseSchema,
  workspaceDiffResponseSchema,
  workspaceReviewProjectionSchema,
  workspaceTerminalListResponseSchema,
  workspaceTerminalOpenResponseSchema,
  integrationConnectionsResponseSchema,
  integrationConnectResponseSchema,
  integrationDisconnectResponseSchema,
  mcpDependencyStatusesResponseSchema,
  pluginInstallationSummarySchema,
  pluginListResponseSchema,
  effectivePluginSnapshotSchema,
  contributionTargetResolutionSchema,
  generatedUiActionResultSchema,
  generatedUiRuntimeDetailSchema,
  generatedUiStateSchema,
  observedCapabilitiesResponseSchema,
  toolAvailabilityResponseSchema,
  toolCallDetailSchema,
  toolCallsResponseSchema,
  toolDefinitionSchema,
  toolDefinitionsResponseSchema,
  taskRunDetailSchema,
  taskRunListResponseSchema,
} from "../protocol/validators";
import { apiBase, apiWsBase } from "./config";
import { logClientEvent } from "../logging/client-logger";
import { readCookie } from "../auth/cookies";

export class UpravaApiError extends Error {
  constructor(
    readonly envelope: ApiError,
    readonly status?: number,
  ) {
    super(envelope.message);
  }
}

export function shouldRetryQuery(failureCount: number, error: unknown) {
  if (failureCount >= 2) return false;
  if (error instanceof UpravaApiError) {
    return (
      error.status !== undefined &&
      error.status >= 500 &&
      error.envelope.retryable
    );
  }
  return true;
}

export async function apiGet<T>(
  path: string,
  schema?: ProtocolSchema<T>,
): Promise<T> {
  return apiRequest<T>(path, { method: "GET" }, schema);
}

export async function apiPost<T>(
  path: string,
  body?: unknown,
  schema?: ProtocolSchema<T>,
): Promise<T> {
  return apiRequest<T>(
    path,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    },
    schema,
  );
}

export async function apiPatch<T>(
  path: string,
  body?: unknown,
  schema?: ProtocolSchema<T>,
): Promise<T> {
  return apiRequest<T>(
    path,
    {
      method: "PATCH",
      headers: { "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    },
    schema,
  );
}

export async function apiPut<T>(
  path: string,
  body?: unknown,
  schema?: ProtocolSchema<T>,
): Promise<T> {
  return apiRequest<T>(
    path,
    {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    },
    schema,
  );
}

export async function apiDelete<T>(
  path: string,
  schema?: ProtocolSchema<T>,
): Promise<T> {
  return apiRequest<T>(path, { method: "DELETE" }, schema);
}

export async function apiGetText(path: string): Promise<string> {
  const response = await fetch(`${apiBase}${path}`, {
    method: "GET",
    credentials: "include",
  });
  if (!response.ok) {
    throw new UpravaApiError(
      {
        error_code: "network.http",
        message: `HTTP ${response.status}`,
        retryable: response.status >= 500,
        correlation_id: "unavailable",
      },
      response.status,
    );
  }
  return response.text();
}

const workspaceCommandTerminalStates = new Set<CommandState>([
  "completed",
  "failed",
  "blocked",
  "expired",
]);

async function apiRequest<T>(
  path: string,
  init: RequestInit,
  schema?: ProtocolSchema<T>,
): Promise<T> {
  const headers = new Headers(init.headers);
  const method = init.method?.toUpperCase() ?? "GET";
  const csrf = readCookie("uprava_csrf");
  if (csrf && !["GET", "HEAD", "OPTIONS"].includes(method)) {
    headers.set("x-uprava-csrf", csrf);
  }
  const response = await fetch(`${apiBase}${path}`, {
    ...init,
    headers,
    credentials: "include",
  });
  if (!response.ok) {
    const fallback: ApiError = {
      error_code: "network.http",
      message: `HTTP ${response.status}`,
      retryable: response.status >= 500,
      correlation_id: "unavailable",
    };
    const envelope = await response.json().catch(() => fallback);
    logClientEvent("warn", "web.api", envelope.message, {
      path,
      status: response.status,
      error_code: envelope.error_code,
      correlation_id: envelope.correlation_id,
    });
    throw new UpravaApiError(envelope, response.status);
  }
  const payload = (await response.json()) as unknown;
  if (!schema) {
    return payload as T;
  }
  const parsed = schema.safeParse(payload);
  if (parsed.success) {
    return parsed.data;
  }
  const detail = formatProtocolIssues(parsed.error.issues);
  logClientEvent("error", "web.protocol", "Core response validation failed", {
    path,
    status: response.status,
    detail,
  });
  throw new UpravaApiError({
    error_code: "web.protocol_validation_failed",
    message: `Core response did not match protocol v2 contract: ${detail}`,
    retryable: false,
    correlation_id: "client",
  });
}

async function sleep(delayMs: number) {
  await new Promise((resolve) => window.setTimeout(resolve, delayMs));
}

function commandResourceError(
  code: string,
  message: string,
  retryable = false,
) {
  return new UpravaApiError({
    error_code: code,
    message,
    retryable,
    correlation_id: "client",
  });
}

async function pollWorkspaceCommandRunResponse(
  placementId: string,
  commandId: string,
  timeoutSeconds: number | null,
) {
  const timeoutMs = Math.max((timeoutSeconds ?? 120) * 1000 + 10_000, 15_000);
  const deadline = Date.now() + timeoutMs;

  while (Date.now() < deadline) {
    const resource = await apiGet<WorkspaceCommandHistoryItem>(
      `/placements/${encodeURIComponent(placementId)}/workspace/commands/async/${encodeURIComponent(commandId)}`,
      workspaceCommandHistoryItemSchema,
    );
    if (!workspaceCommandTerminalStates.has(resource.state)) {
      await sleep(250);
      continue;
    }
    if (resource.state !== "completed") {
      throw commandResourceError(
        "workspace.command_failed",
        `Workspace command finished with state ${resource.state}`,
      );
    }
    const result = parseProtocolPayload(
      workspaceCommandRunResponseSchema,
      resource.result_payload,
    );
    if (result) {
      return result;
    }
    throw commandResourceError(
      "workspace.command_result_invalid",
      "Workspace command completed without a valid result payload",
    );
  }

  throw commandResourceError(
    "workspace.command_poll_timeout",
    "Timed out waiting for the workspace command result",
    true,
  );
}

export const coreApi = {
  health: () => apiGet<HealthResponse>("/health"),
  version: () => apiGet<VersionResponse>("/version"),
  authStatus: () => apiGet<WebAuthStatusResponse>("/auth/status"),
  authSetup: (request: WebAuthSetupRequest) =>
    apiPost<WebAuthResponse>("/auth/setup", request),
  authLogin: (request: WebAuthLoginRequest) =>
    apiPost<WebAuthResponse>("/auth/login", request),
  authLogout: () => apiPost<WebAuthResponse>("/auth/logout"),
  inventory: () =>
    apiGet<import("../protocol/types").InventorySnapshot>("/inventory"),
  toolDefinitions: () =>
    apiGet<ToolDefinitionsResponse>(
      "/tool-definitions",
      toolDefinitionsResponseSchema,
    ),
  toolDefinition: (toolId: string) =>
    apiGet<ToolDefinition>(
      `/tool-definitions/${encodeURIComponent(toolId)}`,
      toolDefinitionSchema,
    ),
  toolAvailability: (scope: {
    nodeId: string;
    projectId: string | null;
    placementId: string;
    sessionThreadId: string;
  }) => {
    const query = new URLSearchParams({
      node_id: scope.nodeId,
      project_placement_id: scope.placementId,
      session_thread_id: scope.sessionThreadId,
    });
    if (scope.projectId) query.set("project_id", scope.projectId);
    return apiGet<ToolAvailabilityResponse>(
      `/tool-availability?${query.toString()}`,
      toolAvailabilityResponseSchema,
    );
  },
  observedCapabilities: (nodeId: string) =>
    apiGet<ObservedCapabilitiesResponse>(
      `/nodes/${encodeURIComponent(nodeId)}/observed-capabilities`,
      observedCapabilitiesResponseSchema,
    ),
  integrationConnections: () =>
    apiGet<IntegrationConnectionsResponse>(
      "/integrations",
      integrationConnectionsResponseSchema,
    ),
  connectIntegration: (request: IntegrationConnectRequest) =>
    apiPost<IntegrationConnectResponse>(
      "/integrations",
      request,
      integrationConnectResponseSchema,
    ),
  disconnectIntegration: (integrationId: string) =>
    apiPost<IntegrationDisconnectResponse>(
      `/integrations/${encodeURIComponent(integrationId)}/disconnect`,
      { revoke_remote: true },
      integrationDisconnectResponseSchema,
    ),
  mcpDependencies: () =>
    apiGet<McpDependencyStatusesResponse>(
      "/mcp-dependencies",
      mcpDependencyStatusesResponseSchema,
    ),
  toolCalls: (scope: {
    nodeId?: string;
    projectId?: string | null;
    placementId?: string;
    sessionThreadId?: string;
    limit?: number;
  }) => {
    const query = new URLSearchParams();
    if (scope.nodeId) query.set("node_id", scope.nodeId);
    if (scope.projectId) query.set("project_id", scope.projectId);
    if (scope.placementId) query.set("project_placement_id", scope.placementId);
    if (scope.sessionThreadId) {
      query.set("session_thread_id", scope.sessionThreadId);
    }
    query.set("limit", String(scope.limit ?? 50));
    return apiGet<ToolCallsResponse>(
      `/tool-calls?${query.toString()}`,
      toolCallsResponseSchema,
    );
  },
  toolCall: (toolCallId: string) =>
    apiGet<ToolCallDetail>(
      `/tool-calls/${encodeURIComponent(toolCallId)}`,
      toolCallDetailSchema,
    ),
  plugins: () =>
    apiGet<PluginListResponse>("/plugins", pluginListResponseSchema),
  pluginContributions: () =>
    apiGet<EffectivePluginSnapshot>(
      "/plugin-contributions",
      effectivePluginSnapshotSchema,
    ),
  artifacts: (
    scope: {
      sessionThreadId?: string;
      placementId?: string;
      artifactType?: string;
    } = {},
  ) => {
    const query = new URLSearchParams();
    if (scope.sessionThreadId) {
      query.set("session_thread_id", scope.sessionThreadId);
    }
    if (scope.placementId) {
      query.set("project_placement_id", scope.placementId);
    }
    if (scope.artifactType) query.set("artifact_type", scope.artifactType);
    const suffix = query.size > 0 ? `?${query.toString()}` : "";
    return apiGet<ArtifactListResponse>(
      `/artifacts${suffix}`,
      artifactListResponseSchema,
    );
  },
  artifact: (artifactId: string, version?: number) =>
    apiGet<ArtifactDetail>(
      `/artifacts/${encodeURIComponent(artifactId)}${version === undefined ? "" : `?version=${version}`}`,
      artifactDetailSchema,
    ),
  createArtifact: (request: CreateArtifactRequest) =>
    apiPost<ArtifactDetail>("/artifacts", request, artifactDetailSchema),
  createArtifactVersion: (
    artifactId: string,
    request: CreateArtifactVersionRequest,
  ) =>
    apiPost<ArtifactDetail>(
      `/artifacts/${encodeURIComponent(artifactId)}/versions`,
      request,
      artifactDetailSchema,
    ),
  createDynamicUiProposal: (request: CreateDynamicUiProposalRequest) =>
    apiPost<GeneratedUiRuntimeDetail>(
      "/dynamic-ui/proposals",
      request,
      generatedUiRuntimeDetailSchema,
    ),
  generatedUiRuntime: (artifactId: string) =>
    apiGet<GeneratedUiRuntimeDetail>(
      `/artifacts/${encodeURIComponent(artifactId)}/dynamic-ui`,
      generatedUiRuntimeDetailSchema,
    ),
  generatedUiSource: (artifactId: string) =>
    apiGetText(
      `/artifacts/${encodeURIComponent(artifactId)}/dynamic-ui/source`,
    ),
  generatedUiBundle: (blobHash: string) =>
    apiGetText(`/generated-ui/bundles/${encodeURIComponent(blobHash)}`),
  updateGeneratedUiState: (
    artifactId: string,
    request: UpdateGeneratedUiStateRequest,
  ) =>
    apiPut<GeneratedUiState>(
      `/artifacts/${encodeURIComponent(artifactId)}/dynamic-ui/state`,
      request,
      generatedUiStateSchema,
    ),
  invokeGeneratedUiAction: (
    artifactId: string,
    actionId: string,
    request: InvokeGeneratedUiActionRequest,
  ) =>
    apiPost<GeneratedUiActionResult>(
      `/artifacts/${encodeURIComponent(artifactId)}/dynamic-ui/actions/${encodeURIComponent(actionId)}`,
      request,
      generatedUiActionResultSchema,
    ),
  updatePluginContributionTarget: (
    targetId: string,
    request: UpdateContributionTargetPreferencesRequest,
  ) =>
    apiPut<ContributionTargetResolution>(
      `/plugin-contribution-targets/${encodeURIComponent(targetId)}`,
      request,
      contributionTargetResolutionSchema,
    ),
  enablePlugin: (pluginId: string) =>
    apiPost<PluginInstallationSummary>(
      `/plugins/${encodeURIComponent(pluginId)}/enable`,
      undefined,
      pluginInstallationSummarySchema,
    ),
  disablePlugin: (pluginId: string) =>
    apiPost<PluginInstallationSummary>(
      `/plugins/${encodeURIComponent(pluginId)}/disable`,
      undefined,
      pluginInstallationSummarySchema,
    ),
  jobs: () => apiGet<JobSummary[]>("/jobs"),
  createJob: (request: CreateJobRequest) =>
    apiPost<JobDetail>("/jobs", request),
  job: (jobId: string) =>
    apiGet<JobDetail>(`/jobs/${encodeURIComponent(jobId)}`),
  updateJob: (jobId: string, request: UpdateJobRequest) =>
    apiPatch<JobDetail>(`/jobs/${encodeURIComponent(jobId)}`, request),
  enableJob: (jobId: string) =>
    apiPost<JobDetail>(`/jobs/${encodeURIComponent(jobId)}/enable`),
  disableJob: (jobId: string) =>
    apiPost<JobDetail>(`/jobs/${encodeURIComponent(jobId)}/disable`),
  runJob: (jobId: string, force = false) =>
    apiPost<JobRunSummary>(`/jobs/${encodeURIComponent(jobId)}/runs`, {
      force,
    }),
  jobRun: (jobRunId: string) =>
    apiGet<JobRunSummary>(`/job-runs/${encodeURIComponent(jobRunId)}`),
  cancelJobRun: (jobRunId: string) =>
    apiPost<JobRunSummary>(`/job-runs/${encodeURIComponent(jobRunId)}/cancel`),
  taskRuns: (placementId?: string) => {
    const query = placementId
      ? `?project_placement_id=${encodeURIComponent(placementId)}`
      : "";
    return apiGet<TaskRunListResponse>(
      `/task-runs${query}`,
      taskRunListResponseSchema,
    );
  },
  createTaskRun: (request: CreateTaskRunRequest) =>
    apiPost<TaskRunDetail>("/task-runs", request, taskRunDetailSchema),
  taskRun: (taskRunId: string) =>
    apiGet<TaskRunDetail>(
      `/task-runs/${encodeURIComponent(taskRunId)}`,
      taskRunDetailSchema,
    ),
  cancelTaskRun: (taskRunId: string) =>
    apiPost<TaskRunDetail>(
      `/task-runs/${encodeURIComponent(taskRunId)}/cancel`,
      undefined,
      taskRunDetailSchema,
    ),
  providerQuota: (provider: string) =>
    apiGet<ProviderQuotaStatus>(
      `/provider-quota/${encodeURIComponent(provider)}`,
    ),
  node: (nodeId: string) =>
    apiGet<import("../protocol/types").NodeSummary>(
      `/nodes/${encodeURIComponent(nodeId)}`,
    ),
  nodeEnrollments: () => apiGet<NodeEnrollmentSummary[]>("/node-enrollments"),
  createNodeEnrollment: (request: ClientCreateNodeEnrollmentRequest) =>
    apiPost<NodeEnrollmentRequestedResponse>("/node-enrollments", request),
  approveNodeEnrollment: (enrollmentId: string) =>
    apiPost<ApproveNodeEnrollmentResponse>(
      `/node-enrollments/${encodeURIComponent(enrollmentId)}/approve`,
    ),
  revokeNode: (nodeId: string) =>
    apiPost<NodeRevocationResponse>(
      `/nodes/${encodeURIComponent(nodeId)}/revoke`,
    ),
  rotateNodeCredential: (nodeId: string) =>
    apiPost<NodeCredentialRotationResponse>(
      `/nodes/${encodeURIComponent(nodeId)}/rotate-credential`,
    ),
  deleteNode: (nodeId: string) =>
    apiDelete<NodeDeletionResponse>(`/nodes/${encodeURIComponent(nodeId)}`),
  placement: (placementId: string) =>
    apiGet<import("../protocol/types").ProjectPlacementSummary>(
      `/placements/${encodeURIComponent(placementId)}`,
    ),
  deletePlacement: (placementId: string) =>
    apiDelete<PlacementDeletionResponse>(
      `/placements/${encodeURIComponent(placementId)}`,
    ),
  refreshResourceSnapshot: (placementId: string) =>
    apiPost<CommandAcceptedResponse>(
      `/placements/${encodeURIComponent(
        placementId,
      )}/resource-snapshot/refresh`,
      undefined,
      commandAcceptedResponseSchema,
    ),
  workspaceTree: (placementId: string, path = ".") =>
    apiGet<WorkspaceTreeResponse>(
      `/placements/${encodeURIComponent(
        placementId,
      )}/workspace/tree?path=${encodeURIComponent(path)}`,
    ),
  workspaceFile: (placementId: string, path: string) =>
    apiGet<WorkspaceFileContentResponse>(
      `/placements/${encodeURIComponent(
        placementId,
      )}/workspace/file?path=${encodeURIComponent(path)}`,
    ),
  writeWorkspaceFile: (
    placementId: string,
    request: WorkspaceFileWriteRequest,
  ) =>
    apiPost<WorkspaceFileWriteResponse>(
      `/placements/${encodeURIComponent(placementId)}/workspace/file`,
      request,
    ),
  runWorkspaceCommand: (
    placementId: string,
    request: WorkspaceCommandRunRequest,
  ) =>
    apiPost<CommandAcceptedResponse>(
      `/placements/${encodeURIComponent(placementId)}/workspace/commands/async`,
      request,
      commandAcceptedResponseSchema,
    ).then((accepted) =>
      pollWorkspaceCommandRunResponse(
        placementId,
        accepted.command_id,
        request.timeout_seconds,
      ),
    ),
  runWorkspaceCommandAsync: (
    placementId: string,
    request: WorkspaceCommandRunRequest,
  ) =>
    apiPost<CommandAcceptedResponse>(
      `/placements/${encodeURIComponent(placementId)}/workspace/commands/async`,
      request,
      commandAcceptedResponseSchema,
    ),
  workspaceCommandResource: (placementId: string, commandId: string) =>
    apiGet<WorkspaceCommandHistoryItem>(
      `/placements/${encodeURIComponent(placementId)}/workspace/commands/async/${encodeURIComponent(commandId)}`,
      workspaceCommandHistoryItemSchema,
    ),
  cancelWorkspaceCommand: (placementId: string, commandId: string) =>
    apiDelete<WorkspaceCommandHistoryItem>(
      `/placements/${encodeURIComponent(placementId)}/workspace/commands/async/${encodeURIComponent(commandId)}`,
      workspaceCommandHistoryItemSchema,
    ),
  workspaceCommandHistory: (placementId: string, limit = 20) =>
    apiGet<WorkspaceCommandHistoryResponse>(
      `/placements/${encodeURIComponent(
        placementId,
      )}/workspace/commands?limit=${encodeURIComponent(String(limit))}`,
      workspaceCommandHistoryResponseSchema,
    ),
  workspaceDiff: (placementId: string) =>
    apiGet<WorkspaceDiffResponse>(
      `/placements/${encodeURIComponent(placementId)}/workspace/diff`,
      workspaceDiffResponseSchema,
    ),
  workspaceReview: (
    placementId: string,
    scope: WorkspaceDiffScope,
    path: string | null,
  ) => {
    const query = new URLSearchParams({ scope });
    if (path) query.set("path", path);
    return apiGet<WorkspaceReviewProjection>(
      `/placements/${encodeURIComponent(placementId)}/workspace/review?${query.toString()}`,
      workspaceReviewProjectionSchema,
    );
  },
  workspaceTerminals: (placementId: string) =>
    apiGet<WorkspaceTerminalListResponse>(
      `/placements/${encodeURIComponent(placementId)}/workspace/terminals`,
      workspaceTerminalListResponseSchema,
    ),
  openWorkspaceTerminal: (
    placementId: string,
    request: WorkspaceTerminalOpenRequest,
  ) =>
    apiPost<WorkspaceTerminalOpenResponse>(
      `/placements/${encodeURIComponent(placementId)}/workspace/terminals`,
      request,
      workspaceTerminalOpenResponseSchema,
    ),
  workspaceTerminalStreamUrl: (placementId: string, terminalId: string) =>
    `${apiWsBase}/placements/${encodeURIComponent(
      placementId,
    )}/workspace/terminals/${encodeURIComponent(terminalId)}/stream`,
  validatePlacement: (request: CreatePlacementRequest) =>
    apiPost<import("../protocol/types").ProjectPlacementSummary>(
      "/project-placements/validate",
      request,
    ),
  createSession: (request: CreateSessionRequest) =>
    apiPost<import("../protocol/types").SessionDetail>("/sessions", request),
  previewSessionPolicy: (request: PreviewSessionPolicyRequest) =>
    apiPost<SessionPolicyPreview>("/sessions/policy-preview", request),
  session: (sessionThreadId: string) =>
    apiGet<import("../protocol/types").SessionDetail>(
      `/sessions/${encodeURIComponent(sessionThreadId)}`,
    ),
  attachSession: (sessionThreadId: string) =>
    apiPost<SessionDetail>(
      `/sessions/${encodeURIComponent(sessionThreadId)}/attach`,
    ),
  detachSession: (sessionThreadId: string) =>
    apiPost<SessionDetail>(
      `/sessions/${encodeURIComponent(sessionThreadId)}/detach`,
    ),
  sessionEvidenceProjection: (sessionThreadId: string) =>
    apiGet<import("../protocol/types").SessionEvidenceProjection>(
      `/sessions/${encodeURIComponent(sessionThreadId)}/evidence-projection`,
    ),
  sessionTrace: (sessionThreadId: string) =>
    apiGet<SessionTraceProjection>(
      `/sessions/${encodeURIComponent(sessionThreadId)}/trace`,
    ),
  events: (
    filters: {
      sessionThreadId?: string;
      placementId?: string;
      kind?: string;
      cursor?: string;
      limit?: number;
    } = {},
  ) => {
    const params = new URLSearchParams();
    if (filters.sessionThreadId) {
      params.set("session_thread_id", filters.sessionThreadId);
    }
    if (filters.placementId) params.set("placement_id", filters.placementId);
    if (filters.kind) params.set("kind", filters.kind);
    if (filters.cursor) params.set("cursor", filters.cursor);
    if (filters.limit !== undefined) params.set("limit", String(filters.limit));
    const query = params.size > 0 ? `?${params.toString()}` : "";
    return apiGet<EventLogPage>(`/events${query}`);
  },
  event: (eventId: string) =>
    apiGet<import("../protocol/types").EventEnvelope>(
      `/events/${encodeURIComponent(eventId)}`,
    ),
  resolveReference: (reference: UpravaRef) =>
    apiPost<ReferenceResolution>("/references/resolve", { reference }),
  createDeduction: (sessionThreadId: string, request: CreateDeductionRequest) =>
    apiPost<DeductionAcceptedResponse>(
      `/sessions/${encodeURIComponent(sessionThreadId)}/deductions`,
      request,
    ),
  deduction: (deductionId: string) =>
    apiGet<DeductionRecord>(`/deductions/${encodeURIComponent(deductionId)}`),
  cancelDeduction: (deductionId: string) =>
    apiPost<DeductionAcceptedResponse>(
      `/deductions/${encodeURIComponent(deductionId)}/cancel`,
    ),
  persistDeduction: (deductionId: string) =>
    apiPost<PersistDeductionResponse>(
      `/deductions/${encodeURIComponent(deductionId)}/persist`,
    ),
  agentProjection: (sessionThreadId: string) =>
    apiGet<import("../protocol/types").AgentProjection>(
      `/sessions/${encodeURIComponent(sessionThreadId)}/agent-projection`,
    ),
  sendTurn: (sessionThreadId: string, request: SendTurnRequest) =>
    apiPost<CommandAcceptedResponse>(
      `/sessions/${encodeURIComponent(sessionThreadId)}/turns`,
      request,
      commandAcceptedResponseSchema,
    ),
  createScheduledMessage: (
    sessionThreadId: string,
    request: CreateScheduledMessageRequest,
  ) =>
    apiPost<ScheduledSessionMessage>(
      `/sessions/${encodeURIComponent(sessionThreadId)}/scheduled-messages`,
      request,
    ),
  updateScheduledMessage: (
    sessionThreadId: string,
    scheduledMessageId: string,
    request: import("../protocol/types").UpdateScheduledMessageRequest,
  ) =>
    apiPatch<ScheduledSessionMessage>(
      `/sessions/${encodeURIComponent(sessionThreadId)}/scheduled-messages/${encodeURIComponent(scheduledMessageId)}`,
      request,
    ),
  cancelScheduledMessage: (
    sessionThreadId: string,
    scheduledMessageId: string,
  ) =>
    apiDelete<ScheduledSessionMessage>(
      `/sessions/${encodeURIComponent(sessionThreadId)}/scheduled-messages/${encodeURIComponent(scheduledMessageId)}`,
    ),
  sendScheduledMessageNow: (
    sessionThreadId: string,
    scheduledMessageId: string,
  ) =>
    apiPost<ScheduledSessionMessage>(
      `/sessions/${encodeURIComponent(sessionThreadId)}/scheduled-messages/${encodeURIComponent(scheduledMessageId)}/send-now`,
    ),
  retryScheduledMessage: (
    sessionThreadId: string,
    scheduledMessageId: string,
  ) =>
    apiPost<ScheduledSessionMessage>(
      `/sessions/${encodeURIComponent(sessionThreadId)}/scheduled-messages/${encodeURIComponent(scheduledMessageId)}/retry`,
    ),
  resolveApproval: (
    sessionThreadId: string,
    approvalId: string,
    request: ResolveApprovalRequest,
  ) =>
    apiPost<CommandAcceptedResponse>(
      `/sessions/${encodeURIComponent(
        sessionThreadId,
      )}/approvals/${encodeURIComponent(approvalId)}/resolve`,
      request,
      commandAcceptedResponseSchema,
    ),
  resolveProviderApproval: (
    sessionThreadId: string,
    providerInteractionId: string,
    request: ResolveApprovalRequest,
  ) =>
    apiPost<CommandAcceptedResponse>(
      `/sessions/${encodeURIComponent(
        sessionThreadId,
      )}/provider-interactions/${encodeURIComponent(
        providerInteractionId,
      )}/approval`,
      request,
      commandAcceptedResponseSchema,
    ),
  submitProviderInput: (
    sessionThreadId: string,
    providerInteractionId: string,
    request: SubmitProviderInputRequest,
  ) =>
    apiPost<CommandAcceptedResponse>(
      `/sessions/${encodeURIComponent(
        sessionThreadId,
      )}/provider-interactions/${encodeURIComponent(providerInteractionId)}/input`,
      request,
      commandAcceptedResponseSchema,
    ),
  acknowledgeWarning: (
    sessionThreadId: string,
    warningKind: string,
    request: AcknowledgeWarningRequest,
  ) =>
    apiPost<WarningAcknowledgementResponse>(
      `/sessions/${encodeURIComponent(
        sessionThreadId,
      )}/warnings/${encodeURIComponent(warningKind)}/acknowledge`,
      request,
    ),
  interruptRuntime: (runtimeSessionId: string) =>
    apiPost<CommandAcceptedResponse>(
      `/runtime-sessions/${encodeURIComponent(runtimeSessionId)}/interrupt`,
      undefined,
      commandAcceptedResponseSchema,
    ),
  stopRuntime: (runtimeSessionId: string) =>
    apiPost<CommandAcceptedResponse>(
      `/runtime-sessions/${encodeURIComponent(runtimeSessionId)}/stop`,
      undefined,
      commandAcceptedResponseSchema,
    ),
  resumeRuntime: (runtimeSessionId: string) =>
    apiPost<CommandAcceptedResponse>(
      `/runtime-sessions/${encodeURIComponent(runtimeSessionId)}/resume`,
      undefined,
      commandAcceptedResponseSchema,
    ),
};
