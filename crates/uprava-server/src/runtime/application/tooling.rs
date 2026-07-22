//! Core-owned Tool Registry, progressive discovery, policy and execution.

use std::cmp::Ordering as CmpOrdering;
use std::collections::HashMap;

use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, Transaction};
use uprava_protocol::{
    compute_tool_schema_hash, CreateDynamicUiProposalRequest, ExecuteToolRequest,
    ExecuteToolResponse, InspectToolRequest, InspectToolResponse, IntegrationAuthState,
    IntegrationConnectRequest, IntegrationConnectResponse, IntegrationConnectionSummary,
    IntegrationConnectionsResponse, IntegrationDesiredState, IntegrationDisconnectRequest,
    IntegrationDisconnectResponse, IntegrationId, McpAccessLeaseClaims, McpAccessLeaseId,
    McpDependencyActualState, McpDependencyInstanceId, McpDependencyStatus,
    McpDependencyStatusesResponse, ObservedCapabilitiesResponse, ObservedCapability,
    PolicyDecision, ScopeRef, SearchToolsRequest, SearchToolsResponse, ToolAvailability,
    ToolAvailabilityResponse, ToolAvailabilityState, ToolCallDetail, ToolCallId, ToolCallState,
    ToolCallSummary, ToolCallsResponse, ToolDefinition, ToolDefinitionState,
    ToolDefinitionsResponse, ToolExecutionError, ToolExecutionErrorCode, ToolExecutionKind, ToolId,
    ToolInvocationMode, ToolRedactionPolicy, ToolResultEnvelope, ToolRiskLevel, ToolScope,
    ToolSearchResult, ToolSourceId, ToolSourceKind, ToolUnavailableReason, ToolingCommandPayloadV1,
    ToolingCommandV1, ToolingEventPayloadV1, ToolingEventV1, TOOLING_CONTRACT_VERSION_V1,
    TOOL_RESULT_MAX_BYTES, TOOL_SEARCH_DEFAULT_LIMIT, TOOL_SEARCH_MAX_LIMIT,
    UPRAVA_MCP_LEASE_AUDIENCE,
};

use super::super::*;

const NATIVE_SOURCE_ID: &str = "uprava-native";
const MOCK_SOURCE_ID: &str = "mock-external";
const LINEAR_SOURCE_ID: &str = "linear-remote-mcp";
const LINEAR_INTEGRATION_ID: &str = "integration-linear";
const LINEAR_UPSTREAM_URL: &str = "https://mcp.linear.app/mcp";
const LINEAR_WORKLOAD_NAME: &str = "uprava-linear";
const LINEAR_TOOL_NAMESPACE: &str = "linear";
const TOOL_POLICY_VERSION: &str = "uprava-tool-policy-v1";
const MCP_LEASE_TTL_MINUTES: i64 = 10;
const MANAGED_MCP_LEASE_TTL_HOURS: i64 = 24;
const MANAGED_MCP_LEASE_REFRESH_WINDOW_MINUTES: i64 = 5;
const TOOL_SUMMARY_MAX_BYTES: usize = 2_048;
const TOOL_SEARCH_CURSOR_TTL_SECONDS: i64 = 300;
const EXTERNAL_TOOL_TIMEOUT: Duration = Duration::from_secs(35);
const INTEGRATION_AUTHORIZATION_URL_TIMEOUT: Duration = Duration::from_secs(50);

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug)]
struct ScopedIdentity {
    node_id: NodeId,
    project_id: Option<ProjectId>,
    placement_id: ProjectPlacementId,
    session_state: SessionThreadState,
}

#[derive(Debug)]
struct RankedDefinition {
    definition: ToolDefinition,
    availability: ToolAvailability,
    score: f64,
}

pub(crate) async fn seed_uprava_native_tools(state: &AppState) -> Result<(), AppError> {
    let definitions = native_tool_definitions()?;
    register_tool_definitions(
        state,
        NATIVE_SOURCE_ID,
        ToolSourceKind::UpravaNative,
        "Uprava",
        &definitions,
    )
    .await
}

