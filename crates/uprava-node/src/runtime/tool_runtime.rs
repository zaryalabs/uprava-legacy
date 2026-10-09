//! Node-owned observed capability probes and ToolHive MCP bridge.

use super::*;

const LINEAR_MCP_URL: &str = "https://mcp.linear.app/mcp";
const LINEAR_PROXY_PORT: u16 = 18_766;
const LINEAR_CALLBACK_PORT: u16 = 18_765;
const TOOL_PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const MCP_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_TOOL_METADATA_CHARS: usize = 4_096;
const MAX_TOOL_DEFINITIONS: usize = 256;
const MAX_TOOLHIVE_PROCESS_OUTPUT_BYTES: usize = 256 * 1024;
const MAX_TOOL_SCHEMA_BYTES: usize = 128 * 1024;
const MAX_TOOL_SCHEMA_DEPTH: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ToolDependencyDesired {
    pub(crate) dependency_instance_id: McpDependencyInstanceId,
    pub(crate) integration_id: IntegrationId,
    pub(crate) desired_state: IntegrationDesiredState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) credential_ref: Option<String>,
    pub(crate) upstream_url: String,
    pub(crate) workload_name: String,
    pub(crate) tool_namespace: String,
    pub(crate) proxy_port: u16,
}

pub(crate) async fn observed_capabilities(
    config: &NodeConfig,
    node_id: &NodeId,
) -> Vec<ObservedCapability> {
    let mut items = Vec::with_capacity(4);
    items.push(probe_toolhive(config, node_id).await);
    items.push(probe_cli(node_id, "git", &["--version"], None).await);
    items.push(probe_cli(node_id, "gh", &["--version"], Some(&["auth", "status"])).await);
    items.push(probe_cli(node_id, "glab", &["--version"], Some(&["auth", "status"])).await);
    items
}

pub(crate) async fn observed_dependency_statuses(
    config: &NodeConfig,
    local_state: &NodeLocalState,
    node_id: &NodeId,
) -> Vec<McpDependencyStatus> {
    let mut statuses = Vec::with_capacity(local_state.tool_dependencies.len());
    for desired in local_state.tool_dependencies.values() {
        statuses.push(observe_dependency_status(config, node_id, desired).await);
    }
    statuses.sort_by(|left, right| {
        left.dependency_instance_id
            .as_str()
            .cmp(right.dependency_instance_id.as_str())
    });
    statuses
}

async fn probe_toolhive(config: &NodeConfig, node_id: &NodeId) -> ObservedCapability {
    let result = toolhive_version(config).await;
    observed_from_probe(node_id, "runtime.toolhive", "ToolHive", result, None)
}

async fn probe_cli(
    node_id: &NodeId,
    binary: &str,
    version_args: &[&str],
    auth_args: Option<&[&str]>,
) -> ObservedCapability {
    let version = bounded_command(binary, version_args, TOOL_PROBE_TIMEOUT).await;
    let auth_state = if version.is_ok() {
        match auth_args {
            Some(args) => Some(
                if bounded_command(binary, args, TOOL_PROBE_TIMEOUT)
                    .await
                    .is_ok()
                {
                    "authenticated"
                } else {
                    "not_authenticated"
                }
                .to_owned(),
            ),
            None => None,
        }
    } else {
        None
    };
    observed_from_probe(node_id, binary, binary, version, auth_state)
}

fn observed_from_probe(
    node_id: &NodeId,
    key: &str,
    display_name: &str,
    probe: anyhow::Result<String>,
    safe_authentication_state: Option<String>,
) -> ObservedCapability {
    let (state, version) = match probe {
        Ok(version) => (
            ObservedCapabilityState::Available,
            Some(
                version
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .chars()
                    .take(160)
                    .collect(),
            ),
        ),
        Err(_) => (ObservedCapabilityState::Unavailable, None),
    };
    ObservedCapability {
        node_id: node_id.clone(),
        capability_key: key.to_owned(),
        display_name: display_name.to_owned(),
        state,
        version,
        safe_authentication_state,
        observed_at: Utc::now(),
    }
}

async fn bounded_command(
    binary: &str,
    args: &[&str],
    deadline: Duration,
) -> anyhow::Result<String> {
    let mut command = TokioCommand::new(binary);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn()?;
    let stdout_task = tokio::spawn(read_capped_process_output(
        child.stdout.take(),
        MAX_TOOLHIVE_PROCESS_OUTPUT_BYTES,
    ));
    let stderr_task = tokio::spawn(read_capped_process_output(
        child.stderr.take(),
        MAX_TOOLHIVE_PROCESS_OUTPUT_BYTES,
    ));
    let status = match timeout(deadline, child.wait()).await {
        Ok(status) => status?,
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            anyhow::bail!("capability probe timed out");
        }
    };
    let (stdout, stdout_truncated) = join_capped_output(stdout_task).await;
    let (_, stderr_truncated) = join_capped_output(stderr_task).await;
    anyhow::ensure!(status.success(), "capability probe failed");
    anyhow::ensure!(
        !stdout_truncated && !stderr_truncated,
        "capability probe output exceeded its limit"
    );
    Ok(stdout.trim().to_owned())
}

