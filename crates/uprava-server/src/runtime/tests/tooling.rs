use uprava_protocol::{
    compute_tool_schema_hash, ExecuteToolRequest, InspectToolRequest, IntegrationAuthState,
    IntegrationConnectionSummary, IntegrationDesiredState, IntegrationId, McpDependencyActualState,
    McpDependencyInstanceId, McpDependencyStatus, PolicyDecision, SearchToolsRequest,
    ToolAvailabilityState, ToolCallId, ToolCallState, ToolCallSummary, ToolExecutionErrorCode,
    ToolExecutionKind, ToolId, ToolScope, ToolSearchFilters, ToolSourceId, ToolSourceKind,
    ToolUnavailableReason,
};

use super::*;

fn scope_for(node_id: &NodeId, detail: &SessionDetail, actor_ref: ActorRef) -> ToolScope {
    ToolScope {
        actor_ref,
        node_id: Some(node_id.clone()),
        project_id: detail.placement.project_id.clone(),
        project_placement_id: Some(detail.placement.project_placement_id.clone()),
        session_thread_id: Some(detail.session.session_thread_id.clone()),
    }
}

#[test]
fn tool_call_scope_filter_does_not_leak_other_sessions() {
    let summary = ToolCallSummary {
        tool_call_id: ToolCallId::from("tool-call-1"),
        tool_id: ToolId::from("uprava.session.inspect"),
        schema_hash: "sha256:fixture".to_owned(),
        actor_ref: ActorRef::Provider {
            provider: "codex".to_owned(),
        },
        scope: ToolScope {
            actor_ref: ActorRef::Provider {
                provider: "codex".to_owned(),
            },
            node_id: Some(NodeId::from("node-1")),
            project_id: Some(ProjectId::from("project-1")),
            project_placement_id: Some(ProjectPlacementId::from("placement-1")),
            session_thread_id: Some(SessionThreadId::from("session-1")),
        },
        source_kind: ToolSourceKind::UpravaNative,
        state: ToolCallState::Completed,
        policy_decision: PolicyDecision::Allow,
        route: "core_native".to_owned(),
        requested_at: Utc::now(),
        started_at: Some(Utc::now()),
        completed_at: Some(Utc::now()),
        correlation_id: CorrelationId::from("correlation-1"),
    };
    let current = ToolCallsQuery {
        limit: Some(50),
        node_id: Some("node-1".to_owned()),
        project_id: Some("project-1".to_owned()),
        project_placement_id: Some("placement-1".to_owned()),
        session_thread_id: Some("session-1".to_owned()),
    };
    let foreign = ToolCallsQuery {
        session_thread_id: Some("session-2".to_owned()),
        ..current
    };

    assert!(!tool_call_matches_query(&summary, &foreign));
}

#[test]
fn core_durable_command_result_removes_ephemeral_authorization_url() {
    let payload = JsonValue(serde_json::json!({
        "authorization_url": "https://linear.app/oauth/authorize?state=secret-state",
        "authorization_expires_at": "2026-07-19T10:05:00Z"
    }));

    let durable = durable_command_result_payload(&payload);

    assert!(durable.0.get("authorization_url").is_none());
}

#[test]
fn linear_authorization_url_validation_rejects_lookalike_host() {
    let expires_at = Utc::now() + ChronoDuration::minutes(5);

    let result = validate_linear_authorization_url(
        "https://linear.app.attacker.test/oauth/authorize?state=opaque",
        expires_at,
    );

    assert!(result.is_err());
}

#[test]
fn linear_authorization_url_validation_accepts_pinned_host() {
    let expires_at = Utc::now() + ChronoDuration::minutes(5);

    let result = validate_linear_authorization_url(
        "https://linear.app/oauth/authorize?client_id=uprava&state=opaque",
        expires_at,
    );

    assert!(
        result.is_ok(),
        "expected pinned Linear URL to pass: {result:?}"
    );
}