pub(crate) async fn seed_external_tool_sources(state: &AppState) -> Result<(), AppError> {
    let now = Utc::now();
    sqlx::query(
        r#"
        insert into tool_sources (source_id, source_kind, display_name, enabled, created_at, updated_at)
        values (?1, 'external_mcp', 'Linear remote MCP', 1, ?2, ?2)
        on conflict(source_id) do update set
            source_kind = excluded.source_kind,
            display_name = excluded.display_name,
            enabled = excluded.enabled,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(LINEAR_SOURCE_ID)
    .bind(now)
    .execute(&state.pool)
    .await?;
    Ok(())
}

pub(crate) async fn register_tool_definitions(
    state: &AppState,
    source_id: &str,
    source_kind: ToolSourceKind,
    source_display_name: &str,
    definitions: &[ToolDefinition],
) -> Result<(), AppError> {
    let now = Utc::now();
    let connection = state.pool.acquire().await?;
    // Definition replacement reads and rewrites multiple registry rows. Reserve
    // the SQLite writer before that snapshot so concurrent registry refreshes
    // cannot invalidate it with SQLITE_BUSY_SNAPSHOT.
    let mut transaction =
        sqlx::Transaction::begin(connection, Some("BEGIN IMMEDIATE".into())).await?;
    sqlx::query(
        r#"
        insert into tool_sources (
            source_id, source_kind, display_name, enabled, created_at, updated_at
        ) values (?1, ?2, ?3, 1, ?4, ?4)
        on conflict(source_id) do update set
            source_kind = excluded.source_kind,
            display_name = excluded.display_name,
            enabled = 1,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(source_id)
    .bind(format_tool_source_kind(source_kind))
    .bind(source_display_name)
    .bind(now)
    .execute(&mut *transaction)
    .await?;

    for definition in definitions {
        upsert_tool_definition(&mut transaction, definition, now).await?;
    }
    transaction.commit().await?;
    Ok(())
}

async fn upsert_tool_definition(
    transaction: &mut Transaction<'_, Sqlite>,
    desired: &ToolDefinition,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    let current: Option<(i64, String, DateTime<Utc>)> = sqlx::query_as(
        "select version, schema_hash, created_at from tool_definitions where tool_id = ?1",
    )
    .bind(desired.tool_id.as_str())
    .fetch_optional(&mut **transaction)
    .await?;
    let (version, created_at) = match current {
        Some((version, schema_hash, created_at)) if schema_hash == desired.schema_hash => {
            (version.max(1) as u64, created_at)
        }
        Some((version, _, created_at)) => ((version + 1).max(1) as u64, created_at),
        None => (1, now),
    };
    let mut definition = desired.clone();
    definition.version = version;
    definition.created_at = created_at;
    definition.updated_at = now;
    let definition_json = serde_json::to_string(&definition)?;
    let search_document = normalize_search_document(&definition);
    sqlx::query(
        r#"
        insert into tool_definitions (
            tool_id, source_id, source_tool_name, version, schema_hash, state,
            definition_json, search_document, created_at, updated_at
        ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        on conflict(tool_id) do update set
            source_id = excluded.source_id,
            source_tool_name = excluded.source_tool_name,
            version = excluded.version,
            schema_hash = excluded.schema_hash,
            state = excluded.state,
            definition_json = excluded.definition_json,
            search_document = excluded.search_document,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(definition.tool_id.as_str())
    .bind(definition.source_id.as_str())
    .bind(&definition.source_tool_name)
    .bind(
        i64::try_from(definition.version)
            .map_err(|_| AppError::internal("tool version exceeds SQLite range"))?,
    )
    .bind(&definition.schema_hash)
    .bind(format_tool_definition_state(definition.state))
    .bind(definition_json)
    .bind(search_document)
    .bind(created_at)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(crate) async fn search_tools(
    state: &AppState,
    request: &SearchToolsRequest,
) -> Result<SearchToolsResponse, AppError> {
    state
        .core_metrics
        .tool_registry_searches
        .fetch_add(1, Ordering::Relaxed);
    if !tool_is_visible_to_actor(&request.scope.actor_ref) {
        return Ok(SearchToolsResponse {
            items: vec![],
            next_cursor: None,
        });
    }
    let offset = parse_search_cursor(state, request)?;
    let requested_limit = request.limit.unwrap_or(TOOL_SEARCH_DEFAULT_LIMIT);
    if !(1..=TOOL_SEARCH_MAX_LIMIT).contains(&requested_limit) {
        return Err(AppError::bad_request(
            "tool_search.invalid_limit",
            "Tool search limit must be within the documented bounds",
        ));
    }
    let limit = requested_limit as usize;
    let definitions = load_active_tool_definitions(state).await?;
    let document_terms = definitions
        .iter()
        .map(|definition| tokenize(&normalize_search_document(definition)))
        .collect::<Vec<_>>();
    let query_terms = tokenize(&request.query);
    let document_count = definitions.len() as f64;
    let average_length = if document_terms.is_empty() {
        1.0
    } else {
        document_terms.iter().map(Vec::len).sum::<usize>() as f64 / document_count
    };
    let document_frequencies = query_terms
        .iter()
        .map(|query| {
            let count = document_terms
                .iter()
                .filter(|terms| terms.iter().any(|term| term == query))
                .count();
            (query.clone(), count)
        })
        .collect::<HashMap<_, _>>();
    let mut ranked = Vec::new();
    for (definition, terms) in definitions.into_iter().zip(document_terms) {
        if !definition_matches_filters(&definition, request) {
            continue;
        }
        let availability = effective_tool_availability(state, &definition, &request.scope).await?;
        if !availability_matches_filters(&availability, request) {
            continue;
        }
        let score = bm25_score(
            &query_terms,
            &terms,
            &document_frequencies,
            document_count,
            average_length,
        );
        if query_terms.is_empty() || score > 0.0 {
            ranked.push(RankedDefinition {
                definition,
                availability,
                score,
            });
        }
    }
    ranked.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(CmpOrdering::Equal)
            .then_with(|| {
                left.definition
                    .tool_id
                    .as_str()
                    .cmp(right.definition.tool_id.as_str())
            })
    });
    let has_more = ranked.len() > offset.saturating_add(limit);
    let items = ranked
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|item| ToolSearchResult {
            tool_id: item.definition.tool_id,
            display_name: item.definition.display_name,
            short_description: item.definition.short_description,
            source_kind: item.definition.source_kind,
            risk_level: item.definition.risk_level,
            availability_state: item.availability.state,
            unavailable_reason: item.availability.reason,
            schema_hash: item.definition.schema_hash,
        })
        .collect();
    Ok(SearchToolsResponse {
        items,
        next_cursor: has_more
            .then(|| create_search_cursor(state, request, offset + limit))
            .transpose()?,
    })
}

pub(crate) async fn inspect_tool(
    state: &AppState,
    request: &InspectToolRequest,
) -> Result<InspectToolResponse, AppError> {
    if !tool_is_visible_to_actor(&request.scope.actor_ref) {
        return Err(AppError::not_found("tool.not_found", "Tool not found"));
    }
    let definition = load_tool_definition(state, &request.tool_id).await?;
    let availability = effective_tool_availability(state, &definition, &request.scope).await?;
    persist_session_tool_snapshot(state, &request.scope, &definition).await?;
    Ok(InspectToolResponse {
        definition,
        availability,
        invocation_mode: ToolInvocationMode::StableExecuteTool,
    })
}

pub(crate) async fn execute_tool(
    state: &AppState,
    request: &ExecuteToolRequest,
) -> Result<ExecuteToolResponse, AppError> {
    state
        .core_metrics
        .tool_execution_requests
        .fetch_add(1, Ordering::Relaxed);
    let tool_call_id = ToolCallId::new();
    let correlation_id = CorrelationId::new();
    let definition = match load_tool_definition(state, &request.tool_id).await {
        Ok(definition) => definition,
        Err(AppError::NotFound { .. }) => {
            return Ok(ExecuteToolResponse {
                tool_call_id,
                state: ToolCallState::Denied,
                result: None,
                error: Some(tool_error(
                    ToolExecutionErrorCode::PermissionDenied,
                    "Tool is not visible in this scope",
                    false,
                )),
            });
        }
        Err(error) => return Err(error),
    };
    let policy = policy_decision(&definition, &request.scope);
    insert_tool_call(
        state,
        &tool_call_id,
        &correlation_id,
        &definition,
        request,
        policy,
    )
    .await?;
    append_tool_call_event(
        state,
        &tool_call_id,
        ToolCallState::Requested,
        json!({
            "correlation_id": correlation_id,
            "tool_id": definition.tool_id,
            "schema_hash": definition.schema_hash,
        }),
    )
    .await?;

    if policy == PolicyDecision::Deny {
        state
            .core_metrics
            .tool_policy_denials
            .fetch_add(1, Ordering::Relaxed);
        let error = tool_error(
            ToolExecutionErrorCode::PermissionDenied,
            "Tool policy denied this call",
            false,
        );
        finish_tool_call(
            state,
            &tool_call_id,
            ToolCallState::Denied,
            None,
            Some(&error),
            vec![],
        )
        .await?;
        audit_tool_policy(state, request, &tool_call_id, "denied").await?;
        return Ok(ExecuteToolResponse {
            tool_call_id,
            state: ToolCallState::Denied,
            result: None,
            error: Some(error),
        });
    }
    if policy == PolicyDecision::RequireApproval {
        let error = tool_error(
            ToolExecutionErrorCode::ApprovalRequired,
            "Tool call requires approval",
            false,
        );
        update_tool_call_state(state, &tool_call_id, ToolCallState::ApprovalRequired).await?;
        append_tool_call_event(
            state,
            &tool_call_id,
            ToolCallState::ApprovalRequired,
            serde_json::to_value(&error)?,
        )
        .await?;
        audit_tool_policy(state, request, &tool_call_id, "approval_required").await?;
        return Ok(ExecuteToolResponse {
            tool_call_id,
            state: ToolCallState::ApprovalRequired,
            result: None,
            error: Some(error),
        });
    }

    append_tool_call_event(
        state,
        &tool_call_id,
        ToolCallState::Authorized,
        json!({"policy_version": TOOL_POLICY_VERSION}),
    )
    .await?;
    let availability = effective_tool_availability(state, &definition, &request.scope).await?;
    if availability.state != ToolAvailabilityState::Available {
        state
            .core_metrics
            .tool_execution_failures
            .fetch_add(1, Ordering::Relaxed);
        let error = tool_error(
            ToolExecutionErrorCode::Unavailable,
            "Tool is unavailable in this scope",
            true,
        );
        finish_tool_call(
            state,
            &tool_call_id,
            ToolCallState::Failed,
            None,
            Some(&error),
            vec![],
        )
        .await?;
        return Ok(ExecuteToolResponse {
            tool_call_id,
            state: ToolCallState::Failed,
            result: None,
            error: Some(error),
        });
    }
    if let Err(message) = validate_arguments(&definition.input_schema.0, &request.arguments.0) {
        state
            .core_metrics
            .tool_execution_failures
            .fetch_add(1, Ordering::Relaxed);
        let error = tool_error(ToolExecutionErrorCode::InvalidArguments, &message, false);
        finish_tool_call(
            state,
            &tool_call_id,
            ToolCallState::Failed,
            None,
            Some(&error),
            vec![],
        )
        .await?;
        return Ok(ExecuteToolResponse {
            tool_call_id,
            state: ToolCallState::Failed,
            result: None,
            error: Some(error),
        });
    }
    persist_session_tool_snapshot(state, &request.scope, &definition).await?;
    mark_tool_call_started(state, &tool_call_id).await?;
    let execution =
        execute_tool_route(state, &tool_call_id, &correlation_id, &definition, request).await;
    match execution {
        Ok((mut result, result_refs)) => {
            result.content.0 = redact_json(
                &result.content.0,
                &definition.redaction.result_json_pointers,
                definition.redaction.redact_all_result,
            );
            result.summary = Some(bounded_json_summary(
                &result.content.0,
                definition.redaction.max_summary_bytes as usize,
            ));
            finish_tool_call(
                state,
                &tool_call_id,
                ToolCallState::Completed,
                Some(&result),
                None,
                result_refs,
            )
            .await?;
            Ok(ExecuteToolResponse {
                tool_call_id,
                state: ToolCallState::Completed,
                result: Some(result),
                error: None,
            })
        }
        Err(error) => {
            state
                .core_metrics
                .tool_execution_failures
                .fetch_add(1, Ordering::Relaxed);
            finish_tool_call(
                state,
                &tool_call_id,
                ToolCallState::Failed,
                None,
                Some(&error),
                vec![],
            )
            .await?;
            Ok(ExecuteToolResponse {
                tool_call_id,
                state: ToolCallState::Failed,
                result: None,
                error: Some(error),
            })
        }
    }
}

async fn execute_tool_route(
    state: &AppState,
    tool_call_id: &ToolCallId,
    correlation_id: &CorrelationId,
    definition: &ToolDefinition,
    request: &ExecuteToolRequest,
) -> Result<(ToolResultEnvelope, Vec<UpravaRef>), ToolExecutionError> {
    let arguments = &request.arguments.0;
    let (content, refs) = match definition.tool_id.as_str() {
        "uprava.node.inspect" => {
            let node_id = string_argument(arguments, "node_id")?;
            let node = load_nodes(state)
                .await
                .map_err(internal_tool_error)?
                .into_iter()
                .find(|candidate| candidate.node_id.as_str() == node_id)
                .ok_or_else(|| {
                    tool_error(
                        ToolExecutionErrorCode::BackendFailed,
                        "Node not found",
                        false,
                    )
                })?;
            (
                serde_json::to_value(node).map_err(internal_tool_error)?,
                vec![UpravaRef::Node {
                    node_id: NodeId::from(node_id),
                }],
            )
        }
        "uprava.workspace.inspect" => {
            let placement_id =
                ProjectPlacementId::from(string_argument(arguments, "placement_id")?);
            let placement = load_placement(state, &placement_id)
                .await
                .map_err(app_tool_error)?;
            (
                serde_json::to_value(placement).map_err(internal_tool_error)?,
                vec![UpravaRef::Workspace { placement_id }],
            )
        }
        "uprava.session.inspect" => {
            let session_id =
                SessionThreadId::from(string_argument(arguments, "session_thread_id")?);
            let detail = load_session_detail(state, &session_id)
                .await
                .map_err(app_tool_error)?;
            (
                serde_json::to_value(detail).map_err(internal_tool_error)?,
                vec![UpravaRef::Session {
                    session_thread_id: session_id,
                }],
            )
        }
        "uprava.trace.resolve" => {
            let session_id =
                SessionThreadId::from(string_argument(arguments, "session_thread_id")?);
            let trace = build_session_trace_projection(state, &session_id)
                .await
                .map_err(app_tool_error)?;
            (
                serde_json::to_value(trace).map_err(internal_tool_error)?,
                vec![UpravaRef::Session {
                    session_thread_id: session_id,
                }],
            )
        }
        "uprava.capability.inspect" => {
            let node_id = NodeId::from(string_argument(arguments, "node_id")?);
            let capability_key = arguments.get("capability_key").and_then(Value::as_str);
            let content = inspect_capabilities(state, &node_id, capability_key)
                .await
                .map_err(app_tool_error)?;
            (content, vec![UpravaRef::Node { node_id }])
        }
        "uprava.dynamic_ui.inspect" => (
            super::dynamic_ui::inspect_generated_ui_capabilities(state)
                .await
                .map_err(app_tool_error)?,
            vec![],
        ),
        "uprava.dynamic_ui.create" => {
            let session_thread_id = request.scope.session_thread_id.clone().ok_or_else(|| {
                tool_error(
                    ToolExecutionErrorCode::InvalidArguments,
                    "A session-scoped tool call is required",
                    false,
                )
            })?;
            let mut proposal = arguments.clone();
            proposal
                .as_object_mut()
                .ok_or_else(|| {
                    tool_error(
                        ToolExecutionErrorCode::InvalidArguments,
                        "arguments must be an object",
                        false,
                    )
                })?
                .insert(
                    "scope_ref".to_owned(),
                    serde_json::to_value(ScopeRef::Session {
                        session_thread_id: session_thread_id.clone(),
                    })
                    .map_err(internal_tool_error)?,
                );
            let proposal: CreateDynamicUiProposalRequest = serde_json::from_value(proposal)
                .map_err(|error| {
                    tool_error(
                        ToolExecutionErrorCode::InvalidArguments,
                        &format!("Invalid dynamic UI proposal: {error}"),
                        false,
                    )
                })?;
            let detail = super::dynamic_ui::create_dynamic_ui_proposal_as(
                state,
                proposal,
                request.scope.actor_ref.clone(),
            )
            .await
            .map_err(app_tool_error)?;
            let artifact_id = detail.artifact.artifact.artifact_id.clone();
            (
                serde_json::to_value(detail).map_err(internal_tool_error)?,
                vec![
                    UpravaRef::Artifact { artifact_id },
                    UpravaRef::Session { session_thread_id },
                ],
            )
        }
        _ if definition.source_id.as_str() == MOCK_SOURCE_ID => {
            if arguments.get("fail").and_then(Value::as_bool) == Some(true) {
                return Err(tool_error(
                    ToolExecutionErrorCode::BackendFailed,
                    "Deterministic mock backend failure",
                    true,
                ));
            }
            (json!({"backend": "mock", "arguments": arguments}), vec![])
        }
        _ if definition.execution_kind == ToolExecutionKind::ToolhiveMcp => {
            let result =
                execute_external_tool(state, tool_call_id, correlation_id, definition, request)
                    .await?;
            return Ok((result, vec![]));
        }
        _ => {
            return Err(tool_error(
                ToolExecutionErrorCode::BackendFailed,
                "No Core route for tool",
                false,
            ))
        }
    };
    normalize_tool_result(content, refs)
}

async fn inspect_capabilities(
    state: &AppState,
    node_id: &NodeId,
    capability_key: Option<&str>,
) -> Result<Value, AppError> {
    let rows = sqlx::query(
        "select capability_key, value_json, updated_at from node_capabilities where node_id = ?1 and (?2 is null or capability_key = ?2) order by capability_key",
    )
    .bind(node_id.as_str())
    .bind(capability_key)
    .fetch_all(&state.pool)
    .await?;
    let items = rows
        .into_iter()
        .map(|row| -> Result<Value, AppError> {
            Ok(json!({
                "capability_key": row.try_get::<String, _>("capability_key")?,
                "value": serde_json::from_str::<Value>(&row.try_get::<String, _>("value_json")?)?,
                "updated_at": row.try_get::<DateTime<Utc>, _>("updated_at")?,
            }))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({"node_id": node_id, "items": items}))
}

#[derive(Debug, Deserialize)]
struct ExternalToolCommandResult {
    status: Option<McpDependencyStatus>,
    #[serde(default)]
    definitions: Vec<ToolDefinition>,
    event: Option<ToolingEventV1>,
    #[serde(default)]
    authorization_url: Option<String>,
    #[serde(default)]
    authorization_expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug)]
struct ExternalToolRoute {
    integration_id: IntegrationId,
    dependency_instance_id: McpDependencyInstanceId,
    node_id: NodeId,
}

async fn execute_external_tool(
    state: &AppState,
    tool_call_id: &ToolCallId,
    correlation_id: &CorrelationId,
    definition: &ToolDefinition,
    request: &ExecuteToolRequest,
) -> Result<ToolResultEnvelope, ToolExecutionError> {
    let route = load_external_tool_route(state, definition, &request.scope)
        .await
        .map_err(app_tool_error)?;
    ensure_node_commandable(state, &route.node_id)
        .await
        .map_err(|_| tool_error(ToolExecutionErrorCode::Unavailable, "Node is offline", true))?;
    let command_id = CommandId::new();
    let (result_sender, result_receiver) = oneshot::channel();
    lock_command_waiters(state)
        .map_err(app_tool_error)?
        .insert(command_id.to_string(), result_sender);
    let _waiter_guard = CommandWaiterGuard::new(state, command_id.clone());
    sqlx::query(
        "update tool_calls set command_id = ?1, integration_id = ?2, dependency_instance_id = ?3, route = 'node-toolhive' where tool_call_id = ?4",
    )
    .bind(command_id.as_str())
    .bind(route.integration_id.as_str())
    .bind(route.dependency_instance_id.as_str())
    .bind(tool_call_id.as_str())
    .execute(&state.pool)
    .await
    .map_err(internal_tool_error)?;
    let target = match request.scope.project_placement_id.clone() {
        Some(project_placement_id) => CommandTarget::Placement {
            node_id: route.node_id.clone(),
            project_placement_id,
        },
        None => CommandTarget::Node {
            node_id: route.node_id.clone(),
        },
    };
    let command = CommandEnvelope {
        command_id: command_id.clone(),
        kind: CommandKind::Tooling,
        target,
        actor_ref: request.scope.actor_ref.clone(),
        source_refs: vec![UpravaRef::ToolCall {
            tool_call_id: tool_call_id.to_string(),
        }],
        cause_refs: request
            .scope
            .session_thread_id
            .clone()
            .map(|session_thread_id| UpravaRef::Session { session_thread_id })
            .into_iter()
            .collect(),
        issued_at: Utc::now(),
        correlation_id: correlation_id.clone(),
        payload: CommandPayload::Tooling {
            command: Box::new(ToolingCommandV1 {
                contract_version: TOOLING_CONTRACT_VERSION_V1,
                payload: ToolingCommandPayloadV1::ExecuteExternalTool {
                    tool_call_id: tool_call_id.clone(),
                    tool_id: definition.tool_id.clone(),
                    schema_hash: definition.schema_hash.clone(),
                    integration_id: route.integration_id.clone(),
                    dependency_instance_id: route.dependency_instance_id.clone(),
                    scope: Box::new(request.scope.clone()),
                    arguments: request.arguments.clone(),
                    deadline_at: Utc::now()
                        + ChronoDuration::from_std(EXTERNAL_TOOL_TIMEOUT)
                            .map_err(internal_tool_error)?,
                    max_result_bytes: TOOL_RESULT_MAX_BYTES,
                },
            }),
        },
    };
    record_and_dispatch_command(state, command)
        .await
        .map_err(app_tool_error)?;
    let result =
        match wait_for_command_result(state, result_receiver, &command_id, EXTERNAL_TOOL_TIMEOUT)
            .await
        {
            Ok(result) => result,
            Err(_) => {
                dispatch_tool_call_cancel(
                    state,
                    &route,
                    tool_call_id,
                    correlation_id,
                    "core_timeout",
                )
                .await;
                return Err(tool_error(
                    ToolExecutionErrorCode::Timeout,
                    "Timed out waiting for the external tool",
                    true,
                ));
            }
        };
    if result.status != CommandState::Completed {
        return Err(tool_error(
            ToolExecutionErrorCode::BackendFailed,
            "Node ToolHive bridge failed",
            true,
        ));
    }
    let result: ExternalToolCommandResult =
        serde_json::from_value(result.payload.0).map_err(internal_tool_error)?;
    if result.status.is_some() || !result.definitions.is_empty() {
        return Err(tool_error(
            ToolExecutionErrorCode::BackendFailed,
            "Node returned an unexpected tooling result",
            false,
        ));
    }
    let event = result.event.ok_or_else(|| {
        tool_error(
            ToolExecutionErrorCode::BackendFailed,
            "Node returned no terminal tool event",
            false,
        )
    })?;
    if event.contract_version != TOOLING_CONTRACT_VERSION_V1 {
        return Err(tool_error(
            ToolExecutionErrorCode::BackendFailed,
            "Node returned an incompatible tooling event",
            false,
        ));
    }
    match event.payload {
        ToolingEventPayloadV1::ToolCallCompleted {
            tool_call_id: returned_id,
            result,
            ..
        } if returned_id == *tool_call_id => Ok(result),
        ToolingEventPayloadV1::ToolCallFailed {
            tool_call_id: returned_id,
            error,
            ..
        }
        | ToolingEventPayloadV1::ToolCallDenied {
            tool_call_id: returned_id,
            error,
            ..
        } if returned_id == *tool_call_id => Err(error),
        _ => Err(tool_error(
            ToolExecutionErrorCode::BackendFailed,
            "Node tool result identity did not match the call",
            false,
        )),
    }
}

async fn dispatch_tool_call_cancel(
    state: &AppState,
    route: &ExternalToolRoute,
    tool_call_id: &ToolCallId,
    correlation_id: &CorrelationId,
    reason: &str,
) {
    let command = CommandEnvelope {
        command_id: CommandId::new(),
        kind: CommandKind::Tooling,
        target: CommandTarget::Node {
            node_id: route.node_id.clone(),
        },
        actor_ref: ActorRef::System,
        source_refs: vec![UpravaRef::ToolCall {
            tool_call_id: tool_call_id.to_string(),
        }],
        cause_refs: vec![],
        issued_at: Utc::now(),
        correlation_id: correlation_id.clone(),
        payload: CommandPayload::Tooling {
            command: Box::new(ToolingCommandV1 {
                contract_version: TOOLING_CONTRACT_VERSION_V1,
                payload: ToolingCommandPayloadV1::CancelToolCall {
                    tool_call_id: tool_call_id.clone(),
                    reason: Some(reason.to_owned()),
                },
            }),
        },
    };
    if let Err(error) = record_and_dispatch_command(state, command).await {
        tracing::warn!(error = %error, "tool call cancellation dispatch failed");
    }
}

async fn load_external_tool_route(
    state: &AppState,
    definition: &ToolDefinition,
    scope: &ToolScope,
) -> Result<ExternalToolRoute, AppError> {
    let node_id = scope
        .node_id
        .clone()
        .ok_or_else(|| AppError::bad_request("tool.node_required", "Tool scope requires a Node"))?;
    let row: Option<(String, String, String, String)> = sqlx::query_as(
        r#"
        select ic.integration_id, ic.auth_state, mdi.dependency_instance_id, mdi.actual_state
        from integration_connections ic
        join mcp_dependency_instances mdi on mdi.integration_id = ic.integration_id
        where ic.source_id = ?1 and mdi.node_id = ?2 and ic.desired_state = 'enabled'
        order by ic.updated_at desc
        limit 1
        "#,
    )
    .bind(definition.source_id.as_str())
    .bind(node_id.as_str())
    .fetch_optional(&state.pool)
    .await?;
    let Some((integration_id, auth_state, dependency_instance_id, actual_state)) = row else {
        return Err(AppError::bad_request(
            "tool.dependency_missing",
            "External tool dependency is not configured",
        ));
    };
    if auth_state != "connected" || actual_state != "running" {
        return Err(AppError::bad_request(
            "tool.dependency_unavailable",
            "External tool dependency is not ready",
        ));
    }
    Ok(ExternalToolRoute {
        integration_id: IntegrationId::from(integration_id),
        dependency_instance_id: McpDependencyInstanceId::from(dependency_instance_id),
        node_id,
    })
}

fn normalize_tool_result(
    content: Value,
    result_refs: Vec<UpravaRef>,
) -> Result<(ToolResultEnvelope, Vec<UpravaRef>), ToolExecutionError> {
    let bytes = serde_json::to_vec(&content).map_err(internal_tool_error)?;
    let size = bytes.len() as u64;
    if size > TOOL_RESULT_MAX_BYTES {
        return Err(tool_error(
            ToolExecutionErrorCode::ResultTooLarge,
            "Tool result exceeds the v1 size limit",
            false,
        ));
    }
    let summary = bounded_json_summary(&content, TOOL_SUMMARY_MAX_BYTES);
    Ok((
        ToolResultEnvelope {
            content: JsonValue(content),
            summary: Some(summary),
            truncated: false,
            original_size_bytes: Some(size),
            artifact_refs: result_refs.clone(),
        },
        result_refs,
    ))
}

pub(crate) async fn list_tool_definitions(
    state: &AppState,
) -> Result<Vec<ToolDefinition>, AppError> {
    load_active_tool_definitions(state).await
}

pub(crate) async fn tool_definitions_route(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ToolDefinitionsResponse>, AppError> {
    Ok(Json(ToolDefinitionsResponse {
        items: list_tool_definitions(&state).await?,
        next_cursor: None,
    }))
}

pub(crate) async fn tool_definition_route(
    State(state): State<Arc<AppState>>,
    Path(tool_id): Path<String>,
) -> Result<Json<ToolDefinition>, AppError> {
    get_tool_definition(&state, &ToolId::from(tool_id))
        .await
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub(crate) struct ToolAvailabilityQuery {
    node_id: String,
    project_id: Option<String>,
    project_placement_id: String,
    session_thread_id: String,
}

pub(crate) async fn tool_availability_route(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ToolAvailabilityQuery>,
) -> Result<Json<ToolAvailabilityResponse>, AppError> {
    let scope = ToolScope {
        actor_ref: ActorRef::local_user(),
        node_id: Some(NodeId::from(query.node_id)),
        project_id: query.project_id.map(ProjectId::from),
        project_placement_id: Some(ProjectPlacementId::from(query.project_placement_id)),
        session_thread_id: Some(SessionThreadId::from(query.session_thread_id)),
    };
    Ok(Json(ToolAvailabilityResponse {
        items: list_tool_availability(&state, &scope).await?,
        generated_at: Utc::now(),
    }))
}

#[derive(Debug, Deserialize)]
pub(crate) struct ToolCallsQuery {
    pub(crate) limit: Option<usize>,
    pub(crate) node_id: Option<String>,
    pub(crate) project_id: Option<String>,
    pub(crate) project_placement_id: Option<String>,
    pub(crate) session_thread_id: Option<String>,
}

pub(crate) async fn tool_calls_route(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ToolCallsQuery>,
) -> Result<Json<ToolCallsResponse>, AppError> {
    let limit = query.limit.unwrap_or(50).min(100);
    let items = list_tool_calls(&state, 100)
        .await?
        .into_iter()
        .filter(|item| tool_call_matches_query(item, &query))
        .take(limit)
        .collect();
    Ok(Json(ToolCallsResponse {
        items,
        next_cursor: None,
    }))
}

pub(crate) fn tool_call_matches_query(item: &ToolCallSummary, query: &ToolCallsQuery) -> bool {
    query.node_id.as_deref().is_none_or(|value| {
        item.scope
            .node_id
            .as_ref()
            .is_some_and(|id| id.as_str() == value)
    }) && query.project_id.as_deref().is_none_or(|value| {
        item.scope
            .project_id
            .as_ref()
            .is_some_and(|id| id.as_str() == value)
    }) && query.project_placement_id.as_deref().is_none_or(|value| {
        item.scope
            .project_placement_id
            .as_ref()
            .is_some_and(|id| id.as_str() == value)
    }) && query.session_thread_id.as_deref().is_none_or(|value| {
        item.scope
            .session_thread_id
            .as_ref()
            .is_some_and(|id| id.as_str() == value)
    })
}

pub(crate) async fn tool_call_detail_route(
    State(state): State<Arc<AppState>>,
    Path(tool_call_id): Path<String>,
) -> Result<Json<ToolCallDetail>, AppError> {
    load_tool_call_detail(&state, &ToolCallId::from(tool_call_id))
        .await
        .map(Json)
}

pub(crate) async fn observed_capabilities_route(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
) -> Result<Json<ObservedCapabilitiesResponse>, AppError> {
    let node_id = NodeId::from(node_id);
    let rows = sqlx::query(
        "select capability_json from observed_capabilities where node_id = ?1 order by capability_key",
    )
    .bind(node_id.as_str())
    .fetch_all(&state.pool)
    .await?;
    let items = rows
        .into_iter()
        .map(|row| {
            serde_json::from_str::<ObservedCapability>(
                &row.try_get::<String, _>("capability_json")?,
            )
            .map_err(AppError::from)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(ObservedCapabilitiesResponse {
        items,
        generated_at: Utc::now(),
    }))
}

pub(crate) async fn integration_connections_route(
    State(state): State<Arc<AppState>>,
) -> Result<Json<IntegrationConnectionsResponse>, AppError> {
    let rows = sqlx::query(
        "select connection_json from integration_connections order by updated_at desc, integration_id",
    )
    .fetch_all(&state.pool)
    .await?;
    let items = rows
        .into_iter()
        .map(|row| {
            serde_json::from_str::<IntegrationConnectionSummary>(
                &row.try_get::<String, _>("connection_json")?,
            )
            .map_err(AppError::from)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(IntegrationConnectionsResponse { items }))
}

pub(crate) async fn connect_integration_route(
    State(state): State<Arc<AppState>>,
    Json(request): Json<IntegrationConnectRequest>,
) -> Result<Json<IntegrationConnectResponse>, AppError> {
    if request.integration_id.as_str() != LINEAR_INTEGRATION_ID {
        return Err(AppError::bad_request(
            "integration.provider_unsupported",
            "Only the pinned Linear integration is supported",
        ));
    }
    ensure_node_commandable(&state, &request.node_id).await?;
    let existing: Option<(String, i64)> = sqlx::query_as(
        "select connection_json, credential_generation from integration_connections where integration_id = ?1",
    )
    .bind(request.integration_id.as_str())
    .fetch_optional(&state.pool)
    .await?;
    if let Some((raw, _)) = existing.as_ref() {
        let connection: IntegrationConnectionSummary = serde_json::from_str(raw)?;
        if connection.node_id.as_ref() != Some(&request.node_id) {
            return Err(AppError::bad_request(
                "integration.node_change_unsupported",
                "Reconnect Linear on its existing Node",
            ));
        }
    }
    let dependency_instance_id = sqlx::query_scalar::<_, String>(
        "select dependency_instance_id from mcp_dependency_instances where integration_id = ?1 and node_id = ?2",
    )
    .bind(request.integration_id.as_str())
    .bind(request.node_id.as_str())
    .fetch_optional(&state.pool)
    .await?
    .map(McpDependencyInstanceId::from)
    .unwrap_or_default();
    let now = Utc::now();
    let connection = IntegrationConnectionSummary {
        integration_id: request.integration_id.clone(),
        source_id: ToolSourceId::from(LINEAR_SOURCE_ID),
        provider: "linear".to_owned(),
        display_name: "Linear".to_owned(),
        desired_state: IntegrationDesiredState::Enabled,
        auth_state: IntegrationAuthState::Connecting,
        node_id: Some(request.node_id.clone()),
        authenticated_actor_label: None,
        connected_at: None,
        updated_at: now,
        error_code: None,
    };
    let dependency_status = McpDependencyStatus {
        dependency_instance_id: dependency_instance_id.clone(),
        integration_id: request.integration_id.clone(),
        node_id: request.node_id.clone(),
        desired_state: IntegrationDesiredState::Enabled,
        actual_state: McpDependencyActualState::Starting,
        runtime_name: "toolhive".to_owned(),
        runtime_version: None,
        upstream_identity: Some(LINEAR_SOURCE_ID.to_owned()),
        schema_set_hash: None,
        error_code: None,
        observed_at: now,
    };
    let credential_generation = existing
        .as_ref()
        .map_or(1_i64, |(_, generation)| generation.saturating_add(1));
    let mut transaction = state.pool.begin().await?;
    sqlx::query(
        r#"
        insert into integration_connections (
            integration_id, source_id, provider, desired_state, auth_state,
            node_id, connection_json, credential_generation, created_at, updated_at
        ) values (?1, ?2, 'linear', 'enabled', 'connecting', ?3, ?4, ?5, ?6, ?6)
        on conflict(integration_id) do update set
            desired_state = 'enabled', auth_state = 'connecting', node_id = excluded.node_id,
            connection_json = excluded.connection_json,
            credential_generation = excluded.credential_generation, updated_at = excluded.updated_at
        "#,
    )
    .bind(request.integration_id.as_str())
    .bind(LINEAR_SOURCE_ID)
    .bind(request.node_id.as_str())
    .bind(serde_json::to_string(&connection)?)
    .bind(credential_generation)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        insert into mcp_dependency_instances (
            dependency_instance_id, integration_id, node_id, desired_state,
            actual_state, status_json, updated_at
        ) values (?1, ?2, ?3, 'enabled', 'starting', ?4, ?5)
        on conflict(dependency_instance_id) do update set
            integration_id = excluded.integration_id, node_id = excluded.node_id,
            desired_state = 'enabled', actual_state = 'starting',
            status_json = excluded.status_json, updated_at = excluded.updated_at
        "#,
    )
    .bind(dependency_instance_id.as_str())
    .bind(request.integration_id.as_str())
    .bind(request.node_id.as_str())
    .bind(serde_json::to_string(&dependency_status)?)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    let command_id = CommandId::new();
    let (result_sender, result_receiver) = oneshot::channel();
    lock_command_waiters(&state)?.insert(command_id.to_string(), result_sender);
    let _waiter_guard = CommandWaiterGuard::new(&state, command_id.clone());
    let command = CommandEnvelope {
        command_id: command_id.clone(),
        kind: CommandKind::Tooling,
        target: CommandTarget::Node {
            node_id: request.node_id.clone(),
        },
        actor_ref: ActorRef::System,
        source_refs: vec![],
        cause_refs: vec![],
        issued_at: now,
        correlation_id: CorrelationId::new(),
        payload: CommandPayload::Tooling {
            command: Box::new(ToolingCommandV1 {
                contract_version: TOOLING_CONTRACT_VERSION_V1,
                payload: ToolingCommandPayloadV1::BeginIntegrationAuthorization {
                    dependency_instance_id,
                    integration_id: request.integration_id.clone(),
                    upstream_url: LINEAR_UPSTREAM_URL.to_owned(),
                    workload_name: LINEAR_WORKLOAD_NAME.to_owned(),
                    tool_namespace: LINEAR_TOOL_NAMESPACE.to_owned(),
                },
            }),
        },
    };
    if let Err(error) = record_and_dispatch_command(&state, command).await {
        mark_integration_authorization_error(&state, &request.integration_id).await?;
        return Err(error);
    }
    let result = match wait_for_command_result(
        &state,
        result_receiver,
        &command_id,
        INTEGRATION_AUTHORIZATION_URL_TIMEOUT,
    )
    .await
    {
        Ok(result) if result.status == CommandState::Completed => result,
        Ok(_) => {
            mark_integration_authorization_error(&state, &request.integration_id).await?;
            return Err(AppError::bad_request(
                "integration.authorization_failed",
                "Node could not begin Linear authorization",
            ));
        }
        Err(error) => {
            mark_integration_authorization_error(&state, &request.integration_id).await?;
            return Err(error);
        }
    };
    let result: ExternalToolCommandResult = serde_json::from_value(result.payload.0)?;
    let authorization_url = result.authorization_url.ok_or_else(|| {
        AppError::bad_request(
            "integration.authorization_url_missing",
            "Node did not return a Linear authorization URL",
        )
    })?;
    let expires_at = result.authorization_expires_at.ok_or_else(|| {
        AppError::bad_request(
            "integration.authorization_expiry_missing",
            "Node did not return an authorization expiry",
        )
    })?;
    validate_linear_authorization_url(&authorization_url, expires_at)?;
    audit_security_event(
        &state,
        "integration.authorization_started",
        Some(&request.node_id),
        None,
        "accepted",
        JsonValue(json!({
            "integration_id": request.integration_id,
            "project_id": request.project_id,
            "expires_at": expires_at,
        })),
    )
    .await?;
    Ok(Json(IntegrationConnectResponse {
        connection,
        authorization_url,
        expires_at,
    }))
}

async fn mark_integration_authorization_error(
    state: &AppState,
    integration_id: &IntegrationId,
) -> Result<(), AppError> {
    let raw: Option<String> = sqlx::query_scalar(
        "select connection_json from integration_connections where integration_id = ?1",
    )
    .bind(integration_id.as_str())
    .fetch_optional(&state.pool)
    .await?;
    let Some(raw) = raw else {
        return Ok(());
    };
    let mut connection: IntegrationConnectionSummary = serde_json::from_str(&raw)?;
    connection.auth_state = IntegrationAuthState::Error;
    connection.error_code = Some("authorization_start_failed".to_owned());
    connection.updated_at = Utc::now();
    sqlx::query(
        "update integration_connections set auth_state = 'error', connection_json = ?1, updated_at = ?2 where integration_id = ?3",
    )
    .bind(serde_json::to_string(&connection)?)
    .bind(connection.updated_at)
    .bind(integration_id.as_str())
    .execute(&state.pool)
    .await?;
    Ok(())
}

pub(crate) fn validate_linear_authorization_url(
    authorization_url: &str,
    expires_at: DateTime<Utc>,
) -> Result<(), AppError> {
    let url = authorization_url.parse::<axum::http::Uri>().map_err(|_| {
        AppError::bad_request(
            "integration.authorization_url_invalid",
            "Node returned an invalid authorization URL",
        )
    })?;
    let host_allowed = url
        .host()
        .is_some_and(|host| host == "linear.app" || host.ends_with(".linear.app"));
    let has_state = url
        .query()
        .is_some_and(|query| query.split('&').any(|part| part.starts_with("state=")));
    if url.scheme_str() != Some("https")
        || !host_allowed
        || !has_state
        || expires_at <= Utc::now()
        || expires_at > Utc::now() + ChronoDuration::minutes(10)
    {
        return Err(AppError::bad_request(
            "integration.authorization_url_invalid",
            "Node returned an invalid authorization URL",
        ));
    }
    Ok(())
}

pub(crate) async fn disconnect_integration_route(
    State(state): State<Arc<AppState>>,
    Path(integration_id): Path<String>,
    Json(_request): Json<IntegrationDisconnectRequest>,
) -> Result<Json<IntegrationDisconnectResponse>, AppError> {
    let integration_id = IntegrationId::from(integration_id);
    let raw: String = sqlx::query_scalar(
        "select connection_json from integration_connections where integration_id = ?1",
    )
    .bind(integration_id.as_str())
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("integration.not_found", "Integration not found"))?;
    let mut connection: IntegrationConnectionSummary = serde_json::from_str(&raw)?;
    connection.desired_state = IntegrationDesiredState::Disabled;
    connection.auth_state = IntegrationAuthState::Disconnected;
    connection.authenticated_actor_label = None;
    connection.error_code = None;
    connection.updated_at = Utc::now();
    let mut transaction = state.pool.begin().await?;
    sqlx::query(
        "update integration_connections set desired_state = 'disabled', auth_state = 'disconnected', connection_json = ?1, credential_generation = credential_generation + 1, updated_at = ?2 where integration_id = ?3",
    )
    .bind(serde_json::to_string(&connection)?)
    .bind(connection.updated_at)
    .bind(integration_id.as_str())
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "update mcp_dependency_instances set desired_state = 'disabled', updated_at = ?1 where integration_id = ?2",
    )
    .bind(connection.updated_at)
    .bind(integration_id.as_str())
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    if let Some(node_id) = connection.node_id.as_ref() {
        dispatch_dependency_desired_snapshots(&state, node_id).await?;
    }
    audit_security_event(
        &state,
        "integration.disconnected",
        connection.node_id.as_ref(),
        None,
        "accepted",
        JsonValue(json!({"integration_id": integration_id})),
    )
    .await?;
    Ok(Json(IntegrationDisconnectResponse {
        connection,
        remote_revocation_confirmed: false,
    }))
}

pub(crate) async fn mcp_dependency_statuses_route(
    State(state): State<Arc<AppState>>,
) -> Result<Json<McpDependencyStatusesResponse>, AppError> {
    let rows = sqlx::query(
        "select status_json from mcp_dependency_instances order by updated_at desc, dependency_instance_id",
    )
    .fetch_all(&state.pool)
    .await?;
    let items = rows
        .into_iter()
        .map(|row| {
            serde_json::from_str::<McpDependencyStatus>(&row.try_get::<String, _>("status_json")?)
                .map_err(AppError::from)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(McpDependencyStatusesResponse {
        items,
        generated_at: Utc::now(),
    }))
}

pub(crate) async fn project_tooling_command_result(
    state: &AppState,
    command_id: &CommandId,
    status: CommandState,
    payload: &JsonValue,
    project_terminal_call: bool,
) -> Result<(), AppError> {
    let command_json: Option<String> =
        sqlx::query_scalar("select command_json from commands where command_id = ?1")
            .bind(command_id.as_str())
            .fetch_optional(&state.pool)
            .await?;
    let Some(command_json) = command_json else {
        return Ok(());
    };
    let command: CommandEnvelope = serde_json::from_str(&command_json)?;
    if command.kind != CommandKind::Tooling || status != CommandState::Completed {
        return Ok(());
    }
    let result: ExternalToolCommandResult = serde_json::from_value(payload.0.clone())?;
    if let Some(mut dependency_status) = result.status {
        dependency_status.node_id = command.target.node_id().clone();
        persist_dependency_status(state, &dependency_status).await?;
    }
    if !result.definitions.is_empty() {
        if result.definitions.iter().any(|definition| {
            definition.source_id.as_str() != LINEAR_SOURCE_ID
                || definition.source_kind != ToolSourceKind::ExternalMcp
                || definition.execution_kind != ToolExecutionKind::ToolhiveMcp
        }) {
            return Err(AppError::bad_request(
                "tool.definition_source_invalid",
                "Node returned an invalid external tool definition",
            ));
        }
        register_tool_definitions(
            state,
            LINEAR_SOURCE_ID,
            ToolSourceKind::ExternalMcp,
            "Linear remote MCP",
            &result.definitions,
        )
        .await?;
    }
    if project_terminal_call {
        if let Some(event) = result.event {
            project_terminal_tooling_event(state, event).await?;
        }
    }
    Ok(())
}

pub(crate) async fn persist_dependency_status(
    state: &AppState,
    status: &McpDependencyStatus,
) -> Result<(), AppError> {
    if matches!(
        status.actual_state,
        McpDependencyActualState::ToolhiveMissing
            | McpDependencyActualState::MissingAuth
            | McpDependencyActualState::Degraded
            | McpDependencyActualState::Failed
    ) {
        state
            .core_metrics
            .tool_dependency_errors
            .fetch_add(1, Ordering::Relaxed);
    }
    sqlx::query(
        r#"
        insert into mcp_dependency_instances (
            dependency_instance_id, integration_id, node_id, desired_state,
            actual_state, status_json, updated_at
        ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        on conflict(dependency_instance_id) do update set
            integration_id = excluded.integration_id,
            node_id = excluded.node_id,
            desired_state = excluded.desired_state,
            actual_state = excluded.actual_state,
            status_json = excluded.status_json,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(status.dependency_instance_id.as_str())
    .bind(status.integration_id.as_str())
    .bind(status.node_id.as_str())
    .bind(format_integration_desired_state(status.desired_state))
    .bind(format_dependency_actual_state(status.actual_state))
    .bind(serde_json::to_string(status)?)
    .bind(status.observed_at)
    .execute(&state.pool)
    .await?;
    update_connection_from_dependency_status(state, status).await?;
    Ok(())
}

async fn update_connection_from_dependency_status(
    state: &AppState,
    status: &McpDependencyStatus,
) -> Result<(), AppError> {
    let raw: Option<String> = sqlx::query_scalar(
        "select connection_json from integration_connections where integration_id = ?1",
    )
    .bind(status.integration_id.as_str())
    .fetch_optional(&state.pool)
    .await?;
    let Some(raw) = raw else {
        return Ok(());
    };
    let mut connection: IntegrationConnectionSummary = serde_json::from_str(&raw)?;
    connection.updated_at = status.observed_at;
    connection.error_code = status.error_code.clone();
    connection.auth_state = match status.actual_state {
        McpDependencyActualState::Running => {
            connection.connected_at.get_or_insert(status.observed_at);
            IntegrationAuthState::Connected
        }
        McpDependencyActualState::MissingAuth => IntegrationAuthState::Disconnected,
        McpDependencyActualState::Failed => IntegrationAuthState::Error,
        McpDependencyActualState::ToolhiveMissing
        | McpDependencyActualState::Installing
        | McpDependencyActualState::Starting
        | McpDependencyActualState::Degraded
        | McpDependencyActualState::Stopped => connection.auth_state,
    };
    sqlx::query(
        "update integration_connections set auth_state = ?1, connection_json = ?2, updated_at = ?3 where integration_id = ?4",
    )
    .bind(format_integration_auth_state(connection.auth_state))
    .bind(serde_json::to_string(&connection)?)
    .bind(connection.updated_at)
    .bind(status.integration_id.as_str())
    .execute(&state.pool)
    .await?;
    Ok(())
}

async fn project_terminal_tooling_event(
    state: &AppState,
    event: ToolingEventV1,
) -> Result<(), AppError> {
    if event.contract_version != TOOLING_CONTRACT_VERSION_V1 {
        return Err(AppError::bad_request(
            "tool.event_version_invalid",
            "Node tooling event version is incompatible",
        ));
    }
    match event.payload {
        ToolingEventPayloadV1::ToolCallCompleted {
            tool_call_id,
            result,
            ..
        } => {
            finish_tool_call(
                state,
                &tool_call_id,
                ToolCallState::Completed,
                Some(&result),
                None,
                result.artifact_refs.clone(),
            )
            .await?;
        }
        ToolingEventPayloadV1::ToolCallFailed {
            tool_call_id,
            error,
            ..
        } => {
            let terminal_state = match error.code {
                ToolExecutionErrorCode::Cancelled => ToolCallState::Cancelled,
                ToolExecutionErrorCode::Timeout => ToolCallState::TimedOut,
                _ => ToolCallState::Failed,
            };
            finish_tool_call(
                state,
                &tool_call_id,
                terminal_state,
                None,
                Some(&error),
                vec![],
            )
            .await?;
        }
        ToolingEventPayloadV1::ToolCallDenied {
            tool_call_id,
            error,
            ..
        } => {
            finish_tool_call(
                state,
                &tool_call_id,
                ToolCallState::Denied,
                None,
                Some(&error),
                vec![],
            )
            .await?;
        }
        _ => {}
    }
    Ok(())
}

pub(crate) async fn dispatch_dependency_desired_snapshots(
    state: &AppState,
    node_id: &NodeId,
) -> Result<(), AppError> {
    let rows = sqlx::query(
        r#"
        select mdi.dependency_instance_id, mdi.integration_id, mdi.desired_state,
               ic.auth_state
        from mcp_dependency_instances mdi
        join integration_connections ic on ic.integration_id = mdi.integration_id
        where mdi.node_id = ?1
        order by mdi.dependency_instance_id
        "#,
    )
    .bind(node_id.as_str())
    .fetch_all(&state.pool)
    .await?;
    for row in rows {
        let dependency_instance_id =
            McpDependencyInstanceId::from(row.try_get::<String, _>("dependency_instance_id")?);
        let integration_id = IntegrationId::from(row.try_get::<String, _>("integration_id")?);
        let desired_state =
            parse_integration_desired_state(&row.try_get::<String, _>("desired_state")?);
        let auth_state: String = row.try_get("auth_state")?;
        let command = CommandEnvelope {
            command_id: CommandId::new(),
            kind: CommandKind::Tooling,
            target: CommandTarget::Node {
                node_id: node_id.clone(),
            },
            actor_ref: ActorRef::System,
            source_refs: vec![],
            cause_refs: vec![],
            issued_at: Utc::now(),
            correlation_id: CorrelationId::new(),
            payload: CommandPayload::Tooling {
                command: Box::new(ToolingCommandV1 {
                    contract_version: TOOLING_CONTRACT_VERSION_V1,
                    payload: ToolingCommandPayloadV1::UpdateDependencyDesiredState {
                        dependency_instance_id,
                        integration_id,
                        desired_state,
                        credential_ref: (auth_state == "connected")
                            .then(|| "toolhive:managed-linear".to_owned()),
                        upstream_url: "https://mcp.linear.app/mcp".to_owned(),
                        workload_name: "uprava-linear".to_owned(),
                        tool_namespace: "linear".to_owned(),
                    },
                }),
            },
        };
        record_and_dispatch_command(state, command).await?;
    }
    Ok(())
}

pub(crate) async fn get_tool_definition(
    state: &AppState,
    tool_id: &ToolId,
) -> Result<ToolDefinition, AppError> {
    load_tool_definition(state, tool_id).await
}

pub(crate) async fn list_tool_availability(
    state: &AppState,
    scope: &ToolScope,
) -> Result<Vec<ToolAvailability>, AppError> {
    let definitions = load_active_tool_definitions(state).await?;
    let mut items = Vec::with_capacity(definitions.len());
    for definition in definitions {
        items.push(effective_tool_availability(state, &definition, scope).await?);
    }
    Ok(items)
}

pub(crate) async fn list_tool_calls(
    state: &AppState,
    limit: usize,
) -> Result<Vec<ToolCallSummary>, AppError> {
    let rows =
        sqlx::query("select * from tool_calls order by requested_at desc, tool_call_id limit ?1")
            .bind(i64::try_from(limit.min(100)).unwrap_or(100))
            .fetch_all(&state.pool)
            .await?;
    rows.into_iter()
        .map(|row| row_to_tool_call_summary(&row))
        .collect()
}

pub(crate) async fn load_tool_call_detail(
    state: &AppState,
    tool_call_id: &ToolCallId,
) -> Result<ToolCallDetail, AppError> {
    let row = sqlx::query("select * from tool_calls where tool_call_id = ?1")
        .bind(tool_call_id.as_str())
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("tool_call.not_found", "Tool call not found"))?;
    let summary = row_to_tool_call_summary(&row)?;
    Ok(ToolCallDetail {
        summary,
        command_id: row
            .try_get::<Option<String>, _>("command_id")?
            .map(CommandId::from),
        integration_id: row
            .try_get::<Option<String>, _>("integration_id")?
            .map(IntegrationId::from),
        dependency_instance_id: row
            .try_get::<Option<String>, _>("dependency_instance_id")?
            .map(McpDependencyInstanceId::from),
        policy_version: row.try_get("policy_version")?,
        redacted_arguments_summary: row.try_get("redacted_arguments_summary")?,
        redacted_result_summary: row.try_get("redacted_result_summary")?,
        argument_hash: row.try_get("argument_hash")?,
        result_hash: row.try_get("result_hash")?,
        result_size_bytes: row
            .try_get::<Option<i64>, _>("result_size_bytes")?
            .map(|value| value.max(0) as u64),
        trace_refs: serde_json::from_str(&row.try_get::<String, _>("trace_refs_json")?)?,
        result_refs: serde_json::from_str(&row.try_get::<String, _>("result_refs_json")?)?,
        error: row
            .try_get::<Option<String>, _>("error_json")?
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?,
    })
}

pub(crate) async fn issue_mcp_access_lease(
    state: &AppState,
    session_id: &SessionThreadId,
    actor_ref: ActorRef,
) -> Result<(String, McpAccessLeaseClaims), AppError> {
    issue_mcp_access_lease_with_state(
        state,
        session_id,
        actor_ref,
        None,
        ChronoDuration::minutes(MCP_LEASE_TTL_MINUTES),
        false,
    )
    .await
}

pub(crate) async fn issue_managed_mcp_access_lease(
    state: &AppState,
    session_id: &SessionThreadId,
    actor_ref: ActorRef,
) -> Result<(String, McpAccessLeaseClaims), AppError> {
    issue_mcp_access_lease_with_state(
        state,
        session_id,
        actor_ref,
        None,
        ChronoDuration::hours(MANAGED_MCP_LEASE_TTL_HOURS),
        true,
    )
    .await
}

pub(crate) async fn issue_mcp_access_lease_for_resume(
    state: &AppState,
    session_id: &SessionThreadId,
    actor_ref: ActorRef,
    resume_command_id: &CommandId,
) -> Result<(String, McpAccessLeaseClaims), AppError> {
    issue_mcp_access_lease_with_state(
        state,
        session_id,
        actor_ref,
        Some(resume_command_id.clone()),
        ChronoDuration::hours(MANAGED_MCP_LEASE_TTL_HOURS),
        true,
    )
    .await
}

async fn issue_mcp_access_lease_with_state(
    state: &AppState,
    session_id: &SessionThreadId,
    actor_ref: ActorRef,
    resume_command_id: Option<CommandId>,
    ttl: ChronoDuration,
    reuse_active: bool,
) -> Result<(String, McpAccessLeaseClaims), AppError> {
    if !tool_is_visible_to_actor(&actor_ref) {
        return Err(AppError::auth(
            "mcp_lease.actor_denied",
            "Actor cannot receive an MCP lease",
        ));
    }
    let scope = load_scoped_identity_for_session(state, session_id).await?;
    if matches!(scope.session_state, SessionThreadState::Stopped) && resume_command_id.is_none() {
        return Err(AppError::auth(
            "mcp_lease.session_stopped",
            "Stopped session cannot receive an MCP lease",
        ));
    }
    let now = Utc::now();
    if reuse_active && resume_command_id.is_none() {
        let reusable_after =
            now + ChronoDuration::minutes(MANAGED_MCP_LEASE_REFRESH_WINDOW_MINUTES);
        let claims_json: Option<String> = sqlx::query_scalar(
            r#"
            select claims_json from mcp_access_leases
            where session_thread_id = ?1 and revoked_at is null and expires_at > ?2
            order by credential_version desc
            limit 1
            "#,
        )
        .bind(session_id.as_str())
        .bind(reusable_after)
        .fetch_optional(&state.pool)
        .await?;
        if let Some(claims_json) = claims_json {
            let claims: McpAccessLeaseClaims = serde_json::from_str(&claims_json)?;
            if claims.actor_ref == actor_ref {
                let signature = sign_lease(state, claims_json.as_bytes())?;
                return Ok((
                    format!("{}.{}", claims.lease_id, encode_hex(&signature)),
                    claims,
                ));
            }
        }
    }
    let expires_at = now + ttl;
    // Lease rotation reads the current credential version before revoking and
    // inserting. A deferred SQLite transaction can lose that read snapshot to
    // a concurrent rotation and fail with SQLITE_BUSY_SNAPSHOT. Reserve the
    // writer up front; SQLite's configured busy timeout provides bounded wait.
    let connection = state.pool.acquire().await?;
    let mut transaction =
        sqlx::Transaction::begin(connection, Some("BEGIN IMMEDIATE".into())).await?;
    let credential_version: i64 = sqlx::query_scalar(
        "select coalesce(max(credential_version), 0) + 1 from mcp_access_leases where session_thread_id = ?1",
    )
    .bind(session_id.as_str())
    .fetch_one(&mut *transaction)
    .await?;
    sqlx::query(
        "update mcp_access_leases set revoked_at = ?1, revocation_reason = 'rotated' where session_thread_id = ?2 and revoked_at is null",
    )
    .bind(now)
    .bind(session_id.as_str())
    .execute(&mut *transaction)
    .await?;
    let claims = McpAccessLeaseClaims {
        lease_id: McpAccessLeaseId::new(),
        audience: UPRAVA_MCP_LEASE_AUDIENCE.to_owned(),
        actor_ref,
        session_thread_id: session_id.clone(),
        project_id: scope.project_id,
        project_placement_id: scope.placement_id,
        node_id: scope.node_id,
        issued_at: now,
        expires_at,
        credential_version: credential_version as u64,
        resume_command_id,
    };
    let claims_json = serde_json::to_string(&claims)?;
    sqlx::query(
        "insert into mcp_access_leases (lease_id, session_thread_id, claims_json, credential_version, issued_at, expires_at) values (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(claims.lease_id.as_str())
    .bind(session_id.as_str())
    .bind(&claims_json)
    .bind(credential_version)
    .bind(now)
    .bind(expires_at)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    state
        .core_metrics
        .mcp_leases_issued
        .fetch_add(1, Ordering::Relaxed);
    let signature = sign_lease(state, claims_json.as_bytes())?;
    Ok((
        format!("{}.{}", claims.lease_id, encode_hex(&signature)),
        claims,
    ))
}

pub(crate) async fn validate_mcp_access_lease(
    state: &AppState,
    token: &str,
) -> Result<McpAccessLeaseClaims, ToolExecutionError> {
    let (lease_id, signature_hex) = token.split_once('.').ok_or_else(|| {
        tool_error(
            ToolExecutionErrorCode::NotAuthenticated,
            "MCP lease token has an invalid format",
            false,
        )
    })?;
    let signature = decode_hex(signature_hex).ok_or_else(|| {
        tool_error(
            ToolExecutionErrorCode::NotAuthenticated,
            "MCP lease signature encoding is invalid",
            false,
        )
    })?;
    let row = sqlx::query(
        "select claims_json, credential_version, expires_at, revoked_at from mcp_access_leases where lease_id = ?1",
    )
    .bind(lease_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal_tool_error)?
    .ok_or_else(|| {
        tool_error(
            ToolExecutionErrorCode::NotAuthenticated,
            "MCP lease id is unknown",
            false,
        )
    })?;
    if row
        .try_get::<Option<DateTime<Utc>>, _>("revoked_at")
        .map_err(internal_tool_error)?
        .is_some()
    {
        return Err(tool_error(
            ToolExecutionErrorCode::LeaseRevoked,
            "MCP lease was revoked",
            false,
        ));
    }
    let claims_json: String = row.try_get("claims_json").map_err(internal_tool_error)?;
    verify_lease_signature(state, claims_json.as_bytes(), &signature)?;
    let claims: McpAccessLeaseClaims =
        serde_json::from_str(&claims_json).map_err(internal_tool_error)?;
    if claims.audience != UPRAVA_MCP_LEASE_AUDIENCE {
        return Err(tool_error(
            ToolExecutionErrorCode::NotAuthenticated,
            "Invalid MCP lease audience",
            false,
        ));
    }
    let expires_at: DateTime<Utc> = row.try_get("expires_at").map_err(internal_tool_error)?;
    if expires_at <= Utc::now() || claims.expires_at <= Utc::now() {
        return Err(tool_error(
            ToolExecutionErrorCode::LeaseExpired,
            "MCP lease expired",
            false,
        ));
    }
    let credential_version: i64 = row
        .try_get("credential_version")
        .map_err(internal_tool_error)?;
    if claims.credential_version != credential_version as u64 {
        return Err(tool_error(
            ToolExecutionErrorCode::LeaseRevoked,
            "MCP lease credential version is stale",
            false,
        ));
    }
    let identity = load_scoped_identity_for_session(state, &claims.session_thread_id)
        .await
        .map_err(app_tool_error)?;
    let stopped_resume_is_active = if matches!(identity.session_state, SessionThreadState::Stopped)
    {
        if let Some(command_id) = claims.resume_command_id.as_ref() {
            sqlx::query_scalar::<_, i64>(
                r#"
                select count(*)
                from commands
                where command_id = ?1
                  and session_thread_id = ?2
                  and kind = 'ResumeRuntime'
                  and state not in ('completed', 'failed', 'blocked', 'expired')
                "#,
            )
            .bind(command_id.as_str())
            .bind(claims.session_thread_id.as_str())
            .fetch_one(&state.pool)
            .await
            .map_err(internal_tool_error)?
                == 1
        } else {
            false
        }
    } else {
        true
    };
    if identity.node_id != claims.node_id
        || identity.placement_id != claims.project_placement_id
        || identity.project_id != claims.project_id
        || !stopped_resume_is_active
    {
        return Err(tool_error(
            ToolExecutionErrorCode::ScopeMismatch,
            "MCP lease scope no longer matches the session",
            false,
        ));
    }
    Ok(claims)
}

pub(crate) async fn revoke_session_mcp_leases(
    state: &AppState,
    session_id: &SessionThreadId,
    reason: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "update mcp_access_leases set revoked_at = ?1, revocation_reason = ?2 where session_thread_id = ?3 and revoked_at is null",
    )
    .bind(Utc::now())
    .bind(reason)
    .bind(session_id.as_str())
    .execute(&state.pool)
    .await?;
    Ok(())
}

pub(crate) async fn revoke_all_mcp_leases_for_credential_rotation(
    state: &AppState,
) -> Result<(), AppError> {
    sqlx::query(
        "update mcp_access_leases set revoked_at = ?1, revocation_reason = 'core_credential_rotated' where revoked_at is null",
    )
    .bind(Utc::now())
    .execute(&state.pool)
    .await?;
    Ok(())
}

pub(crate) fn lease_scope_matches(claims: &McpAccessLeaseClaims, scope: &ToolScope) -> bool {
    claims.actor_ref == scope.actor_ref
        && scope.session_thread_id.as_ref() == Some(&claims.session_thread_id)
        && scope.project_id == claims.project_id
        && scope.project_placement_id.as_ref() == Some(&claims.project_placement_id)
        && scope.node_id.as_ref() == Some(&claims.node_id)
}

async fn effective_tool_availability(
    state: &AppState,
    definition: &ToolDefinition,
    scope: &ToolScope,
) -> Result<ToolAvailability, AppError> {
    let mut availability_state = ToolAvailabilityState::Available;
    let mut reason = None;
    let mut backend_ref = None;
    let mut dependency_instance_id = None;
    if definition.state != ToolDefinitionState::Active {
        availability_state = ToolAvailabilityState::Unavailable;
        reason = Some(ToolUnavailableReason::PolicyBlocked);
    } else if !tool_is_visible_to_actor(&scope.actor_ref) {
        availability_state = ToolAvailabilityState::Unavailable;
        reason = Some(ToolUnavailableReason::PermissionDenied);
    } else if let Err(error) = validate_tool_scope(state, scope).await {
        availability_state = ToolAvailabilityState::Unavailable;
        reason = Some(match error {
            ScopeValidationError::SessionInactive => ToolUnavailableReason::SessionNotEnabled,
            ScopeValidationError::Mismatch => ToolUnavailableReason::PolicyBlocked,
        });
    } else if definition.approval_policy == PolicyDecision::RequireApproval {
        availability_state = ToolAvailabilityState::ApprovalRequired;
    } else if definition.execution_kind == ToolExecutionKind::ToolhiveMcp {
        backend_ref = Some("node-toolhive".to_owned());
        let Some(node_id) = scope.node_id.as_ref() else {
            availability_state = ToolAvailabilityState::Unavailable;
            reason = Some(ToolUnavailableReason::DependencyMissing);
            return Ok(ToolAvailability {
                tool_id: definition.tool_id.clone(),
                scope: scope.clone(),
                state: availability_state,
                reason,
                backend_ref,
                dependency_instance_id,
                schema_hash: definition.schema_hash.clone(),
                policy_version: TOOL_POLICY_VERSION.to_owned(),
                observed_at: Utc::now(),
            });
        };
        match effective_node_presence(state, node_id).await? {
            NodePresence::Offline | NodePresence::Revoked => {
                availability_state = ToolAvailabilityState::Unavailable;
                reason = Some(ToolUnavailableReason::NodeOffline);
            }
            NodePresence::Reachable | NodePresence::Stale => {
                let row: Option<(String, String, String)> = sqlx::query_as(
                    r#"
                    select ic.auth_state, mdi.dependency_instance_id, mdi.actual_state
                    from integration_connections ic
                    join mcp_dependency_instances mdi on mdi.integration_id = ic.integration_id
                    where ic.source_id = ?1 and mdi.node_id = ?2 and ic.desired_state = 'enabled'
                    order by ic.updated_at desc
                    limit 1
                    "#,
                )
                .bind(definition.source_id.as_str())
                .bind(node_id.as_str())
                .fetch_optional(&state.pool)
                .await?;
                match row {
                    None => {
                        availability_state = ToolAvailabilityState::Unavailable;
                        reason = Some(ToolUnavailableReason::DependencyMissing);
                    }
                    Some((auth_state, dependency_id, actual_state)) => {
                        dependency_instance_id = Some(McpDependencyInstanceId::from(dependency_id));
                        if auth_state != "connected" {
                            availability_state = ToolAvailabilityState::Unavailable;
                            reason = Some(ToolUnavailableReason::NotAuthenticated);
                        } else {
                            match actual_state.as_str() {
                                "running" => {}
                                "toolhive_missing" => {
                                    availability_state = ToolAvailabilityState::Unavailable;
                                    reason = Some(ToolUnavailableReason::ToolhiveMissing);
                                }
                                "degraded" => {
                                    availability_state = ToolAvailabilityState::Degraded;
                                    reason = Some(ToolUnavailableReason::DependencyUnhealthy);
                                }
                                _ => {
                                    availability_state = ToolAvailabilityState::Unavailable;
                                    reason = Some(ToolUnavailableReason::DependencyUnhealthy);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(ToolAvailability {
        tool_id: definition.tool_id.clone(),
        scope: scope.clone(),
        state: availability_state,
        reason,
        backend_ref: backend_ref.or_else(|| {
            Some(
                match definition.execution_kind {
                    ToolExecutionKind::CoreNative => "core-native",
                    ToolExecutionKind::ExternalProvider
                        if definition.source_id.as_str() == MOCK_SOURCE_ID =>
                    {
                        "mock-external"
                    }
                    _ => "unavailable",
                }
                .to_owned(),
            )
        }),
        dependency_instance_id,
        schema_hash: definition.schema_hash.clone(),
        policy_version: TOOL_POLICY_VERSION.to_owned(),
        observed_at: Utc::now(),
    })
}

#[derive(Debug, Clone, Copy)]
enum ScopeValidationError {
    SessionInactive,
    Mismatch,
}

async fn validate_tool_scope(
    state: &AppState,
    scope: &ToolScope,
) -> Result<ScopedIdentity, ScopeValidationError> {
    let session_id = scope
        .session_thread_id
        .as_ref()
        .ok_or(ScopeValidationError::Mismatch)?;
    let identity = load_scoped_identity_for_session(state, session_id)
        .await
        .map_err(|_| ScopeValidationError::Mismatch)?;
    if matches!(identity.session_state, SessionThreadState::Stopped) {
        return Err(ScopeValidationError::SessionInactive);
    }
    if scope.node_id.as_ref() != Some(&identity.node_id)
        || scope.project_placement_id.as_ref() != Some(&identity.placement_id)
        || scope.project_id != identity.project_id
    {
        return Err(ScopeValidationError::Mismatch);
    }
    Ok(identity)
}

async fn load_scoped_identity_for_session(
    state: &AppState,
    session_id: &SessionThreadId,
) -> Result<ScopedIdentity, AppError> {
    let row = sqlx::query(
        r#"
        select pp.node_id, pp.project_id, pp.project_placement_id, st.state
        from session_threads st
        join project_placements pp on pp.project_placement_id = st.project_placement_id
        where st.session_thread_id = ?1
        "#,
    )
    .bind(session_id.as_str())
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("session.not_found", "Session not found"))?;
    Ok(ScopedIdentity {
        node_id: NodeId::from(row.try_get::<String, _>("node_id")?),
        project_id: row
            .try_get::<Option<String>, _>("project_id")?
            .map(ProjectId::from),
        placement_id: ProjectPlacementId::from(row.try_get::<String, _>("project_placement_id")?),
        session_state: parse_session_state(row.try_get::<String, _>("state")?.as_str()),
    })
}

async fn load_active_tool_definitions(state: &AppState) -> Result<Vec<ToolDefinition>, AppError> {
    let rows = sqlx::query(
        "select definition_json from tool_definitions where state = 'active' order by tool_id",
    )
    .fetch_all(&state.pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            serde_json::from_str::<ToolDefinition>(&row.try_get::<String, _>("definition_json")?)
                .map_err(AppError::from)
        })
        .collect()
}

async fn load_tool_definition(
    state: &AppState,
    tool_id: &ToolId,
) -> Result<ToolDefinition, AppError> {
    let raw: Option<String> = sqlx::query_scalar(
        "select definition_json from tool_definitions where tool_id = ?1 and state = 'active'",
    )
    .bind(tool_id.as_str())
    .fetch_optional(&state.pool)
    .await?;
    raw.map(|value| serde_json::from_str(&value))
        .transpose()?
        .ok_or_else(|| AppError::not_found("tool.not_found", "Tool not found"))
}

async fn persist_session_tool_snapshot(
    state: &AppState,
    scope: &ToolScope,
    definition: &ToolDefinition,
) -> Result<(), AppError> {
    let Some(session_id) = &scope.session_thread_id else {
        return Ok(());
    };
    sqlx::query(
        r#"
        insert into session_tool_snapshots (
            session_thread_id, tool_id, schema_hash, definition_version, captured_at
        ) values (?1, ?2, ?3, ?4, ?5)
        on conflict(session_thread_id, tool_id) do update set
            schema_hash = excluded.schema_hash,
            definition_version = excluded.definition_version,
            captured_at = excluded.captured_at
        "#,
    )
    .bind(session_id.as_str())
    .bind(definition.tool_id.as_str())
    .bind(&definition.schema_hash)
    .bind(
        i64::try_from(definition.version)
            .map_err(|_| AppError::internal("tool version exceeds SQLite range"))?,
    )
    .bind(Utc::now())
    .execute(&state.pool)
    .await?;
    Ok(())
}

async fn insert_tool_call(
    state: &AppState,
    tool_call_id: &ToolCallId,
    correlation_id: &CorrelationId,
    definition: &ToolDefinition,
    request: &ExecuteToolRequest,
    policy: PolicyDecision,
) -> Result<(), AppError> {
    let now = Utc::now();
    let argument_bytes = serde_json::to_vec(&request.arguments)?;
    let argument_hash = format!("sha256:{:x}", Sha256::digest(&argument_bytes));
    let argument_summary = redact_and_summarize(
        &request.arguments.0,
        &definition.redaction.argument_json_pointers,
        definition.redaction.redact_all_arguments,
        definition.redaction.max_summary_bytes as usize,
    );
    sqlx::query(
        r#"
        insert into tool_calls (
            tool_call_id, tool_id, schema_hash, actor_ref_json, scope_json,
            source_kind, state, policy_decision, policy_version, route,
            correlation_id, argument_hash, redacted_arguments_summary,
            trace_refs_json, result_refs_json, requested_at
        ) values (?1, ?2, ?3, ?4, ?5, ?6, 'requested', ?7, ?8, ?9, ?10, ?11, ?12, ?13, '[]', ?14)
        "#,
    )
    .bind(tool_call_id.as_str())
    .bind(definition.tool_id.as_str())
    .bind(&definition.schema_hash)
    .bind(serde_json::to_string(&request.scope.actor_ref)?)
    .bind(serde_json::to_string(&request.scope)?)
    .bind(format_tool_source_kind(definition.source_kind))
    .bind(format_policy_decision(policy))
    .bind(TOOL_POLICY_VERSION)
    .bind(tool_route(definition))
    .bind(correlation_id.as_str())
    .bind(argument_hash)
    .bind(argument_summary)
    .bind(serde_json::to_string(&vec![UpravaRef::ToolCall {
        tool_call_id: tool_call_id.to_string(),
    }])?)
    .bind(now)
    .execute(&state.pool)
    .await?;
    Ok(())
}

async fn append_tool_call_event(
    state: &AppState,
    tool_call_id: &ToolCallId,
    event_state: ToolCallState,
    payload: Value,
) -> Result<(), AppError> {
    let mut transaction = state.pool.begin().await?;
    let sequence: i64 = sqlx::query_scalar(
        "select coalesce(max(sequence), 0) + 1 from tool_call_events where tool_call_id = ?1",
    )
    .bind(tool_call_id.as_str())
    .fetch_one(&mut *transaction)
    .await?;
    sqlx::query("insert into tool_call_events (tool_call_id, sequence, state, event_json, happened_at) values (?1, ?2, ?3, ?4, ?5)")
        .bind(tool_call_id.as_str())
        .bind(sequence)
        .bind(format_tool_call_state(event_state))
        .bind(serde_json::to_string(&payload)?)
        .bind(Utc::now())
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(())
}

async fn update_tool_call_state(
    state: &AppState,
    tool_call_id: &ToolCallId,
    call_state: ToolCallState,
) -> Result<(), AppError> {
    sqlx::query("update tool_calls set state = ?1 where tool_call_id = ?2")
        .bind(format_tool_call_state(call_state))
        .bind(tool_call_id.as_str())
        .execute(&state.pool)
        .await?;
    Ok(())
}

async fn mark_tool_call_started(
    state: &AppState,
    tool_call_id: &ToolCallId,
) -> Result<(), AppError> {
    let now = Utc::now();
    sqlx::query("update tool_calls set state = 'started', started_at = ?1 where tool_call_id = ?2")
        .bind(now)
        .bind(tool_call_id.as_str())
        .execute(&state.pool)
        .await?;
    append_tool_call_event(
        state,
        tool_call_id,
        ToolCallState::Started,
        json!({"started_at": now}),
    )
    .await
}

async fn finish_tool_call(
    state: &AppState,
    tool_call_id: &ToolCallId,
    terminal_state: ToolCallState,
    result: Option<&ToolResultEnvelope>,
    error: Option<&ToolExecutionError>,
    result_refs: Vec<UpravaRef>,
) -> Result<(), AppError> {
    if !terminal_state.is_terminal() {
        return Err(AppError::internal(
            "tool call finish requires terminal state",
        ));
    }
    let now = Utc::now();
    let result_bytes = result.map(serde_json::to_vec).transpose()?;
    let result_hash = result_bytes
        .as_ref()
        .map(|bytes| format!("sha256:{:x}", Sha256::digest(bytes)));
    let result_size = result_bytes.as_ref().map(|bytes| bytes.len() as i64);
    let result_summary = result.and_then(|value| value.summary.clone());
    sqlx::query(
        "update tool_calls set state = ?1, result_hash = ?2, result_size_bytes = ?3, redacted_result_summary = ?4, result_refs_json = ?5, error_json = ?6, completed_at = ?7 where tool_call_id = ?8",
    )
    .bind(format_tool_call_state(terminal_state))
    .bind(&result_hash)
    .bind(result_size)
    .bind(result_summary)
    .bind(serde_json::to_string(&result_refs)?)
    .bind(error.map(serde_json::to_string).transpose()?)
    .bind(now)
    .bind(tool_call_id.as_str())
    .execute(&state.pool)
    .await?;
    append_tool_call_event(
        state,
        tool_call_id,
        terminal_state,
        json!({
            "completed_at": now,
            "result_hash": result_hash,
            "result_size_bytes": result_size,
            "error": error,
        }),
    )
    .await
}

fn row_to_tool_call_summary(row: &sqlx::sqlite::SqliteRow) -> Result<ToolCallSummary, AppError> {
    Ok(ToolCallSummary {
        tool_call_id: ToolCallId::from(row.try_get::<String, _>("tool_call_id")?),
        tool_id: ToolId::from(row.try_get::<String, _>("tool_id")?),
        schema_hash: row.try_get("schema_hash")?,
        actor_ref: serde_json::from_str(&row.try_get::<String, _>("actor_ref_json")?)?,
        scope: serde_json::from_str(&row.try_get::<String, _>("scope_json")?)?,
        source_kind: parse_tool_source_kind(row.try_get::<String, _>("source_kind")?.as_str()),
        state: parse_tool_call_state(row.try_get::<String, _>("state")?.as_str()),
        policy_decision: parse_policy_decision(
            row.try_get::<String, _>("policy_decision")?.as_str(),
        ),
        route: row.try_get("route")?,
        requested_at: row.try_get("requested_at")?,
        started_at: row.try_get("started_at")?,
        completed_at: row.try_get("completed_at")?,
        correlation_id: CorrelationId::from(row.try_get::<String, _>("correlation_id")?),
    })
}

async fn audit_tool_policy(
    state: &AppState,
    request: &ExecuteToolRequest,
    tool_call_id: &ToolCallId,
    outcome: &str,
) -> Result<(), AppError> {
    audit_security_event(
        state,
        "tool.policy_decision",
        request.scope.node_id.as_ref(),
        Some(format!("tool_call:{}", tool_call_id)),
        outcome,
        JsonValue(json!({
            "tool_call_id": tool_call_id,
            "tool_id": request.tool_id,
            "policy_version": TOOL_POLICY_VERSION,
        })),
    )
    .await
}

fn policy_decision(definition: &ToolDefinition, scope: &ToolScope) -> PolicyDecision {
    if !tool_is_visible_to_actor(&scope.actor_ref) {
        PolicyDecision::Deny
    } else {
        definition.approval_policy
    }
}

fn tool_is_visible_to_actor(actor: &ActorRef) -> bool {
    matches!(
        actor,
        ActorRef::LocalUser { .. } | ActorRef::System | ActorRef::Provider { .. }
    )
}

fn definition_matches_filters(definition: &ToolDefinition, request: &SearchToolsRequest) -> bool {
    (request.filters.source_kinds.is_empty()
        || request
            .filters
            .source_kinds
            .contains(&definition.source_kind))
        && (request.filters.risk_levels.is_empty()
            || request.filters.risk_levels.contains(&definition.risk_level))
}

fn availability_matches_filters(
    availability: &ToolAvailability,
    request: &SearchToolsRequest,
) -> bool {
    request.filters.availability_states.is_empty()
        || request
            .filters
            .availability_states
            .contains(&availability.state)
}

fn normalize_search_document(definition: &ToolDefinition) -> String {
    format!(
        "{} {} {} {}",
        definition.tool_id,
        definition.display_name,
        definition.short_description,
        definition.source_tool_name
    )
    .to_lowercase()
}

fn tokenize(value: &str) -> Vec<String> {
    value
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn bm25_score(
    query_terms: &[String],
    document_terms: &[String],
    document_frequencies: &HashMap<String, usize>,
    document_count: f64,
    average_length: f64,
) -> f64 {
    const K1: f64 = 1.2;
    const B: f64 = 0.75;
    let mut term_counts = HashMap::new();
    for term in document_terms {
        *term_counts.entry(term.as_str()).or_insert(0usize) += 1;
    }
    query_terms
        .iter()
        .map(|query| {
            let frequency = *term_counts.get(query.as_str()).unwrap_or(&0) as f64;
            if frequency == 0.0 {
                return 0.0;
            }
            let document_frequency = *document_frequencies.get(query).unwrap_or(&0) as f64;
            let inverse_frequency =
                ((document_count - document_frequency + 0.5) / (document_frequency + 0.5) + 1.0)
                    .ln();
            let normalization = frequency
                + K1 * (1.0 - B + B * document_terms.len() as f64 / average_length.max(1.0));
            inverse_frequency * frequency * (K1 + 1.0) / normalization
        })
        .sum()
}

#[derive(Debug, Serialize, Deserialize)]
struct SearchCursorV1 {
    offset: usize,
    binding_hash: String,
    expires_at: DateTime<Utc>,
}

fn parse_search_cursor(state: &AppState, request: &SearchToolsRequest) -> Result<usize, AppError> {
    let Some(encoded) = request.cursor.as_deref() else {
        return Ok(0);
    };
    let invalid = || {
        AppError::bad_request(
            "tool_search.invalid_cursor",
            "Invalid or expired tool search cursor",
        )
    };
    let mut parts = encoded.split('.');
    if parts.next() != Some("v1") {
        return Err(invalid());
    }
    let payload = parts.next().and_then(decode_hex).ok_or_else(&invalid)?;
    let signature = parts.next().and_then(decode_hex).ok_or_else(&invalid)?;
    if parts.next().is_some() {
        return Err(invalid());
    }
    let mut mac = HmacSha256::new_from_slice(&state.mcp_lease_signing_key)
        .map_err(|_| AppError::internal("invalid search cursor signing key"))?;
    mac.update(&payload);
    mac.verify_slice(&signature).map_err(|_| invalid())?;
    let cursor: SearchCursorV1 = serde_json::from_slice(&payload).map_err(|_| invalid())?;
    if cursor.expires_at <= Utc::now() || cursor.binding_hash != search_cursor_binding(request)? {
        return Err(invalid());
    }
    Ok(cursor.offset)
}

fn create_search_cursor(
    state: &AppState,
    request: &SearchToolsRequest,
    offset: usize,
) -> Result<String, AppError> {
    let payload = serde_json::to_vec(&SearchCursorV1 {
        offset,
        binding_hash: search_cursor_binding(request)?,
        expires_at: Utc::now() + ChronoDuration::seconds(TOOL_SEARCH_CURSOR_TTL_SECONDS),
    })?;
    let signature = sign_lease(state, &payload)?;
    Ok(format!(
        "v1.{}.{}",
        encode_hex(&payload),
        encode_hex(&signature)
    ))
}

fn search_cursor_binding(request: &SearchToolsRequest) -> Result<String, AppError> {
    let material = serde_json::to_vec(&json!({
        "scope": request.scope,
        "query": request.query,
        "filters": request.filters,
    }))?;
    Ok(format!("sha256:{:x}", Sha256::digest(material)))
}

fn validate_arguments(schema: &Value, arguments: &Value) -> Result<(), String> {
    let object = arguments
        .as_object()
        .ok_or_else(|| "arguments must be an object".to_owned())?;
    let schema_object = schema
        .as_object()
        .ok_or_else(|| "tool schema must be an object".to_owned())?;
    if schema_object.get("type").and_then(Value::as_str) != Some("object") {
        return Err("tool schema root must be an object".to_owned());
    }
    let properties = schema_object.get("properties").and_then(Value::as_object);
    let required = schema_object
        .get("required")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for required_name in required.iter().filter_map(Value::as_str) {
        if !object.contains_key(required_name) {
            return Err(format!("missing required argument `{required_name}`"));
        }
    }
    if schema_object
        .get("additionalProperties")
        .and_then(Value::as_bool)
        == Some(false)
    {
        for key in object.keys() {
            if properties.is_none_or(|items| !items.contains_key(key)) {
                return Err(format!("unknown argument `{key}`"));
            }
        }
    }
    if let Some(properties) = properties {
        for (key, value) in object {
            let Some(property) = properties.get(key) else {
                continue;
            };
            if !json_type_matches(property.get("type").and_then(Value::as_str), value) {
                return Err(format!("argument `{key}` has the wrong JSON type"));
            }
        }
    }
    Ok(())
}

fn json_type_matches(expected: Option<&str>, value: &Value) -> bool {
    match expected {
        None => true,
        Some("string") => value.is_string(),
        Some("boolean") => value.is_boolean(),
        Some("number") => value.is_number(),
        Some("integer") => value.as_i64().is_some() || value.as_u64().is_some(),
        Some("object") => value.is_object(),
        Some("array") => value.is_array(),
        Some("null") => value.is_null(),
        Some(_) => false,
    }
}

fn string_argument<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, ToolExecutionError> {
    arguments.get(key).and_then(Value::as_str).ok_or_else(|| {
        tool_error(
            ToolExecutionErrorCode::InvalidArguments,
            &format!("Missing string argument `{key}`"),
            false,
        )
    })
}

fn redact_and_summarize(
    value: &Value,
    pointers: &[String],
    redact_all: bool,
    max_bytes: usize,
) -> String {
    let redacted = redact_json(value, pointers, redact_all);
    bounded_json_summary(&redacted, max_bytes.clamp(16, TOOL_SUMMARY_MAX_BYTES))
}

fn redact_json(value: &Value, pointers: &[String], redact_all: bool) -> Value {
    if redact_all {
        return Value::String("[redacted]".to_owned());
    }
    let mut redacted = value.clone();
    for pointer in pointers {
        if let Some(slot) = redacted.pointer_mut(pointer) {
            *slot = Value::String("[redacted]".to_owned());
        }
    }
    redacted
}

fn bounded_json_summary(value: &Value, max_bytes: usize) -> String {
    let serialized = serde_json::to_string(value).unwrap_or_else(|_| "[unserializable]".to_owned());
    if serialized.len() <= max_bytes {
        return serialized;
    }
    let mut boundary = max_bytes.saturating_sub(3).min(serialized.len());
    while !serialized.is_char_boundary(boundary) {
        boundary = boundary.saturating_sub(1);
    }
    format!("{}...", &serialized[..boundary])
}

fn tool_error(code: ToolExecutionErrorCode, message: &str, retryable: bool) -> ToolExecutionError {
    ToolExecutionError {
        code,
        message: message.to_owned(),
        retryable,
        redacted_details: JsonValue::default(),
    }
}

fn internal_tool_error(error: impl std::fmt::Display) -> ToolExecutionError {
    tracing::error!(error = %error, "tool execution internal error");
    tool_error(
        ToolExecutionErrorCode::BackendFailed,
        "Core tool execution failed",
        true,
    )
}

fn app_tool_error(error: AppError) -> ToolExecutionError {
    match error {
        AppError::NotFound { message, .. } | AppError::BadRequest { message, .. } => {
            tool_error(ToolExecutionErrorCode::BackendFailed, &message, false)
        }
        other => internal_tool_error(other),
    }
}

fn sign_lease(state: &AppState, material: &[u8]) -> Result<Vec<u8>, AppError> {
    let mut mac = HmacSha256::new_from_slice(&state.mcp_lease_signing_key)
        .map_err(|_| AppError::internal("invalid MCP lease signing key"))?;
    mac.update(material);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn verify_lease_signature(
    state: &AppState,
    material: &[u8],
    signature: &[u8],
) -> Result<(), ToolExecutionError> {
    let mut mac =
        HmacSha256::new_from_slice(&state.mcp_lease_signing_key).map_err(internal_tool_error)?;
    mac.update(material);
    mac.verify_slice(signature).map_err(|_| {
        tool_error(
            ToolExecutionErrorCode::NotAuthenticated,
            "Invalid MCP lease signature",
            false,
        )
    })
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if !value.is_ascii() || !value.len().is_multiple_of(2) {
        return None;
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).ok())
        .collect()
}

fn tool_route(definition: &ToolDefinition) -> &'static str {
    if definition.source_id.as_str() == MOCK_SOURCE_ID {
        "mock_external"
    } else {
        "core_native"
    }
}

fn format_tool_source_kind(value: ToolSourceKind) -> &'static str {
    match value {
        ToolSourceKind::UpravaNative => "uprava_native",
        ToolSourceKind::ExternalMcp => "external_mcp",
        ToolSourceKind::Plugin => "plugin",
    }
}

fn parse_tool_source_kind(value: &str) -> ToolSourceKind {
    match value {
        "external_mcp" => ToolSourceKind::ExternalMcp,
        "plugin" => ToolSourceKind::Plugin,
        _ => ToolSourceKind::UpravaNative,
    }
}

fn format_tool_definition_state(value: ToolDefinitionState) -> &'static str {
    match value {
        ToolDefinitionState::Active => "active",
        ToolDefinitionState::Deprecated => "deprecated",
        ToolDefinitionState::Disabled => "disabled",
    }
}

fn format_policy_decision(value: PolicyDecision) -> &'static str {
    match value {
        PolicyDecision::Allow => "allow",
        PolicyDecision::Deny => "deny",
        PolicyDecision::RequireApproval => "require_approval",
    }
}

fn parse_policy_decision(value: &str) -> PolicyDecision {
    match value {
        "deny" => PolicyDecision::Deny,
        "require_approval" => PolicyDecision::RequireApproval,
        _ => PolicyDecision::Allow,
    }
}

fn format_tool_call_state(value: ToolCallState) -> &'static str {
    match value {
        ToolCallState::Requested => "requested",
        ToolCallState::Authorized => "authorized",
        ToolCallState::ApprovalRequired => "approval_required",
        ToolCallState::Started => "started",
        ToolCallState::Completed => "completed",
        ToolCallState::Failed => "failed",
        ToolCallState::Denied => "denied",
        ToolCallState::Cancelled => "cancelled",
        ToolCallState::TimedOut => "timed_out",
    }
}

fn format_integration_desired_state(value: IntegrationDesiredState) -> &'static str {
    match value {
        IntegrationDesiredState::Enabled => "enabled",
        IntegrationDesiredState::Disabled => "disabled",
    }
}

fn format_integration_auth_state(value: IntegrationAuthState) -> &'static str {
    match value {
        IntegrationAuthState::Disconnected => "disconnected",
        IntegrationAuthState::Connecting => "connecting",
        IntegrationAuthState::Connected => "connected",
        IntegrationAuthState::Expired => "expired",
        IntegrationAuthState::Error => "error",
    }
}

fn parse_integration_desired_state(value: &str) -> IntegrationDesiredState {
    match value {
        "enabled" => IntegrationDesiredState::Enabled,
        _ => IntegrationDesiredState::Disabled,
    }
}

fn format_dependency_actual_state(value: McpDependencyActualState) -> &'static str {
    match value {
        McpDependencyActualState::ToolhiveMissing => "toolhive_missing",
        McpDependencyActualState::MissingAuth => "missing_auth",
        McpDependencyActualState::Installing => "installing",
        McpDependencyActualState::Starting => "starting",
        McpDependencyActualState::Running => "running",
        McpDependencyActualState::Degraded => "degraded",
        McpDependencyActualState::Failed => "failed",
        McpDependencyActualState::Stopped => "stopped",
    }
}

fn parse_tool_call_state(value: &str) -> ToolCallState {
    match value {
        "authorized" => ToolCallState::Authorized,
        "approval_required" => ToolCallState::ApprovalRequired,
        "started" => ToolCallState::Started,
        "completed" => ToolCallState::Completed,
        "failed" => ToolCallState::Failed,
        "denied" => ToolCallState::Denied,
        "cancelled" => ToolCallState::Cancelled,
        "timed_out" => ToolCallState::TimedOut,
        _ => ToolCallState::Requested,
    }
}

fn native_tool_definitions() -> Result<Vec<ToolDefinition>, AppError> {
    let source_id = ToolSourceId::from(NATIVE_SOURCE_ID);
    [
        (
            "uprava.node.inspect",
            "node.inspect",
            "Inspect node",
            "Read Core-owned Node state and safe capabilities.",
            "node_id",
        ),
        (
            "uprava.workspace.inspect",
            "workspace.inspect",
            "Inspect workspace",
            "Read Core-owned project placement and workspace state.",
            "placement_id",
        ),
        (
            "uprava.session.inspect",
            "session.inspect",
            "Inspect session",
            "Read one Core-owned agent session and its current projection.",
            "session_thread_id",
        ),
        (
            "uprava.trace.resolve",
            "trace.resolve",
            "Resolve trace",
            "Read the structured trace projection for one session.",
            "session_thread_id",
        ),
    ]
    .into_iter()
    .map(
        |(tool_id, source_name, display_name, description, argument)| {
            native_definition(
                &source_id,
                tool_id,
                source_name,
                display_name,
                description,
                json!({
                    "type": "object",
                    "properties": { argument: { "type": "string", "minLength": 1 } },
                    "required": [argument],
                    "additionalProperties": false
                }),
            )
        },
    )
    .chain(std::iter::once(native_definition(
        &source_id,
        "uprava.capability.inspect",
        "capability.inspect",
        "Inspect capability",
        "Read safe capability state observed for one Node.",
        json!({
            "type": "object",
            "properties": {
                "node_id": { "type": "string", "minLength": 1 },
                "capability_key": { "type": "string", "minLength": 1 }
            },
            "required": ["node_id"],
            "additionalProperties": false
        }),
    )))
    .chain(std::iter::once(native_definition(
        &source_id,
        "uprava.dynamic_ui.inspect",
        "dynamic_ui.inspect",
        "Inspect generated UI",
        "Inspect the installed Generated React runtime, Uprava UI SDK contract, layouts, capabilities and opt-in state.",
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }),
    )))
    .chain(std::iter::once(dynamic_ui_tool_definition(&source_id)))
    .collect()
}

fn dynamic_ui_tool_definition(source_id: &ToolSourceId) -> Result<ToolDefinition, AppError> {
    let mut definition = native_definition(
        source_id,
        "uprava.dynamic_ui.create",
        "dynamic_ui.create",
        "Create generated UI",
        "Propose a session-scoped Generated React artifact with a readable fallback. Inspect uprava.dynamic_ui.inspect first for the SDK contract and opt-in state.",
        json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "minLength": 1, "maxLength": 240 },
                "description": { "type": "string", "maxLength": 2000 },
                "runtime_id": { "type": "string", "minLength": 1 },
                "sdk_version": { "type": "string", "minLength": 1 },
                "layout_intent": { "type": "string", "enum": ["inline", "panel", "canvas"] },
                "source": { "type": "string", "minLength": 1 },
                "data_model": {},
                "actions": { "type": "array" },
                "requested_capabilities": { "type": "array" },
                "fallback_markdown": { "type": "string", "minLength": 1 },
                "fallback_snapshot": { "type": "string" },
                "source_refs": { "type": "array" },
                "evidence_refs": { "type": "array" },
                "cause_refs": { "type": "array" },
                "trace_refs": { "type": "array" }
            },
            "required": [
                "title", "runtime_id", "sdk_version", "layout_intent", "source",
                "data_model", "fallback_markdown"
            ],
            "additionalProperties": false
        }),
    )?;
    definition.risk_level = ToolRiskLevel::WorkspaceWrite;
    definition.required_permissions = vec!["dynamic_ui.propose".to_owned()];
    Ok(definition)
}