pub(crate) fn tool_call_cancellation_key(tool_call_id: &ToolCallId) -> String {
    format!("tool-call:{}", tool_call_id.as_str())
}

pub(crate) async fn execute_tooling_command(
    config: &NodeConfig,
    local_state: &mut NodeLocalState,
    command: &ToolingCommandV1,
    cancellation: Option<watch::Receiver<bool>>,
) -> (CommandState, JsonValue) {
    if command.contract_version != TOOLING_CONTRACT_VERSION_V1 {
        return tooling_failure(
            None,
            ToolExecutionErrorCode::BackendFailed,
            "Unsupported tooling contract version",
            false,
        );
    }
    match &command.payload {
        ToolingCommandPayloadV1::BeginIntegrationAuthorization {
            dependency_instance_id,
            integration_id,
            upstream_url,
            workload_name,
            tool_namespace,
        } => {
            let desired = match ToolDependencyDesired::try_new(
                dependency_instance_id.clone(),
                integration_id.clone(),
                IntegrationDesiredState::Enabled,
                Some("toolhive:managed-linear".to_owned()),
                upstream_url,
                workload_name,
                tool_namespace,
            ) {
                Ok(desired) => desired,
                Err(message) => {
                    return tooling_failure(
                        None,
                        ToolExecutionErrorCode::BackendFailed,
                        message,
                        false,
                    )
                }
            };
            let node_id = local_state
                .node_id
                .clone()
                .unwrap_or_else(|| NodeId::from("unregistered-node"));
            local_state
                .tool_dependencies
                .insert(dependency_instance_id.to_string(), desired.clone());
            match toolhive_begin_authorization(config, &desired).await {
                Ok((authorization_url, expires_at)) => tooling_success(ToolingCommandResult {
                    status: Some(dependency_status(
                        &desired,
                        &node_id,
                        McpDependencyActualState::Starting,
                        None,
                        None,
                        None,
                        Utc::now(),
                    )),
                    definitions: vec![],
                    event: None,
                    authorization_url: Some(authorization_url),
                    authorization_expires_at: Some(expires_at),
                }),
                Err(_) => tooling_failure(
                    None,
                    ToolExecutionErrorCode::BackendFailed,
                    "ToolHive could not start Linear authorization",
                    true,
                ),
            }
        }
        ToolingCommandPayloadV1::UpdateDependencyDesiredState {
            dependency_instance_id,
            integration_id,
            desired_state,
            credential_ref,
            upstream_url,
            workload_name,
            tool_namespace,
        } => {
            let desired = match ToolDependencyDesired::try_new(
                dependency_instance_id.clone(),
                integration_id.clone(),
                *desired_state,
                credential_ref.clone(),
                upstream_url,
                workload_name,
                tool_namespace,
            ) {
                Ok(desired) => desired,
                Err(message) => {
                    return tooling_failure(
                        None,
                        ToolExecutionErrorCode::BackendFailed,
                        message,
                        false,
                    )
                }
            };
            local_state
                .tool_dependencies
                .insert(dependency_instance_id.to_string(), desired.clone());
            let node_id = local_state
                .node_id
                .clone()
                .unwrap_or_else(|| NodeId::from("unregistered-node"));
            let (status, definitions) = reconcile_dependency(config, &node_id, &desired).await;
            tooling_success(ToolingCommandResult {
                status: Some(status),
                definitions,
                event: None,
                authorization_url: None,
                authorization_expires_at: None,
            })
        }
        ToolingCommandPayloadV1::ExecuteExternalTool {
            tool_call_id,
            tool_id,
            schema_hash,
            dependency_instance_id,
            arguments,
            deadline_at,
            max_result_bytes,
            ..
        } => {
            let Some(desired) = local_state
                .tool_dependencies
                .get(dependency_instance_id.as_str())
                .cloned()
            else {
                return tooling_failure(
                    Some(tool_call_id),
                    ToolExecutionErrorCode::Unavailable,
                    "MCP dependency is not configured on this Node",
                    true,
                );
            };
            if desired.desired_state != IntegrationDesiredState::Enabled {
                return tooling_failure(
                    Some(tool_call_id),
                    ToolExecutionErrorCode::Unavailable,
                    "MCP dependency is stopped",
                    false,
                );
            }
            if desired.credential_ref.is_none() {
                return tooling_failure(
                    Some(tool_call_id),
                    ToolExecutionErrorCode::NotAuthenticated,
                    "MCP dependency is not authenticated",
                    false,
                );
            }
            let now = Utc::now();
            let remaining = (*deadline_at - now)
                .to_std()
                .unwrap_or(Duration::ZERO)
                .min(MCP_REQUEST_TIMEOUT);
            if remaining.is_zero() {
                return tooling_failure(
                    Some(tool_call_id),
                    ToolExecutionErrorCode::Timeout,
                    "External tool deadline elapsed before execution",
                    true,
                );
            }
            let tool_name = tool_id
                .as_str()
                .strip_prefix(&format!("{}.", desired.tool_namespace))
                .unwrap_or(tool_id.as_str());
            let max_bytes = (*max_result_bytes).min(TOOL_RESULT_MAX_BYTES);
            match call_external_tool(
                config,
                ExternalToolCallRequest {
                    desired: &desired,
                    tool_name,
                    arguments,
                    expected_schema_hash: schema_hash,
                    max_result_bytes: max_bytes,
                    deadline: remaining,
                },
                cancellation,
            )
            .await
            {
                Ok(result) => tooling_success(ToolingCommandResult {
                    status: None,
                    definitions: vec![],
                    event: Some(ToolingEventV1 {
                        contract_version: TOOLING_CONTRACT_VERSION_V1,
                        payload: ToolingEventPayloadV1::ToolCallCompleted {
                            tool_call_id: tool_call_id.clone(),
                            result,
                            completed_at: Utc::now(),
                        },
                    }),
                    authorization_url: None,
                    authorization_expires_at: None,
                }),
                Err(error) => tooling_failure_event(tool_call_id, error),
            }
        }
        ToolingCommandPayloadV1::CancelToolCall {
            tool_call_id,
            reason: _,
        } => tooling_success(ToolingCommandResult {
            status: None,
            definitions: vec![],
            event: Some(ToolingEventV1 {
                contract_version: TOOLING_CONTRACT_VERSION_V1,
                payload: ToolingEventPayloadV1::ToolCallFailed {
                    tool_call_id: tool_call_id.clone(),
                    error: tool_error(
                        ToolExecutionErrorCode::Cancelled,
                        "External tool call was cancelled",
                        false,
                    ),
                    failed_at: Utc::now(),
                },
            }),
            authorization_url: None,
            authorization_expires_at: None,
        }),
    }
}