#[tokio::test]
async fn external_availability_uses_node_auth_and_dependency_actual_state() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    let mut definition = mock_external_definition();
    definition.tool_id = ToolId::from("linear.search_issues");
    definition.source_id = ToolSourceId::from("linear-remote-mcp");
    definition.source_tool_name = "search_issues".to_owned();
    definition.execution_kind = ToolExecutionKind::ToolhiveMcp;
    register_tool_definitions(
        &state,
        "linear-remote-mcp",
        ToolSourceKind::ExternalMcp,
        "Linear remote MCP",
        std::slice::from_ref(&definition),
    )
    .await
    .expect("linear definition registers");
    let now = Utc::now();
    let integration_id = IntegrationId::from("integration-linear-test");
    let dependency_id = McpDependencyInstanceId::from("dependency-linear-test");
    let connection = IntegrationConnectionSummary {
        integration_id: integration_id.clone(),
        source_id: ToolSourceId::from("linear-remote-mcp"),
        provider: "linear".to_owned(),
        display_name: "Linear".to_owned(),
        desired_state: IntegrationDesiredState::Enabled,
        auth_state: IntegrationAuthState::Connected,
        node_id: Some(node_id.clone()),
        authenticated_actor_label: None,
        connected_at: Some(now),
        updated_at: now,
        error_code: None,
    };
    sqlx::query(
        "insert into integration_connections (integration_id, source_id, provider, desired_state, auth_state, node_id, connection_json, credential_generation, created_at, updated_at) values (?1, 'linear-remote-mcp', 'linear', 'enabled', 'connected', ?2, ?3, 1, ?4, ?4)",
    )
    .bind(integration_id.as_str())
    .bind(node_id.as_str())
    .bind(serde_json::to_string(&connection).expect("connection serializes"))
    .bind(now)
    .execute(&state.pool)
    .await
    .expect("connection inserts");
    let status = McpDependencyStatus {
        dependency_instance_id: dependency_id,
        integration_id,
        node_id: node_id.clone(),
        desired_state: IntegrationDesiredState::Enabled,
        actual_state: McpDependencyActualState::Running,
        runtime_name: "toolhive".to_owned(),
        runtime_version: Some("0.40.0".to_owned()),
        upstream_identity: Some("linear-remote-mcp".to_owned()),
        schema_set_hash: Some("sha256:fixture".to_owned()),
        error_code: None,
        observed_at: now,
    };
    persist_dependency_status(&state, &status)
        .await
        .expect("dependency status persists");
    let inspected = inspect_tool(
        &state,
        &InspectToolRequest {
            scope: scope_for(&node_id, &detail, ActorRef::local_user()),
            tool_id: definition.tool_id,
        },
    )
    .await
    .expect("external tool inspects");
    assert_eq!(
        inspected.availability.state,
        ToolAvailabilityState::Available
    );

    sqlx::query("update integration_connections set auth_state = 'disconnected'")
        .execute(&state.pool)
        .await
        .expect("connection disconnects");
    let unavailable = inspect_tool(
        &state,
        &InspectToolRequest {
            scope: scope_for(&node_id, &detail, ActorRef::local_user()),
            tool_id: ToolId::from("linear.search_issues"),
        },
    )
    .await
    .expect("disconnected tool remains inspectable");
    assert_eq!(
        unavailable.availability.reason,
        Some(ToolUnavailableReason::NotAuthenticated)
    );
    let _ = std::fs::remove_dir_all(workspace_path);
}