fn native_definition(
    source_id: &ToolSourceId,
    tool_id: &str,
    source_tool_name: &str,
    display_name: &str,
    description: &str,
    input_schema: Value,
) -> Result<ToolDefinition, AppError> {
    let now = Utc::now();
    let input_schema = JsonValue(input_schema);
    let schema_hash = compute_tool_schema_hash(&input_schema, None)?;
    Ok(ToolDefinition {
        tool_id: ToolId::from(tool_id),
        source_id: source_id.clone(),
        source_kind: ToolSourceKind::UpravaNative,
        source_tool_name: source_tool_name.to_owned(),
        version: 1,
        display_name: display_name.to_owned(),
        short_description: description.to_owned(),
        documentation_url: None,
        input_schema,
        output_schema: None,
        schema_hash,
        risk_level: ToolRiskLevel::ReadOnly,
        required_permissions: vec![],
        execution_kind: ToolExecutionKind::CoreNative,
        approval_policy: PolicyDecision::Allow,
        redaction: ToolRedactionPolicy {
            argument_json_pointers: vec![],
            result_json_pointers: vec![],
            redact_all_arguments: false,
            redact_all_result: false,
            max_summary_bytes: TOOL_SUMMARY_MAX_BYTES as u32,
        },
        state: ToolDefinitionState::Active,
        created_at: now,
        updated_at: now,
    })
}