impl ToolDependencyDesired {
    fn try_new(
        dependency_instance_id: McpDependencyInstanceId,
        integration_id: IntegrationId,
        desired_state: IntegrationDesiredState,
        credential_ref: Option<String>,
        upstream_url: &str,
        workload_name: &str,
        tool_namespace: &str,
    ) -> Result<Self, &'static str> {
        if upstream_url != LINEAR_MCP_URL {
            return Err("Only the pinned Linear MCP upstream is allowed in v1");
        }
        if !valid_identifier(workload_name) || !valid_identifier(tool_namespace) {
            return Err("ToolHive workload and namespace must use safe identifiers");
        }
        let proxy_port = dependency_proxy_port(&dependency_instance_id);
        Ok(Self {
            dependency_instance_id,
            integration_id,
            desired_state,
            credential_ref,
            upstream_url: upstream_url.to_owned(),
            workload_name: workload_name.to_owned(),
            tool_namespace: tool_namespace.to_owned(),
            proxy_port,
        })
    }
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn dependency_proxy_port(_id: &McpDependencyInstanceId) -> u16 {
    LINEAR_PROXY_PORT
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ToolingCommandResult {
    pub(crate) status: Option<McpDependencyStatus>,
    #[serde(default)]
    pub(crate) definitions: Vec<ToolDefinition>,
    pub(crate) event: Option<ToolingEventV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) authorization_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) authorization_expires_at: Option<DateTime<Utc>>,
}

pub(crate) fn durable_tooling_result_payload(payload: &JsonValue) -> JsonValue {
    let mut value = payload.0.clone();
    if let Some(object) = value.as_object_mut() {
        object.remove("authorization_url");
    }
    JsonValue(value)
}

fn tooling_success(result: ToolingCommandResult) -> (CommandState, JsonValue) {
    match serde_json::to_value(result) {
        Ok(value) => (CommandState::Completed, JsonValue(value)),
        Err(_) => tooling_failure(
            None,
            ToolExecutionErrorCode::BackendFailed,
            "Node could not encode the tooling result",
            false,
        ),
    }
}

fn tooling_failure_event(
    tool_call_id: &ToolCallId,
    error: ToolExecutionError,
) -> (CommandState, JsonValue) {
    tooling_success(ToolingCommandResult {
        status: None,
        definitions: vec![],
        event: Some(ToolingEventV1 {
            contract_version: TOOLING_CONTRACT_VERSION_V1,
            payload: ToolingEventPayloadV1::ToolCallFailed {
                tool_call_id: tool_call_id.clone(),
                error,
                failed_at: Utc::now(),
            },
        }),
        authorization_url: None,
        authorization_expires_at: None,
    })
}