#[tokio::test]
async fn registry_keeps_version_for_same_schema_and_increments_for_schema_change() {
    let state = test_state().await;
    let tool_id = ToolId::from("uprava.node.inspect");
    let original = get_tool_definition(&state, &tool_id)
        .await
        .expect("seeded tool loads");
    register_tool_definitions(
        &state,
        "uprava-native",
        ToolSourceKind::UpravaNative,
        "Uprava",
        std::slice::from_ref(&original),
    )
    .await
    .expect("unchanged definition registers");
    let unchanged = get_tool_definition(&state, &tool_id)
        .await
        .expect("unchanged tool loads");
    assert_eq!(unchanged.version, original.version);

    let mut changed = original.clone();
    changed.input_schema = JsonValue(serde_json::json!({
        "type": "object",
        "properties": {
            "node_id": { "type": "string" },
            "include_diagnostics": { "type": "boolean" }
        },
        "required": ["node_id"],
        "additionalProperties": false
    }));
    changed.schema_hash = compute_tool_schema_hash(&changed.input_schema, None)
        .expect("changed schema hash computes");
    register_tool_definitions(
        &state,
        "uprava-native",
        ToolSourceKind::UpravaNative,
        "Uprava",
        &[changed],
    )
    .await
    .expect("changed definition registers");
    let incremented = get_tool_definition(&state, &tool_id)
        .await
        .expect("changed tool loads");
    assert_eq!(incremented.version, original.version + 1);
}

#[tokio::test]
async fn progressive_discovery_executes_native_tool_with_durable_trace() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    let scope = scope_for(&node_id, &detail, ActorRef::local_user());

    let search = search_tools(
        &state,
        &SearchToolsRequest {
            scope: scope.clone(),
            query: "session inspect".to_owned(),
            filters: ToolSearchFilters::default(),
            cursor: None,
            limit: Some(10),
        },
    )
    .await
    .expect("tool search succeeds");
    assert!(search
        .items
        .iter()
        .any(|item| item.tool_id.as_str() == "uprava.session.inspect"));

    let inspected = inspect_tool(
        &state,
        &InspectToolRequest {
            scope: scope.clone(),
            tool_id: ToolId::from("uprava.session.inspect"),
        },
    )
    .await
    .expect("tool inspect succeeds");
    assert_eq!(
        inspected.definition.tool_id.as_str(),
        "uprava.session.inspect"
    );

    let executed = execute_tool(
        &state,
        &ExecuteToolRequest {
            scope,
            tool_id: ToolId::from("uprava.session.inspect"),
            arguments: JsonValue(serde_json::json!({
                "session_thread_id": detail.session.session_thread_id
            })),
        },
    )
    .await
    .expect("native tool execution succeeds");
    assert_eq!(executed.state, ToolCallState::Completed);

    let detail = load_tool_call_detail(&state, &executed.tool_call_id)
        .await
        .expect("tool call detail loads");
    assert_eq!(detail.summary.route, "core_native");

    let event_states: Vec<String> = sqlx::query_scalar(
        "select state from tool_call_events where tool_call_id = ?1 order by sequence",
    )
    .bind(executed.tool_call_id.as_str())
    .fetch_all(&state.pool)
    .await
    .expect("tool call events load");
    assert_eq!(
        event_states,
        vec!["requested", "authorized", "started", "completed"]
    );

    let snapshot_count: i64 = sqlx::query_scalar(
        "select count(*) from session_tool_snapshots where session_thread_id = ?1 and tool_id = 'uprava.session.inspect'",
    )
    .bind(detail.summary.scope.session_thread_id.expect("session scope").as_str())
    .fetch_one(&state.pool)
    .await
    .expect("session tool snapshot count loads");
    assert_eq!(snapshot_count, 1);
    let _ = std::fs::remove_dir_all(workspace_path);
}

#[tokio::test]
async fn search_hides_all_definitions_from_denied_actor() {
    let state = test_state().await;
    let response = search_tools(
        &state,
        &SearchToolsRequest {
            scope: ToolScope {
                actor_ref: ActorRef::Unknown,
                node_id: None,
                project_id: None,
                project_placement_id: None,
                session_thread_id: None,
            },
            query: "inspect".to_owned(),
            filters: ToolSearchFilters::default(),
            cursor: None,
            limit: None,
        },
    )
    .await
    .expect("denied search returns an empty page");

    assert!(response.items.is_empty());
}