#[cfg(test)]
pub(crate) fn mock_external_definition() -> ToolDefinition {
    let now = Utc::now();
    let input_schema = JsonValue(json!({
        "type": "object",
        "properties": { "value": {}, "fail": { "type": "boolean" } },
        "additionalProperties": false
    }));
    ToolDefinition {
        tool_id: ToolId::from("mock.external.echo"),
        source_id: ToolSourceId::from(MOCK_SOURCE_ID),
        source_kind: ToolSourceKind::ExternalMcp,
        source_tool_name: "echo".to_owned(),
        version: 1,
        display_name: "Mock external echo".to_owned(),
        short_description: "Deterministic external backend fixture.".to_owned(),
        documentation_url: None,
        schema_hash: compute_tool_schema_hash(&input_schema, None)
            .expect("mock schema hash computes"),
        input_schema,
        output_schema: None,
        risk_level: ToolRiskLevel::ExternalRead,
        required_permissions: vec![],
        execution_kind: ToolExecutionKind::ExternalProvider,
        approval_policy: PolicyDecision::Allow,
        redaction: ToolRedactionPolicy {
            argument_json_pointers: vec![],
            result_json_pointers: vec![],
            redact_all_arguments: false,
            redact_all_result: false,
            max_summary_bytes: 512,
        },
        state: ToolDefinitionState::Active,
        created_at: now,
        updated_at: now,
    }
}