fn tooling_failure(
    tool_call_id: Option<&ToolCallId>,
    code: ToolExecutionErrorCode,
    message: &str,
    retryable: bool,
) -> (CommandState, JsonValue) {
    let error = tool_error(code, message, retryable);
    match tool_call_id {
        Some(tool_call_id) => tooling_failure_event(tool_call_id, error),
        None => (
            CommandState::Failed,
            JsonValue(serde_json::json!({
                "error_code": format!("{code:?}").to_ascii_lowercase(),
                "message": message,
                "retryable": retryable,
            })),
        ),
    }
}

fn tool_error(code: ToolExecutionErrorCode, message: &str, retryable: bool) -> ToolExecutionError {
    ToolExecutionError {
        code,
        message: message.to_owned(),
        retryable,
        redacted_details: JsonValue(serde_json::json!({})),
    }
}

async fn reconcile_dependency(
    config: &NodeConfig,
    node_id: &NodeId,
    desired: &ToolDependencyDesired,
) -> (McpDependencyStatus, Vec<ToolDefinition>) {
    let now = Utc::now();
    let version = match toolhive_version(config).await {
        Ok(version) => version,
        Err(_) => {
            return (
                dependency_status(
                    desired,
                    node_id,
                    McpDependencyActualState::ToolhiveMissing,
                    None,
                    None,
                    Some("toolhive_missing"),
                    now,
                ),
                vec![],
            )
        }
    };
    if desired.desired_state == IntegrationDesiredState::Disabled {
        let _ = toolhive_cleanup(config, desired).await;
        return (
            dependency_status(
                desired,
                node_id,
                McpDependencyActualState::Stopped,
                Some(version),
                None,
                None,
                now,
            ),
            vec![],
        );
    }
    if desired.credential_ref.is_none() {
        return (
            dependency_status(
                desired,
                node_id,
                McpDependencyActualState::MissingAuth,
                Some(version),
                None,
                Some("missing_auth"),
                now,
            ),
            vec![],
        );
    }
    if !toolhive_workload_running(config, desired).await
        && toolhive_start(config, desired).await.is_err()
    {
        return (
            dependency_status(
                desired,
                node_id,
                McpDependencyActualState::Failed,
                Some(version),
                None,
                Some("toolhive_start_failed"),
                now,
            ),
            vec![],
        );
    }
    match discover_tools(config, desired).await {
        Ok(definitions) => {
            let schema_set_hash = schema_set_hash(&definitions);
            (
                dependency_status(
                    desired,
                    node_id,
                    McpDependencyActualState::Running,
                    Some(version),
                    Some(schema_set_hash),
                    None,
                    Utc::now(),
                ),
                definitions,
            )
        }
        Err(_) => (
            dependency_status(
                desired,
                node_id,
                McpDependencyActualState::Degraded,
                Some(version),
                None,
                Some("tool_discovery_failed"),
                Utc::now(),
            ),
            vec![],
        ),
    }
}

fn dependency_status(
    desired: &ToolDependencyDesired,
    node_id: &NodeId,
    actual_state: McpDependencyActualState,
    runtime_version: Option<String>,
    schema_set_hash: Option<String>,
    error_code: Option<&str>,
    observed_at: DateTime<Utc>,
) -> McpDependencyStatus {
    McpDependencyStatus {
        dependency_instance_id: desired.dependency_instance_id.clone(),
        integration_id: desired.integration_id.clone(),
        node_id: node_id.clone(),
        desired_state: desired.desired_state,
        actual_state,
        runtime_name: "toolhive".to_owned(),
        runtime_version,
        upstream_identity: Some("linear-remote-mcp".to_owned()),
        schema_set_hash,
        error_code: error_code.map(str::to_owned),
        observed_at,
    }
}

async fn observe_dependency_status(
    config: &NodeConfig,
    node_id: &NodeId,
    desired: &ToolDependencyDesired,
) -> McpDependencyStatus {
    let now = Utc::now();
    let version = match toolhive_version(config).await {
        Ok(version) => version,
        Err(_) => {
            return dependency_status(
                desired,
                node_id,
                McpDependencyActualState::ToolhiveMissing,
                None,
                None,
                Some("toolhive_missing"),
                now,
            )
        }
    };
    if desired.desired_state == IntegrationDesiredState::Disabled {
        return dependency_status(
            desired,
            node_id,
            McpDependencyActualState::Stopped,
            Some(version),
            None,
            None,
            now,
        );
    }
    if desired.credential_ref.is_none() {
        return dependency_status(
            desired,
            node_id,
            McpDependencyActualState::MissingAuth,
            Some(version),
            None,
            Some("missing_auth"),
            now,
        );
    }
    if let Ok(Ok(definitions)) = timeout(TOOL_PROBE_TIMEOUT, discover_tools(config, desired)).await
    {
        return dependency_status(
            desired,
            node_id,
            McpDependencyActualState::Running,
            Some(version),
            Some(schema_set_hash(&definitions)),
            None,
            Utc::now(),
        );
    }
    if toolhive_workload_running(config, desired).await {
        dependency_status(
            desired,
            node_id,
            McpDependencyActualState::Starting,
            Some(version),
            None,
            None,
            now,
        )
    } else {
        dependency_status(
            desired,
            node_id,
            McpDependencyActualState::MissingAuth,
            Some(version),
            None,
            Some("missing_auth"),
            now,
        )
    }
}