#[tokio::test]
async fn search_cursor_is_bounded_signed_and_bound_to_the_original_query() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    let scope = scope_for(&node_id, &detail, ActorRef::local_user());
    let first_page = search_tools(
        &state,
        &SearchToolsRequest {
            scope: scope.clone(),
            query: "inspect".to_owned(),
            filters: ToolSearchFilters::default(),
            cursor: None,
            limit: Some(1),
        },
    )
    .await
    .expect("first search page succeeds");
    assert_eq!(first_page.items.len(), 1);
    let cursor = first_page.next_cursor.expect("next cursor exists");

    let second_page = search_tools(
        &state,
        &SearchToolsRequest {
            scope: scope.clone(),
            query: "inspect".to_owned(),
            filters: ToolSearchFilters::default(),
            cursor: Some(cursor.clone()),
            limit: Some(1),
        },
    )
    .await
    .expect("bound cursor continues the search");
    assert_eq!(second_page.items.len(), 1);
    assert_ne!(first_page.items[0].tool_id, second_page.items[0].tool_id);

    let rebound = search_tools(
        &state,
        &SearchToolsRequest {
            scope,
            query: "trace".to_owned(),
            filters: ToolSearchFilters::default(),
            cursor: Some(cursor),
            limit: Some(1),
        },
    )
    .await
    .expect_err("cursor cannot be reused for another query");
    assert!(matches!(
        rebound,
        AppError::BadRequest { code, .. } if code == "tool_search.invalid_cursor"
    ));

    let invalid_limit = search_tools(
        &state,
        &SearchToolsRequest {
            scope: scope_for(&node_id, &detail, ActorRef::local_user()),
            query: String::new(),
            filters: ToolSearchFilters::default(),
            cursor: None,
            limit: Some(0),
        },
    )
    .await
    .expect_err("zero search limit is rejected");
    assert!(matches!(
        invalid_limit,
        AppError::BadRequest { code, .. } if code == "tool_search.invalid_limit"
    ));
    let _ = std::fs::remove_dir_all(workspace_path);
}

#[tokio::test]
async fn execute_rechecks_schema_and_records_terminal_failure() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    let response = execute_tool(
        &state,
        &ExecuteToolRequest {
            scope: scope_for(&node_id, &detail, ActorRef::local_user()),
            tool_id: ToolId::from("uprava.node.inspect"),
            arguments: JsonValue(serde_json::json!({"unexpected": true})),
        },
    )
    .await
    .expect("invalid execution returns a typed response");

    assert_eq!(response.state, ToolCallState::Failed);
    assert_eq!(
        response.error.expect("typed error").code,
        ToolExecutionErrorCode::InvalidArguments
    );
    let persisted_state: String =
        sqlx::query_scalar("select state from tool_calls where tool_call_id = ?1")
            .bind(response.tool_call_id.as_str())
            .fetch_one(&state.pool)
            .await
            .expect("failed tool call state loads");
    assert_eq!(persisted_state, "failed");
    let _ = std::fs::remove_dir_all(workspace_path);
}

