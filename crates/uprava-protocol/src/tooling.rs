//! Shared Agent Tooling and Tool Registry contracts.
//!
//! These types are transport-neutral. Core remains the authority for meaning,
//! policy and trace; MCP and the Web API are projections of the same contract.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use super::*;

/// First version of the Core-to-Node tooling payload contract.
pub const TOOLING_CONTRACT_VERSION_V1: u16 = 1;
/// Stable MCP revision selected for the 0.2.11 implementation slice.
pub const TOOLING_MCP_REVISION: &str = "2025-11-25";
/// Default number of results returned by `search_tools`.
pub const TOOL_SEARCH_DEFAULT_LIMIT: u16 = 10;
/// Hard upper bound for one `search_tools` page.
pub const TOOL_SEARCH_MAX_LIMIT: u16 = 25;
/// Hard upper bound for one external tool result before artifact fallback.
pub const TOOL_RESULT_MAX_BYTES: u64 = 1_048_576;
/// Audience required on session-scoped Uprava MCP access leases.
pub const UPRAVA_MCP_LEASE_AUDIENCE: &str = "uprava:mcp";
/// Version of the private host Node to Compose ToolHive bridge contract.
pub const TOOLHIVE_BRIDGE_CONTRACT_VERSION_V1: u16 = 1;

/// Bounded request for one ToolHive-managed Linear workload operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolhiveBridgeWorkloadRequest {
    pub contract_version: u16,
    pub upstream_url: String,
    pub workload_name: String,
    pub proxy_port: u16,
    pub callback_port: u16,
}

/// Result of starting an interactive ToolHive authorization flow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolhiveBridgeAuthorizationResponse {
    pub authorization_url: String,
    pub expires_at: DateTime<Utc>,
}

/// Safe ToolHive runtime identity returned to the host Node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolhiveBridgeVersionResponse {
    pub version: String,
}

/// Safe workload presence projection returned by the ToolHive bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolhiveBridgeWorkloadStatusResponse {
    pub running: bool,
}

/// One bounded MCP operation executed through the private ToolHive bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolhiveBridgeMcpRequest {
    pub contract_version: u16,
    pub proxy_port: u16,
    pub method: String,
    pub params: JsonValue,
}

/// Raw MCP result returned after the bridge completes a local session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolhiveBridgeMcpResponse {
    pub result: JsonValue,
}

/// Node request for ephemeral provider access to the Uprava MCP endpoint.
///
/// The durable command contains only this opaque command reference. Core
/// resolves the authoritative session, Node and provider before issuing a
/// short-lived credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderMcpAccessRequest {
    pub command_id: CommandId,
}

/// Short-lived MCP credential returned only across the authenticated Node
/// transport boundary.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderMcpAccess {
    pub endpoint_url: String,
    pub access_token: McpAccessToken,
    pub expires_at: DateTime<Utc>,
}

/// Bearer credential with a redacted debug representation.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct McpAccessToken(String);

impl McpAccessToken {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for McpAccessToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("McpAccessToken([REDACTED])")
    }
}