async fn toolhive_begin_authorization(
    config: &NodeConfig,
    desired: &ToolDependencyDesired,
) -> anyhow::Result<(String, DateTime<Utc>)> {
    let response = toolhive_client(config)?
        .post(toolhive_workload_url(config, desired, "authorize")?)
        .json(&toolhive_workload_request(desired))
        .send()
        .await?
        .error_for_status()?
        .json::<ToolhiveBridgeAuthorizationResponse>()
        .await?;
    Ok((response.authorization_url, response.expires_at))
}

async fn toolhive_start(
    config: &NodeConfig,
    desired: &ToolDependencyDesired,
) -> anyhow::Result<()> {
    toolhive_client(config)?
        .post(toolhive_workload_url(config, desired, "start")?)
        .json(&toolhive_workload_request(desired))
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

async fn toolhive_cleanup(
    config: &NodeConfig,
    desired: &ToolDependencyDesired,
) -> anyhow::Result<()> {
    toolhive_client(config)?
        .delete(toolhive_workload_url(config, desired, "")?)
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

async fn toolhive_workload_running(config: &NodeConfig, desired: &ToolDependencyDesired) -> bool {
    let operation = async {
        toolhive_client(config)?
            .get(toolhive_workload_url(config, desired, "")?)
            .send()
            .await?
            .error_for_status()?
            .json::<ToolhiveBridgeWorkloadStatusResponse>()
            .await
            .map_err(anyhow::Error::from)
    };
    timeout(TOOL_PROBE_TIMEOUT, operation)
        .await
        .ok()
        .and_then(Result::ok)
        .is_some_and(|status| status.running)
}

async fn toolhive_version(config: &NodeConfig) -> anyhow::Result<String> {
    let operation = async {
        toolhive_client(config)?
            .get(config.toolhive_url.join("api/v1/version")?)
            .send()
            .await?
            .error_for_status()?
            .json::<ToolhiveBridgeVersionResponse>()
            .await
            .map(|response| response.version)
            .map_err(anyhow::Error::from)
    };
    timeout(TOOL_PROBE_TIMEOUT, operation)
        .await
        .context("ToolHive bridge version probe timed out")?
}

fn toolhive_client(config: &NodeConfig) -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(TOOL_PROBE_TIMEOUT)
        .timeout(config.toolhive_timeout)
        .build()
        .context("failed to build ToolHive bridge client")
}

fn toolhive_workload_url(
    config: &NodeConfig,
    desired: &ToolDependencyDesired,
    action: &str,
) -> anyhow::Result<reqwest::Url> {
    let suffix = if action.is_empty() {
        String::new()
    } else {
        format!("/{action}")
    };
    config
        .toolhive_url
        .join(&format!(
            "api/v1/workloads/{}{}",
            desired.workload_name, suffix
        ))
        .context("failed to construct ToolHive bridge URL")
}

fn toolhive_workload_request(desired: &ToolDependencyDesired) -> ToolhiveBridgeWorkloadRequest {
    ToolhiveBridgeWorkloadRequest {
        contract_version: TOOLHIVE_BRIDGE_CONTRACT_VERSION_V1,
        upstream_url: desired.upstream_url.clone(),
        workload_name: desired.workload_name.clone(),
        proxy_port: desired.proxy_port,
        callback_port: LINEAR_CALLBACK_PORT,
    }
}

async fn toolhive_mcp_request(
    config: &NodeConfig,
    desired: &ToolDependencyDesired,
    method: &str,
    params: serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    let response = toolhive_client(config)?
        .post(toolhive_workload_url(config, desired, "mcp")?)
        .json(&ToolhiveBridgeMcpRequest {
            contract_version: TOOLHIVE_BRIDGE_CONTRACT_VERSION_V1,
            proxy_port: desired.proxy_port,
            method: method.to_owned(),
            params: JsonValue(params),
        })
        .send()
        .await?
        .error_for_status()?
        .json::<ToolhiveBridgeMcpResponse>()
        .await?;
    Ok(response.result.0)
}

async fn discover_tools(
    config: &NodeConfig,
    desired: &ToolDependencyDesired,
) -> anyhow::Result<Vec<ToolDefinition>> {
    let result = toolhive_mcp_request(config, desired, "tools/list", serde_json::json!({})).await?;
    normalize_tool_definitions(desired, &result)
}

struct ExternalToolCallRequest<'a> {
    desired: &'a ToolDependencyDesired,
    tool_name: &'a str,
    arguments: &'a JsonValue,
    expected_schema_hash: &'a str,
    max_result_bytes: u64,
    deadline: Duration,
}