#[tokio::test]
async fn deterministic_mock_backend_proves_success_and_failure_routes() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    let mut definition = mock_external_definition();
    definition.redaction.result_json_pointers = vec!["/backend".to_owned()];
    register_tool_definitions(
        &state,
        "mock-external",
        ToolSourceKind::ExternalMcp,
        "Mock external backend",
        &[definition],
    )
    .await
    .expect("mock definition registers");
    let scope = scope_for(&node_id, &detail, ActorRef::local_user());
    let successful = execute_tool(
        &state,
        &ExecuteToolRequest {
            scope: scope.clone(),
            tool_id: ToolId::from("mock.external.echo"),
            arguments: JsonValue(serde_json::json!({"value": "fixture"})),
        },
    )
    .await
    .expect("mock success returns");
    assert_eq!(successful.state, ToolCallState::Completed);
    assert_eq!(
        successful
            .result
            .as_ref()
            .expect("mock result exists")
            .content
            .0["backend"],
        "[redacted]"
    );
    let persisted_summary: String = sqlx::query_scalar(
        "select redacted_result_summary from tool_calls where tool_call_id = ?1",
    )
    .bind(successful.tool_call_id.as_str())
    .fetch_one(&state.pool)
    .await
    .expect("redacted mock result summary loads");
    assert!(persisted_summary.contains("[redacted]"));
    assert!(!persisted_summary.contains("\"backend\":\"mock\""));

    let failed = execute_tool(
        &state,
        &ExecuteToolRequest {
            scope,
            tool_id: ToolId::from("mock.external.echo"),
            arguments: JsonValue(serde_json::json!({"fail": true})),
        },
    )
    .await
    .expect("mock failure returns");
    assert_eq!(failed.state, ToolCallState::Failed);
    assert_eq!(
        failed.error.expect("mock error").code,
        ToolExecutionErrorCode::BackendFailed
    );
    let _ = std::fs::remove_dir_all(workspace_path);
}

#[tokio::test]
async fn leases_reject_rotation_expiry_revocation_and_foreign_scope() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    let actor = ActorRef::Provider {
        provider: "codex".to_owned(),
    };
    let (first_token, first_claims) = state
        .create_mcp_access_lease(&detail.session.session_thread_id, actor.clone())
        .await
        .expect("first lease issues");
    validate_mcp_access_lease(&state, &first_token)
        .await
        .expect("first lease validates");

    let (second_token, second_claims) = state
        .create_mcp_access_lease(&detail.session.session_thread_id, actor.clone())
        .await
        .expect("rotated lease issues");
    let rotated = validate_mcp_access_lease(&state, &first_token)
        .await
        .expect_err("rotated lease is revoked");
    assert_eq!(rotated.code, ToolExecutionErrorCode::LeaseRevoked);

    let mut foreign_scope = scope_for(&node_id, &detail, actor);
    foreign_scope.node_id = Some(NodeId::from("foreign-node"));
    assert!(!lease_scope_matches(&second_claims, &foreign_scope));

    sqlx::query("update mcp_access_leases set expires_at = ?1 where lease_id = ?2")
        .bind(Utc::now() - chrono::Duration::seconds(1))
        .bind(second_claims.lease_id.as_str())
        .execute(&state.pool)
        .await
        .expect("lease expiry updates");
    let expired = validate_mcp_access_lease(&state, &second_token)
        .await
        .expect_err("expired lease is rejected");
    assert_eq!(expired.code, ToolExecutionErrorCode::LeaseExpired);

    let (third_token, _) = state
        .create_mcp_access_lease(
            &detail.session.session_thread_id,
            ActorRef::Provider {
                provider: "codex".to_owned(),
            },
        )
        .await
        .expect("third lease issues");
    revoke_session_mcp_leases(&state, &detail.session.session_thread_id, "test_revocation")
        .await
        .expect("lease revokes");
    let revoked = validate_mcp_access_lease(&state, &third_token)
        .await
        .expect_err("revoked lease is rejected");
    assert_eq!(revoked.code, ToolExecutionErrorCode::LeaseRevoked);
    assert_eq!(
        first_claims.session_thread_id,
        detail.session.session_thread_id
    );
    let _ = std::fs::remove_dir_all(workspace_path);
}

