export const queryKeys = {
  health: ["health"] as const,
  version: ["version"] as const,
  inventory: ["inventory"] as const,
  jobs: ["jobs"] as const,
  job: (jobId: string) => ["job", jobId] as const,
  jobRun: (jobRunId: string) => ["job-run", jobRunId] as const,
  taskRuns: (placementId: string) => ["task-runs", placementId] as const,
  taskRun: (taskRunId: string) => ["task-run", taskRunId] as const,
  providerQuota: (provider: string) => ["provider-quota", provider] as const,
  nodeEnrollments: ["node-enrollments"] as const,
  node: (nodeId: string) => ["node", nodeId] as const,
  placement: (placementId: string) => ["placement", placementId] as const,
  workspaceTree: (placementId: string, path: string) =>
    ["placement", placementId, "workspace-tree", path] as const,
  workspaceFile: (placementId: string, path: string) =>
    ["placement", placementId, "workspace-file", path] as const,
  workspaceCommandHistory: (placementId: string) =>
    ["placement", placementId, "workspace-command-history"] as const,
  workspaceCommandResource: (placementId: string, commandId: string) =>
    ["placement", placementId, "workspace-command", commandId] as const,
  workspaceDiff: (placementId: string) =>
    ["placement", placementId, "workspace-diff"] as const,
  workspaceReview: (placementId: string, scope: string, path: string | null) =>
    ["placement", placementId, "workspace-review", scope, path] as const,
  workspaceTerminals: (placementId: string) =>
    ["placement", placementId, "workspace-terminals"] as const,
  session: (sessionThreadId: string) => ["session", sessionThreadId] as const,
  sessionPolicyPreview: (placementId: string, executionProfile: string) =>
    ["session-policy-preview", placementId, executionProfile] as const,
  sessionEvidenceProjection: (sessionThreadId: string) =>
    ["session", sessionThreadId, "evidence-projection"] as const,
  sessionTrace: (sessionThreadId: string) =>
    ["session", sessionThreadId, "trace"] as const,
  eventLogRoot: (sessionThreadId: string) =>
    ["events", sessionThreadId] as const,
  eventLog: (sessionThreadId: string, kind: string) =>
    ["events", sessionThreadId, kind] as const,
  referenceResolution: (referenceKey: string) =>
    ["reference-resolution", referenceKey] as const,
  deduction: (deductionId: string) => ["deduction", deductionId] as const,
  agentProjection: (sessionThreadId: string) =>
    ["session", sessionThreadId, "agent-projection"] as const,
  toolDefinitions: ["tooling", "definitions"] as const,
  toolAvailability: (sessionThreadId: string) =>
    ["tooling", "availability", sessionThreadId] as const,
  observedCapabilities: (nodeId: string) =>
    ["tooling", "observed-capabilities", nodeId] as const,
  integrationConnections: ["tooling", "integrations"] as const,
  mcpDependencies: ["tooling", "dependencies"] as const,
  toolCalls: (scopeKey: string) => ["tooling", "calls", scopeKey] as const,
  toolCall: (toolCallId: string) => ["tooling", "calls", toolCallId] as const,
  plugins: ["plugins"] as const,
  pluginContributions: ["plugins", "contributions"] as const,
  artifacts: (scopeKey: string) => ["artifacts", scopeKey] as const,
  artifact: (artifactId: string, version?: number) =>
    ["artifact", artifactId, version ?? "current"] as const,
  generatedUiRuntime: (artifactId: string) =>
    ["artifact", artifactId, "generated-ui"] as const,
};