async fn call_external_tool(
    config: &NodeConfig,
    request: ExternalToolCallRequest<'_>,
    mut cancellation: Option<watch::Receiver<bool>>,
) -> Result<ToolResultEnvelope, ToolExecutionError> {
    let ExternalToolCallRequest {
        desired,
        tool_name,
        arguments,
        expected_schema_hash,
        max_result_bytes,
        deadline,
    } = request;
    let operation = async {
        let definitions = discover_tools(config, desired).await.map_err(|_| {
            tool_error(
                ToolExecutionErrorCode::BackendFailed,
                "Tool discovery failed before execution",
                true,
            )
        })?;
        let definition = definitions
            .iter()
            .find(|definition| definition.source_tool_name == tool_name)
            .ok_or_else(|| {
                tool_error(
                    ToolExecutionErrorCode::Unavailable,
                    "Upstream tool is unavailable",
                    true,
                )
            })?;
        if definition.schema_hash != expected_schema_hash {
            return Err(tool_error(
                ToolExecutionErrorCode::SchemaChanged,
                "Upstream tool schema changed",
                false,
            ));
        }
        let content = toolhive_mcp_request(
            config,
            desired,
            "tools/call",
            serde_json::json!({
                "name": tool_name,
                "arguments": arguments.0,
            }),
        )
        .await
        .map_err(|_| {
            tool_error(
                ToolExecutionErrorCode::BackendFailed,
                "External MCP call failed",
                true,
            )
        })?;
        let bytes = serde_json::to_vec(&content).map_err(|_| {
            tool_error(
                ToolExecutionErrorCode::BackendFailed,
                "External MCP result was invalid",
                false,
            )
        })?;
        if bytes.len() as u64 > max_result_bytes {
            return Err(tool_error(
                ToolExecutionErrorCode::ResultTooLarge,
                "External MCP result exceeds the configured limit",
                false,
            ));
        }
        Ok(ToolResultEnvelope {
            content: JsonValue(content),
            summary: None,
            truncated: false,
            original_size_bytes: Some(bytes.len() as u64),
            artifact_refs: vec![],
        })
    };
    tokio::select! {
        result = timeout(deadline, operation) => result.unwrap_or_else(|_| Err(tool_error(
            ToolExecutionErrorCode::Timeout,
            "External MCP call timed out",
            true,
        ))),
        _ = async {
            if let Some(receiver) = cancellation.as_mut() {
                let _ = receiver.changed().await;
            } else {
                std::future::pending::<()>().await;
            }
        } => Err(tool_error(
            ToolExecutionErrorCode::Cancelled,
            "External MCP call was cancelled",
            false,
        )),
    }
}

fn normalize_tool_definitions(
    desired: &ToolDependencyDesired,
    result: &serde_json::Value,
) -> anyhow::Result<Vec<ToolDefinition>> {
    let tools = result
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .context("MCP tools/list result did not contain tools")?;
    anyhow::ensure!(tools.len() <= MAX_TOOL_DEFINITIONS, "too many MCP tools");
    let now = Utc::now();
    tools
        .iter()
        .map(|tool| {
            let source_name = bounded_metadata(tool, "name")?;
            let normalized_name = normalize_tool_name(&source_name)?;
            let description = tool
                .get("description")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("External Linear MCP tool")
                .chars()
                .take(MAX_TOOL_METADATA_CHARS)
                .collect::<String>();
            let input_schema = tool
                .get("inputSchema")
                .cloned()
                .context("MCP tool is missing inputSchema")?;
            validate_upstream_schema(&input_schema)?;
            let output_schema = tool.get("outputSchema").cloned();
            if let Some(output_schema) = output_schema.as_ref() {
                validate_upstream_schema(output_schema)?;
            }
            let input_schema = JsonValue(input_schema);
            let output_schema = output_schema.map(JsonValue);
            let schema_hash =
                uprava_protocol::compute_tool_schema_hash(&input_schema, output_schema.as_ref())?;
            Ok(ToolDefinition {
                tool_id: ToolId::from(format!("{}.{}", desired.tool_namespace, normalized_name)),
                source_id: ToolSourceId::from("linear-remote-mcp"),
                source_kind: ToolSourceKind::ExternalMcp,
                source_tool_name: source_name,
                version: 1,
                display_name: normalized_name.replace('_', " "),
                short_description: description,
                documentation_url: None,
                input_schema,
                output_schema,
                schema_hash,
                risk_level: ToolRiskLevel::ExternalRead,
                required_permissions: vec!["integration.linear.read".to_owned()],
                execution_kind: ToolExecutionKind::ToolhiveMcp,
                approval_policy: PolicyDecision::Allow,
                redaction: ToolRedactionPolicy {
                    argument_json_pointers: vec![],
                    result_json_pointers: vec![],
                    redact_all_arguments: false,
                    redact_all_result: false,
                    max_summary_bytes: 2_048,
                },
                state: ToolDefinitionState::Active,
                created_at: now,
                updated_at: now,
            })
        })
        .collect()
}