#[tokio::test]
async fn node_provider_access_issues_ephemeral_codex_lease_without_persisting_token() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    set_session_runtime_state(&state, &detail, RuntimeSessionState::Ready).await;
    let turn = send_turn(
        State(state.clone()),
        Path(detail.session.session_thread_id.to_string()),
        Json(SendTurnRequest {
            content: "inspect available tools".to_owned(),
        }),
    )
    .await
    .expect("turn sends")
    .0;
    let rotated = rotate_node_credential(State(state.clone()), Path(node_id.to_string()))
        .await
        .expect("node credential rotates")
        .0;
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-uprava-node-id",
        HeaderValue::from_str(node_id.as_str()).expect("node header builds"),
    );
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", rotated.credential))
            .expect("authorization header builds"),
    );

    let access = node_provider_mcp_access(
        State(state.clone()),
        headers,
        Json(ProviderMcpAccessRequest {
            command_id: turn.command_id.clone(),
        }),
    )
    .await
    .expect("provider access issues")
    .0;
    let token = access.access_token.expose_secret().to_owned();
    let claims = validate_mcp_access_lease(&state, &token)
        .await
        .expect("issued lease validates");
    let command_json: String =
        sqlx::query_scalar("select command_json from commands where command_id = ?1")
            .bind(turn.command_id.as_str())
            .fetch_one(&state.pool)
            .await
            .expect("command loads");
    let audit_metadata: String = sqlx::query_scalar(
        "select metadata_json from security_audit_events where kind = 'provider.mcp_access.issued' order by happened_at desc limit 1",
    )
    .fetch_one(&state.pool)
    .await
    .expect("provider access audit loads");

    assert_eq!(access.endpoint_url, "/mcp");
    assert_eq!(claims.session_thread_id, detail.session.session_thread_id);
    assert!(!command_json.contains(&token));
    assert!(!audit_metadata.contains(&token));
    let _ = std::fs::remove_dir_all(workspace_path);
}

#[tokio::test]
async fn node_provider_access_issues_lease_for_managed_runtime_start_and_resume() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    let capabilities = std::iter::once(CapabilitySummary {
        key: "provider.codex".to_owned(),
        value: CapabilityValue::provider(true),
    })
    .chain(
        ProviderRuntimeCapability::required_for_managed_codex()
            .iter()
            .map(|capability| CapabilitySummary {
                key: capability.as_str().to_owned(),
                value: CapabilityValue::Provider {
                    available: true,
                    configured: true,
                    mode: "managed".to_owned(),
                    timeout_seconds: None,
                    unavailable_reason: None,
                },
            }),
    )
    .collect();
    set_node_capabilities(&state, &node_id, capabilities).await;
    let managed = create_session(
        State(state.clone()),
        Json(CreateSessionRequest {
            project_placement_id: detail.placement.project_placement_id,
            title: Some("Managed provider access".to_owned()),
            provider: "codex".to_owned(),
            execution_profile: Some(AgentExecutionProfile::Managed),
            force: false,
        }),
    )
    .await
    .expect("managed session creates")
    .0;
    let command_id: String = sqlx::query_scalar(
        "select command_id from commands where runtime_session_id = ?1 and kind = 'StartRuntime'",
    )
    .bind(managed.session.runtime.runtime_session_id.as_str())
    .fetch_one(&state.pool)
    .await
    .expect("managed start command loads");
    let rotated = rotate_node_credential(State(state.clone()), Path(node_id.to_string()))
        .await
        .expect("node credential rotates")
        .0;
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-uprava-node-id",
        HeaderValue::from_str(node_id.as_str()).expect("node header builds"),
    );
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", rotated.credential))
            .expect("authorization header builds"),
    );

    let access = node_provider_mcp_access(
        State(state.clone()),
        headers.clone(),
        Json(ProviderMcpAccessRequest {
            command_id: CommandId::from(command_id),
        }),
    )
    .await
    .expect("managed runtime provider access issues")
    .0;
    let claims = validate_mcp_access_lease(&state, access.access_token.expose_secret())
        .await
        .expect("managed runtime lease validates");

    assert_eq!(access.endpoint_url, "/mcp");
    assert_eq!(claims.session_thread_id, managed.session.session_thread_id);

    sqlx::query("update runtime_sessions set state = ?1 where runtime_session_id = ?2")
        .bind(format_runtime_state(RuntimeSessionState::Stopped))
        .bind(managed.session.runtime.runtime_session_id.as_str())
        .execute(&state.pool)
        .await
        .expect("managed runtime stops");
    sqlx::query("update session_threads set state = ?1 where session_thread_id = ?2")
        .bind(format_session_state(SessionThreadState::Stopped))
        .bind(managed.session.session_thread_id.as_str())
        .execute(&state.pool)
        .await
        .expect("managed session stops");
    let resume = resume_runtime(
        State(state.clone()),
        Path(managed.session.runtime.runtime_session_id.to_string()),
    )
    .await
    .expect("managed runtime resume command records")
    .0;
    let resume_access = node_provider_mcp_access(
        State(state.clone()),
        headers,
        Json(ProviderMcpAccessRequest {
            command_id: resume.command_id,
        }),
    )
    .await
    .expect("stopped managed runtime receives scoped resume access")
    .0;
    let resume_claims =
        validate_mcp_access_lease(&state, resume_access.access_token.expose_secret())
            .await
            .expect("managed resume lease validates");
    assert_eq!(
        resume_claims.session_thread_id,
        managed.session.session_thread_id
    );
    sqlx::query("update commands set state = 'failed' where command_id = ?1")
        .bind(
            resume_claims
                .resume_command_id
                .as_ref()
                .expect("resume command id")
                .as_str(),
        )
        .execute(&state.pool)
        .await
        .expect("resume command fails");
    let rejected = validate_mcp_access_lease(&state, resume_access.access_token.expose_secret())
        .await
        .expect_err("failed resume closes stopped-session lease scope");
    assert_eq!(rejected.code, ToolExecutionErrorCode::ScopeMismatch);
    let _ = std::fs::remove_dir_all(workspace_path);
}