impl std::fmt::Debug for ProviderMcpAccess {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderMcpAccess")
            .field("endpoint_url", &self.endpoint_url)
            .field("access_token", &self.access_token)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSourceKind {
    UpravaNative,
    ExternalMcp,
    Plugin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolExecutionKind {
    CoreNative,
    NodeNative,
    ToolhiveMcp,
    ExternalProvider,
    Hybrid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRiskLevel {
    ReadOnly,
    WorkspaceWrite,
    ExternalRead,
    ExternalWrite,
    CredentialedAction,
    Destructive,
    PrivilegedLocal,
    NetworkBroad,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolDefinitionState {
    Active,
    Deprecated,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolRedactionPolicy {
    #[serde(default)]
    pub argument_json_pointers: Vec<String>,
    #[serde(default)]
    pub result_json_pointers: Vec<String>,
    pub redact_all_arguments: bool,
    pub redact_all_result: bool,
    pub max_summary_bytes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub tool_id: ToolId,
    pub source_id: ToolSourceId,
    pub source_kind: ToolSourceKind,
    pub source_tool_name: String,
    pub version: u64,
    pub display_name: String,
    pub short_description: String,
    pub documentation_url: Option<String>,
    pub input_schema: JsonValue,
    pub output_schema: Option<JsonValue>,
    pub schema_hash: String,
    pub risk_level: ToolRiskLevel,
    #[serde(default)]
    pub required_permissions: Vec<String>,
    pub execution_kind: ToolExecutionKind,
    pub approval_policy: PolicyDecision,
    pub redaction: ToolRedactionPolicy,
    pub state: ToolDefinitionState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolScope {
    pub actor_ref: ActorRef,
    pub node_id: Option<NodeId>,
    pub project_id: Option<ProjectId>,
    pub project_placement_id: Option<ProjectPlacementId>,
    pub session_thread_id: Option<SessionThreadId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolAvailabilityState {
    Available,
    Unavailable,
    Degraded,
    ApprovalRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolUnavailableReason {
    NodeOffline,
    CapabilityMissing,
    DependencyMissing,
    DependencyUnhealthy,
    NotAuthenticated,
    PermissionDenied,
    PolicyBlocked,
    ProjectNotEnabled,
    SessionNotEnabled,
    SchemaChanged,
    BackendUnreachable,
    ToolhiveMissing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolAvailability {
    pub tool_id: ToolId,
    pub scope: ToolScope,
    pub state: ToolAvailabilityState,
    pub reason: Option<ToolUnavailableReason>,
    pub backend_ref: Option<String>,
    pub dependency_instance_id: Option<McpDependencyInstanceId>,
    pub schema_hash: String,
    pub policy_version: String,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedCapabilityState {
    Available,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedCapability {
    pub node_id: NodeId,
    pub capability_key: String,
    pub display_name: String,
    pub state: ObservedCapabilityState,
    pub version: Option<String>,
    pub safe_authentication_state: Option<String>,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationDesiredState {
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationAuthState {
    Disconnected,
    Connecting,
    Connected,
    Expired,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrationConnectionSummary {
    pub integration_id: IntegrationId,
    pub source_id: ToolSourceId,
    pub provider: String,
    pub display_name: String,
    pub desired_state: IntegrationDesiredState,
    pub auth_state: IntegrationAuthState,
    pub node_id: Option<NodeId>,
    pub authenticated_actor_label: Option<String>,
    pub connected_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpDependencyActualState {
    ToolhiveMissing,
    MissingAuth,
    Installing,
    Starting,
    Running,
    Degraded,
    Failed,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpDependencyStatus {
    pub dependency_instance_id: McpDependencyInstanceId,
    pub integration_id: IntegrationId,
    pub node_id: NodeId,
    pub desired_state: IntegrationDesiredState,
    pub actual_state: McpDependencyActualState,
    pub runtime_name: String,
    pub runtime_version: Option<String>,
    pub upstream_identity: Option<String>,
    pub schema_set_hash: Option<String>,
    pub error_code: Option<String>,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDecision {
    Allow,
    Deny,
    RequireApproval,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallState {
    Requested,
    Authorized,
    ApprovalRequired,
    Started,
    Completed,
    Failed,
    Denied,
    Cancelled,
    TimedOut,
}

impl ToolCallState {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Denied | Self::Cancelled | Self::TimedOut
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallSummary {
    pub tool_call_id: ToolCallId,
    pub tool_id: ToolId,
    pub schema_hash: String,
    pub actor_ref: ActorRef,
    pub scope: ToolScope,
    pub source_kind: ToolSourceKind,
    pub state: ToolCallState,
    pub policy_decision: PolicyDecision,
    pub route: String,
    pub requested_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub correlation_id: CorrelationId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallDetail {
    pub summary: ToolCallSummary,
    pub command_id: Option<CommandId>,
    pub integration_id: Option<IntegrationId>,
    pub dependency_instance_id: Option<McpDependencyInstanceId>,
    pub policy_version: String,
    pub redacted_arguments_summary: Option<String>,
    pub redacted_result_summary: Option<String>,
    pub argument_hash: Option<String>,
    pub result_hash: Option<String>,
    pub result_size_bytes: Option<u64>,
    #[serde(default)]
    pub trace_refs: Vec<UpravaRef>,
    #[serde(default)]
    pub result_refs: Vec<UpravaRef>,
    pub error: Option<ToolExecutionError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ToolSearchFilters {
    pub source_kinds: Vec<ToolSourceKind>,
    pub risk_levels: Vec<ToolRiskLevel>,
    pub availability_states: Vec<ToolAvailabilityState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchToolsRequest {
    pub scope: ToolScope,
    pub query: String,
    #[serde(default)]
    pub filters: ToolSearchFilters,
    pub cursor: Option<String>,
    pub limit: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSearchResult {
    pub tool_id: ToolId,
    pub display_name: String,
    pub short_description: String,
    pub source_kind: ToolSourceKind,
    pub risk_level: ToolRiskLevel,
    pub availability_state: ToolAvailabilityState,
    pub unavailable_reason: Option<ToolUnavailableReason>,
    pub schema_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchToolsResponse {
    pub items: Vec<ToolSearchResult>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectToolRequest {
    pub scope: ToolScope,
    pub tool_id: ToolId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolInvocationMode {
    StableExecuteTool,
    DynamicMountOptional,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectToolResponse {
    pub definition: ToolDefinition,
    pub availability: ToolAvailability,
    pub invocation_mode: ToolInvocationMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecuteToolRequest {
    pub scope: ToolScope,
    pub tool_id: ToolId,
    pub arguments: JsonValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolExecutionErrorCode {
    InvalidArguments,
    PermissionDenied,
    ApprovalRequired,
    Unavailable,
    SchemaChanged,
    RateLimited,
    RequestTooLarge,
    ResultTooLarge,
    Timeout,
    Cancelled,
    BackendFailed,
    ToolhiveMissing,
    NotAuthenticated,
    ScopeMismatch,
    LeaseExpired,
    LeaseRevoked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolExecutionError {
    pub code: ToolExecutionErrorCode,
    pub message: String,
    pub retryable: bool,
    pub redacted_details: JsonValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResultEnvelope {
    pub content: JsonValue,
    pub summary: Option<String>,
    pub truncated: bool,
    pub original_size_bytes: Option<u64>,
    #[serde(default)]
    pub artifact_refs: Vec<UpravaRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecuteToolResponse {
    pub tool_call_id: ToolCallId,
    pub state: ToolCallState,
    pub result: Option<ToolResultEnvelope>,
    pub error: Option<ToolExecutionError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpAccessLeaseClaims {
    pub lease_id: McpAccessLeaseId,
    pub audience: String,
    pub actor_ref: ActorRef,
    pub session_thread_id: SessionThreadId,
    pub project_id: Option<ProjectId>,
    pub project_placement_id: ProjectPlacementId,
    pub node_id: NodeId,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub credential_version: u64,
    #[serde(default)]
    pub resume_command_id: Option<CommandId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrationConnectRequest {
    pub integration_id: IntegrationId,
    pub project_id: Option<ProjectId>,
    pub node_id: NodeId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrationConnectResponse {
    pub connection: IntegrationConnectionSummary,
    pub authorization_url: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrationDisconnectRequest {
    pub revoke_remote: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrationDisconnectResponse {
    pub connection: IntegrationConnectionSummary,
    pub remote_revocation_confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDefinitionsResponse {
    pub items: Vec<ToolDefinition>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolAvailabilityResponse {
    pub items: Vec<ToolAvailability>,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedCapabilitiesResponse {
    pub items: Vec<ObservedCapability>,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrationConnectionsResponse {
    pub items: Vec<IntegrationConnectionSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpDependencyStatusesResponse {
    pub items: Vec<McpDependencyStatus>,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallsResponse {
    pub items: Vec<ToolCallSummary>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolingCommandV1 {
    pub contract_version: u16,
    pub payload: ToolingCommandPayloadV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolingCommandPayloadV1 {
    BeginIntegrationAuthorization {
        dependency_instance_id: McpDependencyInstanceId,
        integration_id: IntegrationId,
        upstream_url: String,
        workload_name: String,
        tool_namespace: String,
    },
    ExecuteExternalTool {
        tool_call_id: ToolCallId,
        tool_id: ToolId,
        schema_hash: String,
        integration_id: IntegrationId,
        dependency_instance_id: McpDependencyInstanceId,
        scope: Box<ToolScope>,
        arguments: JsonValue,
        deadline_at: DateTime<Utc>,
        max_result_bytes: u64,
    },
    CancelToolCall {
        tool_call_id: ToolCallId,
        reason: Option<String>,
    },
    UpdateDependencyDesiredState {
        dependency_instance_id: McpDependencyInstanceId,
        integration_id: IntegrationId,
        desired_state: IntegrationDesiredState,
        credential_ref: Option<String>,
        upstream_url: String,
        workload_name: String,
        tool_namespace: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolingEventV1 {
    pub contract_version: u16,
    pub payload: ToolingEventPayloadV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolingEventPayloadV1 {
    DependencyActualStateReported {
        status: McpDependencyStatus,
    },
    ToolDefinitionsDiscovered {
        dependency_instance_id: McpDependencyInstanceId,
        definitions: Vec<ToolDefinition>,
        schema_set_hash: String,
    },
    ToolCallStarted {
        tool_call_id: ToolCallId,
        started_at: DateTime<Utc>,
    },
    ToolCallCompleted {
        tool_call_id: ToolCallId,
        result: ToolResultEnvelope,
        completed_at: DateTime<Utc>,
    },
    ToolCallFailed {
        tool_call_id: ToolCallId,
        error: ToolExecutionError,
        failed_at: DateTime<Utc>,
    },
    ToolCallDenied {
        tool_call_id: ToolCallId,
        error: ToolExecutionError,
        denied_at: DateTime<Utc>,
    },
    ToolAvailabilityChanged {
        availability: ToolAvailability,
    },
}

/// Computes the definition schema hash after recursively ordering JSON object
/// keys. Array order remains significant and object key order does not.
pub fn compute_tool_schema_hash(
    input_schema: &JsonValue,
    output_schema: Option<&JsonValue>,
) -> Result<String, serde_json::Error> {
    let material = serde_json::json!({
        "input_schema": canonical_json(&input_schema.0),
        "output_schema": output_schema.map(|schema| canonical_json(&schema.0)),
    });
    let bytes = serde_json::to_vec(&material)?;
    let digest = Sha256::digest(bytes);
    Ok(format!("sha256:{digest:x}"))
}

fn canonical_json(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(object) => {
            let sorted = object
                .iter()
                .map(|(key, value)| (key.clone(), canonical_json(value)))
                .collect::<BTreeMap<_, _>>();
            serde_json::Value::Object(sorted.into_iter().collect())
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.iter().map(canonical_json).collect())
        }
        scalar => scalar.clone(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn schema_hash_is_stable_when_object_key_order_changes() {
        let first = JsonValue(
            json!({"type": "object", "properties": {"a": {"type": "string"}, "b": {"type": "number"}}}),
        );
        let second = JsonValue(
            json!({"properties": {"b": {"type": "number"}, "a": {"type": "string"}}, "type": "object"}),
        );

        let first_hash = compute_tool_schema_hash(&first, None).expect("schema hash computes");
        let second_hash = compute_tool_schema_hash(&second, None).expect("schema hash computes");

        assert_eq!(first_hash, second_hash);
    }

    #[test]
    fn schema_hash_changes_when_array_order_changes() {
        let first = JsonValue(json!({"required": ["a", "b"]}));
        let second = JsonValue(json!({"required": ["b", "a"]}));

        let first_hash = compute_tool_schema_hash(&first, None).expect("schema hash computes");
        let second_hash = compute_tool_schema_hash(&second, None).expect("schema hash computes");

        assert_ne!(first_hash, second_hash);
    }

    #[test]
    fn search_result_does_not_serialize_a_full_schema() {
        let result = ToolSearchResult {
            tool_id: ToolId::from("uprava.session.inspect"),
            display_name: "Inspect session".to_owned(),
            short_description: "Inspect one session".to_owned(),
            source_kind: ToolSourceKind::UpravaNative,
            risk_level: ToolRiskLevel::ReadOnly,
            availability_state: ToolAvailabilityState::Available,
            unavailable_reason: None,
            schema_hash: "sha256:fixture".to_owned(),
        };

        let serialized = serde_json::to_string(&result).expect("search result serializes");

        assert!(!serialized.contains("input_schema"));
    }

    #[test]
    fn tool_call_terminal_states_are_explicit() {
        let terminal_states = [
            ToolCallState::Completed,
            ToolCallState::Failed,
            ToolCallState::Denied,
            ToolCallState::Cancelled,
            ToolCallState::TimedOut,
        ];

        assert!(terminal_states.into_iter().all(ToolCallState::is_terminal));
    }

    #[test]
    fn tooling_command_round_trips_without_extension_payload() {
        let command = ToolingCommandV1 {
            contract_version: TOOLING_CONTRACT_VERSION_V1,
            payload: ToolingCommandPayloadV1::CancelToolCall {
                tool_call_id: ToolCallId::from("tool-call-fixture"),
                reason: Some("operator_requested".to_owned()),
            },
        };

        let encoded = serde_json::to_string(&command).expect("tooling command serializes");
        let decoded: ToolingCommandV1 =
            serde_json::from_str(&encoded).expect("tooling command deserializes");

        assert_eq!(decoded, command);
    }
}