fn validate_upstream_schema(schema: &serde_json::Value) -> anyhow::Result<()> {
    anyhow::ensure!(schema.is_object(), "MCP JSON Schema must be an object");
    anyhow::ensure!(
        serde_json::to_vec(schema)?.len() <= MAX_TOOL_SCHEMA_BYTES,
        "MCP JSON Schema exceeds its byte limit"
    );
    validate_upstream_value(schema, 0)
}

fn validate_upstream_value(value: &serde_json::Value, depth: usize) -> anyhow::Result<()> {
    anyhow::ensure!(
        depth <= MAX_TOOL_SCHEMA_DEPTH,
        "MCP JSON Schema is too deep"
    );
    match value {
        serde_json::Value::Object(object) => {
            for (key, value) in object {
                anyhow::ensure!(
                    key.len() <= MAX_TOOL_METADATA_CHARS,
                    "MCP JSON Schema key is too large"
                );
                validate_upstream_value(value, depth + 1)?;
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                validate_upstream_value(value, depth + 1)?;
            }
        }
        serde_json::Value::String(value) => anyhow::ensure!(
            value.len() <= MAX_TOOL_METADATA_CHARS,
            "MCP JSON Schema string is too large"
        ),
        _ => {}
    }
    Ok(())
}

fn bounded_metadata(tool: &serde_json::Value, key: &str) -> anyhow::Result<String> {
    let value = tool
        .get(key)
        .and_then(serde_json::Value::as_str)
        .context("MCP tool metadata is missing")?;
    anyhow::ensure!(
        !value.is_empty() && value.len() <= MAX_TOOL_METADATA_CHARS,
        "MCP tool metadata exceeds limits"
    );
    Ok(value.to_owned())
}

fn normalize_tool_name(value: &str) -> anyhow::Result<String> {
    let normalized = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    anyhow::ensure!(
        !normalized.is_empty() && normalized.len() <= 120,
        "invalid MCP tool name"
    );
    Ok(normalized)
}