#[tokio::test]
async fn concurrent_mcp_lease_rotation_serializes_on_sqlite() {
    let database_path = std::env::temp_dir().join(format!(
        "uprava-mcp-lease-concurrency-{}.sqlite",
        Uuid::new_v4()
    ));
    remove_sqlite_file_set(&database_path);
    let state = Arc::new(
        AppState::new(
            test_config(86_400),
            sqlite_file_pool_with_connections(&database_path, 4).await,
        )
        .await
        .expect("state migrates"),
    );
    let (_node_id, detail, workspace_path) = create_test_session(&state).await;
    let first_actor = ActorRef::Provider {
        provider: "codex".to_owned(),
    };
    let second_actor = first_actor.clone();

    let (first, second) = tokio::join!(
        issue_mcp_access_lease(&state, &detail.session.session_thread_id, first_actor),
        issue_mcp_access_lease(&state, &detail.session.session_thread_id, second_actor)
    );

    first.expect("first concurrent lease issues");
    second.expect("second concurrent lease issues");
    let active_leases: i64 = sqlx::query_scalar(
        "select count(*) from mcp_access_leases where session_thread_id = ?1 and revoked_at is null",
    )
    .bind(detail.session.session_thread_id.as_str())
    .fetch_one(&state.pool)
    .await
    .expect("active leases count loads");
    assert_eq!(active_leases, 1);

    let _ = std::fs::remove_dir_all(workspace_path);
    drop(state);
    remove_sqlite_file_set(&database_path);
}

#[tokio::test]
async fn mcp_streamable_http_lists_only_meta_tools_and_searches_registry() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    let actor = ActorRef::Provider {
        provider: "codex".to_owned(),
    };
    let scope = scope_for(&node_id, &detail, actor.clone());
    let (access_token, _) = state
        .create_mcp_access_lease(&detail.session.session_thread_id, actor)
        .await
        .expect("MCP lease issues");
    let authorization = format!("Bearer {access_token}");
    let app = build_router(state.clone());
    let initialize = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "uprava-test", "version": "1"}
        }
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("host", "localhost")
                .header(AUTHORIZATION, &authorization)
                .header(CONTENT_TYPE, "application/json")
                .header("accept", "application/json, text/event-stream")
                .body(Body::from(initialize.to_string()))
                .expect("initialize request builds"),
        )
        .await
        .expect("initialize response returns");
    assert_eq!(response.status(), StatusCode::OK);
    let session_id = response
        .headers()
        .get("mcp-session-id")
        .expect("MCP session id returned")
        .to_str()
        .expect("MCP session id is utf8")
        .to_owned();
    let initialize_body = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("initialize body reads");
    assert!(!initialize_body.is_empty());

    let initialized = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("host", "localhost")
                .header(AUTHORIZATION, &authorization)
                .header(CONTENT_TYPE, "application/json")
                .header("accept", "application/json, text/event-stream")
                .header("mcp-session-id", &session_id)
                .header("mcp-protocol-version", "2025-11-25")
                .body(Body::from(
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "notifications/initialized"
                    })
                    .to_string(),
                ))
                .expect("initialized request builds"),
        )
        .await
        .expect("initialized response returns");
    assert_eq!(initialized.status(), StatusCode::ACCEPTED);

    let list_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("host", "localhost")
                .header(AUTHORIZATION, &authorization)
                .header(CONTENT_TYPE, "application/json")
                .header("accept", "application/json, text/event-stream")
                .header("mcp-session-id", &session_id)
                .header("mcp-protocol-version", "2025-11-25")
                .body(Body::from(
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": 2,
                        "method": "tools/list",
                        "params": {}
                    })
                    .to_string(),
                ))
                .expect("list request builds"),
        )
        .await
        .expect("list response returns");
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_headers = list_response.headers().clone();
    let list_bytes = to_bytes(list_response.into_body(), 64 * 1024)
        .await
        .expect("list body reads");
    assert!(
        !list_bytes.is_empty(),
        "list body is empty; headers: {list_headers:?}"
    );
    let list_body = parse_mcp_http_body(&list_bytes);
    let names = list_body["result"]["tools"]
        .as_array()
        .expect("tools list exists")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["search_tools", "inspect_tool", "execute_tool"]);

    let search_request = SearchToolsRequest {
        scope,
        query: "session inspect".to_owned(),
        filters: ToolSearchFilters::default(),
        cursor: None,
        limit: Some(10),
    };
    let search_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("host", "localhost")
                .header(AUTHORIZATION, authorization)
                .header(CONTENT_TYPE, "application/json")
                .header("accept", "application/json, text/event-stream")
                .header("mcp-session-id", session_id)
                .header("mcp-protocol-version", "2025-11-25")
                .body(Body::from(
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": 3,
                        "method": "tools/call",
                        "params": {
                            "name": "search_tools",
                            "arguments": search_request
                        }
                    })
                    .to_string(),
                ))
                .expect("search request builds"),
        )
        .await
        .expect("search response returns");
    assert_eq!(search_response.status(), StatusCode::OK);
    let search_bytes = to_bytes(search_response.into_body(), 128 * 1024)
        .await
        .expect("search body reads");
    let search_body = parse_mcp_http_body(&search_bytes);
    assert!(search_body["result"]["structuredContent"]["items"]
        .as_array()
        .expect("search items exist")
        .iter()
        .any(|item| item["tool_id"] == "uprava.session.inspect"));
    let _ = std::fs::remove_dir_all(workspace_path);
}

fn parse_mcp_http_body(body: &[u8]) -> serde_json::Value {
    let text = std::str::from_utf8(body).expect("MCP response is utf8");
    if let Ok(value) = serde_json::from_str(text) {
        return value;
    }
    text.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .find_map(|data| serde_json::from_str(data).ok())
        .unwrap_or_else(|| panic!("MCP response contains no JSON-RPC payload: {text:?}"))
}