fn schema_set_hash(definitions: &[ToolDefinition]) -> String {
    let mut material = definitions
        .iter()
        .map(|definition| format!("{}:{}", definition.tool_id, definition.schema_hash))
        .collect::<Vec<_>>();
    material.sort();
    let digest = Sha256::digest(material.join("\n").as_bytes());
    format!("sha256:{digest:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependency_port_is_fixed_to_the_compose_bridge() {
        let id = McpDependencyInstanceId::from("dependency-linear");
        assert_eq!(dependency_proxy_port(&id), LINEAR_PROXY_PORT);
    }

    #[test]
    fn upstream_tool_names_are_normalized_without_metadata_injection() {
        assert_eq!(
            normalize_tool_name("search-issues").expect("name"),
            "search_issues"
        );
        assert!(normalize_tool_name("---").is_err());
    }

    #[test]
    fn workload_request_uses_the_private_bridge_contract() {
        let desired = ToolDependencyDesired::try_new(
            McpDependencyInstanceId::from("dependency-linear"),
            IntegrationId::from("integration-linear"),
            IntegrationDesiredState::Enabled,
            Some("toolhive:managed-linear".to_owned()),
            LINEAR_MCP_URL,
            "uprava-linear",
            "linear",
        )
        .expect("desired state");
        let request = toolhive_workload_request(&desired);
        assert_eq!(request.proxy_port, LINEAR_PROXY_PORT);
        assert_eq!(request.callback_port, LINEAR_CALLBACK_PORT);
        assert_eq!(
            request.contract_version,
            TOOLHIVE_BRIDGE_CONTRACT_VERSION_V1
        );
    }

    #[test]
    fn malicious_upstream_schema_is_bounded() {
        let mut value = serde_json::json!({"type": "object"});
        for _ in 0..=MAX_TOOL_SCHEMA_DEPTH {
            value = serde_json::json!({"properties": {"nested": value}});
        }
        assert!(validate_upstream_schema(&value).is_err());
        assert!(validate_upstream_schema(&serde_json::json!({
            "description": "x".repeat(MAX_TOOL_METADATA_CHARS + 1)
        }))
        .is_err());
    }

    #[tokio::test]
    async fn desired_state_reports_explicit_toolhive_missing_fallback() {
        let config = NodeConfig {
            core_url: "http://127.0.0.1:8080".parse().expect("core URL"),
            display_name: "Tooling test Node".to_owned(),
            heartbeat_interval: Duration::from_secs(5),
            state_path: std::env::temp_dir().join("uprava-tooling-test.sqlite3"),
            workspace_paths: vec![std::env::temp_dir()],
            codex_binary: "missing-codex".to_owned(),
            codex_version: None,
            codex_managed_unavailable_reason: Some("binary_not_found".to_owned()),
            codex_ignore_user_config: false,
            codex_timeout: Duration::from_secs(5),
            opensandbox_url: None,
            task_runtime_image: "uprava/codex-runtime:test".to_owned(),
            toolhive_url: "http://127.0.0.1:9".parse().expect("ToolHive fixture URL"),
            toolhive_timeout: Duration::from_secs(1),
        };
        let mut state = NodeLocalState {
            node_id: Some(NodeId::from("node-tooling-test")),
            ..NodeLocalState::default()
        };
        let command = CommandEnvelope {
            command_id: CommandId::from("command-tooling-desired"),
            kind: CommandKind::Tooling,
            target: CommandTarget::Node {
                node_id: NodeId::from("node-tooling-test"),
            },
            actor_ref: ActorRef::System,
            source_refs: vec![],
            cause_refs: vec![],
            issued_at: Utc::now(),
            correlation_id: CorrelationId::from("correlation-tooling-desired"),
            payload: CommandPayload::Tooling {
                command: Box::new(ToolingCommandV1 {
                    contract_version: TOOLING_CONTRACT_VERSION_V1,
                    payload: ToolingCommandPayloadV1::UpdateDependencyDesiredState {
                        dependency_instance_id: McpDependencyInstanceId::from("dependency-linear"),
                        integration_id: IntegrationId::from("integration-linear"),
                        desired_state: IntegrationDesiredState::Enabled,
                        credential_ref: Some("toolhive:managed-linear".to_owned()),
                        upstream_url: LINEAR_MCP_URL.to_owned(),
                        workload_name: "uprava-linear".to_owned(),
                        tool_namespace: "linear".to_owned(),
                    },
                }),
            },
        };

        let first = prepare_command_dispatch(&config, &mut state, &command).await;
        let result: ToolingCommandResult =
            serde_json::from_value(first.result_payload.0.clone()).expect("tooling result decodes");
        let replay = prepare_command_dispatch(&config, &mut state, &command).await;

        assert_eq!(first.status, CommandState::Completed);
        assert_eq!(
            result.status.expect("dependency status").actual_state,
            McpDependencyActualState::ToolhiveMissing
        );
        assert!(state.tool_dependencies.contains_key("dependency-linear"));
        assert_eq!(replay.status, first.status);
        assert_eq!(replay.result_payload, first.result_payload);
        assert!(!replay.state_changed);
    }

    #[test]
    fn tooling_cancel_uses_the_same_cancellation_identity_as_execute() {
        let tool_call_id = ToolCallId::from("tool-call-cancel");
        let execute = tooling_envelope(
            "command-tool-execute",
            ToolingCommandPayloadV1::ExecuteExternalTool {
                tool_call_id: tool_call_id.clone(),
                tool_id: ToolId::from("linear.search_issues"),
                schema_hash: "sha256:fixture".to_owned(),
                integration_id: IntegrationId::from("integration-linear"),
                dependency_instance_id: McpDependencyInstanceId::from("dependency-linear"),
                scope: Box::new(uprava_protocol::ToolScope {
                    actor_ref: ActorRef::System,
                    node_id: Some(NodeId::from("node-tooling-test")),
                    project_id: None,
                    project_placement_id: None,
                    session_thread_id: None,
                }),
                arguments: JsonValue(serde_json::json!({})),
                deadline_at: Utc::now() + chrono::Duration::seconds(5),
                max_result_bytes: TOOL_RESULT_MAX_BYTES,
            },
        );
        let cancel = tooling_envelope(
            "command-tool-cancel",
            ToolingCommandPayloadV1::CancelToolCall {
                tool_call_id: tool_call_id.clone(),
                reason: Some("test".to_owned()),
            },
        );

        assert_eq!(
            execution_cancellation_key(&execute),
            Some(tool_call_cancellation_key(&tool_call_id))
        );
        assert_eq!(
            cancellation_signal(&cancel),
            Some((tool_call_cancellation_key(&tool_call_id), true))
        );
        assert!(is_priority_cancellation_command(&cancel));
    }

    fn tooling_envelope(command_id: &str, payload: ToolingCommandPayloadV1) -> CommandEnvelope {
        CommandEnvelope {
            command_id: CommandId::from(command_id),
            kind: CommandKind::Tooling,
            target: CommandTarget::Node {
                node_id: NodeId::from("node-tooling-test"),
            },
            actor_ref: ActorRef::System,
            source_refs: vec![],
            cause_refs: vec![],
            issued_at: Utc::now(),
            correlation_id: CorrelationId::from(format!("correlation-{command_id}")),
            payload: CommandPayload::Tooling {
                command: Box::new(ToolingCommandV1 {
                    contract_version: TOOLING_CONTRACT_VERSION_V1,
                    payload,
                }),
            },
        }
    }
}
