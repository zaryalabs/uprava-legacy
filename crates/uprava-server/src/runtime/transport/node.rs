//! Node HTTP/control ingress and WebSocket terminal transport.

use uprava_protocol::ObservedCapability;

use super::super::*;

pub(crate) async fn inventory(
    State(state): State<Arc<AppState>>,
) -> Result<Json<InventorySnapshot>, AppError> {
    Ok(Json(load_inventory(&state).await?))
}

pub(crate) async fn nodes(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<uprava_protocol::NodeSummary>>, AppError> {
    Ok(Json(load_nodes(&state).await?))
}

pub(crate) async fn node_detail(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
) -> Result<Json<uprava_protocol::NodeSummary>, AppError> {
    let node_id = NodeId::from(node_id);
    load_nodes(&state)
        .await?
        .into_iter()
        .find(|node| node.node_id == node_id)
        .map(Json)
        .ok_or_else(|| AppError::not_found("node.not_found", "Node not found"))
}

pub(crate) async fn node_enrollments(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<NodeEnrollmentSummary>>, AppError> {
    load_enrollments(&state).await.map(Json)
}

pub(crate) async fn create_client_node_enrollment(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ClientCreateNodeEnrollmentRequest>,
) -> Result<Json<NodeEnrollmentRequestedResponse>, AppError> {
    create_enrollment(&state, &request.display_name, None, Vec::new())
        .await
        .map(Json)
}

pub(crate) async fn node_enrollment_request(
    State(state): State<Arc<AppState>>,
    Json(request): Json<NodeEnrollmentRequest>,
) -> Result<Json<NodeEnrollmentRequestedResponse>, AppError> {
    create_enrollment(
        &state,
        &request.display_name,
        Some(&request.daemon_version),
        request.capabilities,
    )
    .await
    .map(Json)
}

pub(crate) async fn approve_node_enrollment(
    State(state): State<Arc<AppState>>,
    Path(enrollment_id): Path<String>,
) -> Result<Json<ApproveNodeEnrollmentResponse>, AppError> {
    let now = Utc::now();
    let enrollment_id = EnrollmentId::from(enrollment_id);
    let updated = sqlx::query(
        r#"
        update node_enrollments
        set status = 'approved', approved_at = ?1, updated_at = ?1
        where enrollment_id = ?2
          and status = 'pending_user_approval'
          and approved_at is null
          and expires_at > ?1
        "#,
    )
    .bind(now)
    .bind(enrollment_id.as_str())
    .execute(&state.pool)
    .await?
    .rows_affected();
    if updated == 0 {
        return Err(AppError::bad_request(
            "node_enrollment.not_approvable",
            "Enrollment is missing, expired or already claimed",
        ));
    }
    let enrollment = load_enrollment(&state, &enrollment_id).await?;
    tracing::info!("node enrollment approved");
    Ok(Json(ApproveNodeEnrollmentResponse { enrollment }))
}

pub(crate) async fn node_enrollment_claim(
    State(state): State<Arc<AppState>>,
    Json(request): Json<NodeEnrollmentClaimRequest>,
) -> Result<Json<NodeEnrollmentClaimResponse>, AppError> {
    claim_enrollment(&state, &request).await.map(Json)
}

pub(crate) async fn revoke_node(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
) -> Result<Json<NodeRevocationResponse>, AppError> {
    let node_id = NodeId::from(node_id);
    let updated = sqlx::query(
        r#"
        update nodes
        set presence = 'revoked', credential_hash = null, updated_at = ?1
        where node_id = ?2
        "#,
    )
    .bind(Utc::now())
    .bind(node_id.as_str())
    .execute(&state.pool)
    .await?
    .rows_affected();
    if updated == 0 {
        return Err(AppError::not_found("node.not_found", "Node not found"));
    }
    audit_security_event(
        &state,
        "node.credential.revoked",
        Some(&node_id),
        None,
        "accepted",
        JsonValue(json!({})),
    )
    .await?;
    tracing::warn!("node revoked");
    Ok(Json(NodeRevocationResponse {
        node_id,
        revoked: true,
    }))
}

pub(crate) async fn rotate_node_credential(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
) -> Result<Json<NodeCredentialRotationResponse>, AppError> {
    let node_id = NodeId::from(node_id);
    let now = Utc::now();
    let credential = new_secret("node");
    let credential_hash = hash_secret(&credential);
    let updated = sqlx::query(
        r#"
        update nodes
        set credential_hash = ?1, updated_at = ?2
        where node_id = ?3 and presence != 'revoked'
        "#,
    )
    .bind(credential_hash)
    .bind(now)
    .bind(node_id.as_str())
    .execute(&state.pool)
    .await?
    .rows_affected();
    if updated == 0 {
        return Err(AppError::not_found(
            "node.not_found_or_revoked",
            "Node is missing or revoked",
        ));
    }
    audit_security_event(
        &state,
        "node.credential.rotated",
        Some(&node_id),
        None,
        "accepted",
        JsonValue(json!({})),
    )
    .await?;
    tracing::warn!("node credential rotated");
    Ok(Json(NodeCredentialRotationResponse {
        node_id,
        credential,
        rotated_at: now,
    }))
}

pub(crate) async fn delete_node(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
) -> Result<Json<NodeDeletionResponse>, AppError> {
    let node_id = NodeId::from(node_id);
    let mut transaction = state.pool.begin().await?;
    let exists = sqlx::query_scalar::<_, i64>("select 1 from nodes where node_id = ?1")
        .bind(node_id.as_str())
        .fetch_optional(&mut *transaction)
        .await?
        .is_some();
    if !exists {
        return Err(AppError::not_found("node.not_found", "Node not found"));
    }

    let deleted_sessions = sqlx::query_scalar::<_, i64>(
        r#"
        select count(*)
        from session_threads st
        join project_placements pp on pp.project_placement_id = st.project_placement_id
        where pp.node_id = ?1
        "#,
    )
    .bind(node_id.as_str())
    .fetch_one(&mut *transaction)
    .await?;
    let deleted_placements =
        sqlx::query_scalar::<_, i64>("select count(*) from project_placements where node_id = ?1")
            .bind(node_id.as_str())
            .fetch_one(&mut *transaction)
            .await?;

    for statement in [
        r#"
        delete from events
        where node_id = ?1
           or runtime_session_id in (
                select rs.runtime_session_id
                from runtime_sessions rs
                join session_threads st on st.session_thread_id = rs.session_thread_id
                join project_placements pp on pp.project_placement_id = st.project_placement_id
                where pp.node_id = ?1
           )
           or session_thread_id in (
                select st.session_thread_id
                from session_threads st
                join project_placements pp on pp.project_placement_id = st.project_placement_id
                where pp.node_id = ?1
           )
        "#,
        r#"
        delete from warning_acknowledgements
        where session_thread_id in (
            select st.session_thread_id
            from session_threads st
            join project_placements pp on pp.project_placement_id = st.project_placement_id
            where pp.node_id = ?1
        )
        "#,
        r#"
        delete from approvals
        where session_thread_id in (
            select st.session_thread_id
            from session_threads st
            join project_placements pp on pp.project_placement_id = st.project_placement_id
            where pp.node_id = ?1
        )
        "#,
        r#"
        delete from messages
        where session_thread_id in (
            select st.session_thread_id
            from session_threads st
            join project_placements pp on pp.project_placement_id = st.project_placement_id
            where pp.node_id = ?1
        )
        "#,
        r#"
        delete from turns
        where session_thread_id in (
            select st.session_thread_id
            from session_threads st
            join project_placements pp on pp.project_placement_id = st.project_placement_id
            where pp.node_id = ?1
        )
        "#,
        r#"
        delete from runtime_sessions
        where session_thread_id in (
            select st.session_thread_id
            from session_threads st
            join project_placements pp on pp.project_placement_id = st.project_placement_id
            where pp.node_id = ?1
        )
        "#,
        r#"
        delete from session_threads
        where project_placement_id in (
            select project_placement_id from project_placements where node_id = ?1
        )
        "#,
        "delete from task_runs where node_id = ?1",
        "delete from commands where target_node_id = ?1",
        "delete from deleted_workspace_bindings where node_id = ?1",
        "delete from project_placements where node_id = ?1",
        "delete from node_capabilities where node_id = ?1",
        "delete from node_enrollments where claimed_node_id = ?1",
    ] {
        sqlx::query(statement)
            .bind(node_id.as_str())
            .execute(&mut *transaction)
            .await?;
    }

    let deleted = sqlx::query("delete from nodes where node_id = ?1")
        .bind(node_id.as_str())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
    transaction.commit().await?;
    state.control_connections.remove_node(&node_id).await;

    if deleted == 0 {
        return Err(AppError::not_found("node.not_found", "Node not found"));
    }
    tracing::warn!(deleted_placements, deleted_sessions, "node deleted");
    Ok(Json(NodeDeletionResponse {
        node_id,
        deleted: true,
    }))
}

pub(crate) async fn node_heartbeat_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<NodeHeartbeatRequest>,
) -> Result<Json<NodeHeartbeatResponse>, AppError> {
    let credential = bearer_token(&headers).ok_or_else(|| {
        AppError::auth(
            "auth_dev.credential_required",
            "Node credential is required",
        )
    })?;
    node_heartbeat(State(state), Some(credential.as_str()), Json(request)).await
}

pub(crate) async fn node_heartbeat(
    State(state): State<Arc<AppState>>,
    credential: Option<&str>,
    Json(request): Json<NodeHeartbeatRequest>,
) -> Result<Json<NodeHeartbeatResponse>, AppError> {
    let now = Utc::now();
    let node_id = request
        .node_id
        .ok_or_else(|| AppError::auth("auth_dev.node_id_required", "Node id is required"))?;
    verify_node_credential(&state, &node_id, credential).await?;
    let display_name = request.display_name;
    let daemon_version = request.daemon_version;
    let active_runtime_count = request.active_runtime_count;
    let capabilities = request.capabilities;
    let observed_capabilities = request.observed_capabilities;
    if observed_capabilities
        .iter()
        .any(|capability| capability.node_id != node_id)
    {
        return Err(AppError::bad_request(
            "node.observed_capability_owner_mismatch",
            "Observed capability belongs to another Node",
        ));
    }
    let dependency_statuses = request.dependency_statuses;
    if dependency_statuses
        .iter()
        .any(|status| status.node_id != node_id)
    {
        return Err(AppError::bad_request(
            "node.dependency_status_owner_mismatch",
            "Dependency status belongs to another Node",
        ));
    }
    let diagnostics = request
        .diagnostics
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "heartbeat accepted".to_owned());
    let workspace_summaries = request.workspace_summaries;
    let workspace_count = workspace_summaries.len();
    let capabilities_json = serde_json::to_string(&capabilities)?;
    sqlx::query(
        r#"
        insert into nodes (
            node_id, display_name, presence, sleep_hint, last_heartbeat_at,
            daemon_version, active_runtime_count, capabilities_json, diagnostics, created_at, updated_at
        )
        values (?1, ?2, 'reachable', ?3, ?4, ?5, ?6, ?7, ?8, ?4, ?4)
        on conflict(node_id) do update set
            display_name = excluded.display_name,
            presence = 'reachable',
            sleep_hint = excluded.sleep_hint,
            last_heartbeat_at = excluded.last_heartbeat_at,
            daemon_version = excluded.daemon_version,
            active_runtime_count = excluded.active_runtime_count,
            capabilities_json = excluded.capabilities_json,
            diagnostics = excluded.diagnostics,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(node_id.as_str())
    .bind(display_name)
    .bind(format_sleep_hint(request.sleep_hint))
    .bind(now)
    .bind(daemon_version)
    .bind(active_runtime_count)
    .bind(&capabilities_json)
    .bind(diagnostics)
    .execute(&state.pool)
    .await?;

    replace_node_capabilities(&state, &node_id, &capabilities, now).await?;
    replace_observed_capabilities(&state, &node_id, &observed_capabilities).await?;
    let mut refresh_definitions = false;
    for dependency_status in dependency_statuses {
        let previous_state: Option<String> = sqlx::query_scalar(
            "select actual_state from mcp_dependency_instances where dependency_instance_id = ?1",
        )
        .bind(dependency_status.dependency_instance_id.as_str())
        .fetch_optional(&state.pool)
        .await?;
        refresh_definitions |= dependency_status.actual_state == McpDependencyActualState::Running
            && previous_state.as_deref() != Some("running");
        persist_dependency_status(&state, &dependency_status).await?;
    }
    upsert_heartbeat_workspaces(&state, &node_id, workspace_summaries).await?;
    let open_control_channel = should_open_control_channel(&state, &node_id).await?;
    if refresh_definitions {
        dispatch_dependency_desired_snapshots(&state, &node_id).await?;
    }
    tracing::debug!(
        active_runtime_count,
        workspace_count,
        open_control_channel,
        "node heartbeat accepted"
    );

    Ok(Json(NodeHeartbeatResponse {
        accepted: true,
        node_id: node_id.clone(),
        open_control_channel,
        server_time: now,
    }))
}

pub(crate) async fn node_provider_mcp_access(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<ProviderMcpAccessRequest>,
) -> Result<Json<ProviderMcpAccess>, AppError> {
    let node_id = header_value(&headers, "x-uprava-node-id")
        .map(NodeId::from)
        .ok_or_else(|| AppError::auth("auth_dev.node_id_required", "Node id is required"))?;
    let credential = bearer_token(&headers).ok_or_else(|| {
        AppError::auth(
            "auth_dev.credential_required",
            "Node credential is required",
        )
    })?;
    verify_node_credential(&state, &node_id, Some(&credential)).await?;

    let (command_json, command_state): (String, String) = sqlx::query_as(
        "select command_json, state from commands where command_id = ?1 and target_node_id = ?2",
    )
    .bind(request.command_id.as_str())
    .bind(node_id.as_str())
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| {
        AppError::not_found("provider.command_not_found", "Provider command not found")
    })?;
    if matches!(
        command_state.as_str(),
        "completed" | "failed" | "blocked" | "expired"
    ) {
        return Err(AppError::bad_request(
            "provider.command_terminal",
            "Terminal provider command cannot receive new MCP access",
        ));
    }
    let command: CommandEnvelope = serde_json::from_str(&command_json)?;
    let managed_process_start = matches!(
        &command.payload,
        CommandPayload::StartRuntime {
            execution_profile: AgentExecutionProfile::Managed,
            ..
        } | CommandPayload::ResumeRuntime {
            execution_profile: AgentExecutionProfile::Managed,
            ..
        }
    );
    if command.kind != CommandKind::SendTurn && !managed_process_start {
        return Err(AppError::bad_request(
            "provider.command_not_eligible",
            "Only a session turn or Managed runtime start can receive provider MCP access",
        ));
    }
    let session_thread_id = command.target.session_thread_id().cloned().ok_or_else(|| {
        AppError::bad_request(
            "provider.session_required",
            "Provider command is missing a session target",
        )
    })?;
    let (provider, execution_profile): (String, String) = sqlx::query_as(
        r#"
        select rs.provider, rs.execution_profile
        from runtime_sessions rs
        where rs.session_thread_id = ?1
        order by rs.updated_at desc
        limit 1
        "#,
    )
    .bind(session_thread_id.as_str())
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("provider.runtime_not_found", "Runtime not found"))?;
    if provider != "codex" {
        return Err(AppError::bad_request(
            "provider.mcp_delivery_unsupported",
            "Provider does not support Uprava MCP delivery",
        ));
    }

    let managed_runtime = managed_process_start || execution_profile == "managed";
    let actor_ref = ActorRef::Provider { provider };
    let (access_token, claims) = if command.kind == CommandKind::ResumeRuntime {
        issue_mcp_access_lease_for_resume(
            &state,
            &session_thread_id,
            actor_ref,
            &request.command_id,
        )
        .await?
    } else if managed_runtime {
        issue_managed_mcp_access_lease(&state, &session_thread_id, actor_ref).await?
    } else {
        issue_mcp_access_lease(&state, &session_thread_id, actor_ref).await?
    };
    audit_security_event(
        &state,
        "provider.mcp_access.issued",
        Some(&node_id),
        Some(session_thread_id.to_string()),
        "accepted",
        JsonValue(json!({
            "command_id": request.command_id,
            "lease_id": claims.lease_id,
            "expires_at": claims.expires_at,
        })),
    )
    .await?;

    Ok(Json(ProviderMcpAccess {
        endpoint_url: "/mcp".to_owned(),
        access_token: McpAccessToken::new(access_token),
        expires_at: claims.expires_at,
    }))
}

pub(crate) async fn replace_observed_capabilities(
    state: &AppState,
    node_id: &NodeId,
    capabilities: &[ObservedCapability],
) -> Result<(), AppError> {
    let mut transaction = state.pool.begin().await?;
    sqlx::query("delete from observed_capabilities where node_id = ?1")
        .bind(node_id.as_str())
        .execute(&mut *transaction)
        .await?;
    for capability in capabilities {
        sqlx::query(
            "insert into observed_capabilities (node_id, capability_key, capability_json, observed_at) values (?1, ?2, ?3, ?4)",
        )
        .bind(node_id.as_str())
        .bind(&capability.capability_key)
        .bind(serde_json::to_string(capability)?)
        .bind(capability.observed_at)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(())
}

pub(crate) async fn node_control(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse, AppError> {
    let node_id = header_value(&headers, "x-uprava-node-id")
        .map(NodeId::from)
        .ok_or_else(|| AppError::auth("auth_dev.node_id_required", "Node id is required"))?;
    let credential = bearer_token(&headers).ok_or_else(|| {
        AppError::auth(
            "auth_dev.credential_required",
            "Node credential is required",
        )
    })?;
    verify_node_credential(&state, &node_id, Some(&credential)).await?;

    Ok(ws.on_upgrade(move |socket| handle_control_socket(state, node_id, socket)))
}

pub(crate) async fn handle_control_socket(
    state: Arc<AppState>,
    node_id: NodeId,
    socket: WebSocket,
) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::channel::<ControlFrame>(CONTROL_QUEUE_CAPACITY);
    let context = state.control_connections.context(node_id.clone(), tx);
    tracing::info!(
        generation = context.generation,
        "node control socket connected"
    );

    let send_task = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            let Ok(text) = serde_json::to_string(&frame) else {
                continue;
            };
            if sender.send(WsMessage::Text(text.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(message) = receiver.next().await {
        let Ok(WsMessage::Text(text)) = message else {
            continue;
        };
        if text.len() > MAX_CONTROL_FRAME_BYTES {
            tracing::warn!(frame_bytes = text.len(), "oversized control frame rejected");
            continue;
        }
        match serde_json::from_str::<ControlFrame>(&text) {
            Ok(frame) => {
                if let Err(error) = handle_node_control_frame(&state, &context, frame).await {
                    tracing::warn!(generation = context.generation, error = %error, "control frame failed");
                }
            }
            Err(error) => {
                tracing::warn!(error = %error, "invalid control frame");
            }
        }
    }

    let removed = state.control_connections.remove_if_active(&context).await;
    tracing::info!(
        generation = context.generation,
        removed,
        "node control socket disconnected"
    );
    send_task.abort();
}

pub(crate) async fn handle_node_control_frame(
    state: &AppState,
    context: &NodeContext,
    frame: ControlFrame,
) -> Result<(), AppError> {
    let node_id = &context.node_id;
    validate_control_frame_limits(&frame)?;
    if !matches!(frame, ControlFrame::Hello { .. })
        && !is_supported_protocol_version(control_frame_protocol_version(&frame))
    {
        send_control_error(
            state,
            context,
            "control.protocol_incompatible",
            "Control protocol version is incompatible",
            false,
        )
        .await;
        return Err(AppError::bad_request(
            "control.protocol_incompatible",
            "Control protocol version is incompatible",
        ));
    }

    match frame {
        ControlFrame::Hello {
            protocol_version,
            node_id: hello_node_id,
            active_runtime_ids,
            actual_runtime_attempts,
            ..
        } => {
            if hello_node_id != *node_id {
                send_control_error(
                    state,
                    context,
                    "control.node_mismatch",
                    "Control hello node id does not match authenticated node",
                    false,
                )
                .await;
                return Err(AppError::auth(
                    "control.node_mismatch",
                    "Control hello node id does not match authenticated node",
                ));
            }
            if !is_supported_protocol_version(&protocol_version) {
                send_control_error(
                    state,
                    context,
                    "control.protocol_incompatible",
                    "Control protocol version is incompatible",
                    false,
                )
                .await;
                return Err(AppError::bad_request(
                    "control.protocol_incompatible",
                    "Control protocol version is incompatible",
                ));
            }
            if !state.control_connections.activate(context).await {
                return Err(AppError::auth(
                    "control.stale_generation",
                    "A newer control connection is already active",
                ));
            }
            reconcile_node_runtime_actual_state(
                state,
                node_id,
                &active_runtime_ids,
                &actual_runtime_attempts,
            )
            .await?;
            send_control_frame(
                state,
                node_id,
                ControlFrame::HelloAck {
                    frame_id: Uuid::new_v4().to_string(),
                    protocol_version: API_VERSION.to_owned(),
                    sent_at: Utc::now(),
                },
            )
            .await;
            dispatch_dependency_desired_snapshots(state, node_id).await?;
            dispatch_pending_commands(state, node_id).await
        }
        ControlFrame::CommandAck {
            command_id, status, ..
        } => {
            require_active_generation(state, context).await?;
            validate_command_ack(state, node_id, &command_id, status).await?;
            tracing::debug!(
                command_state = ?status,
                "node command acknowledged"
            );
            update_command_state(state, &command_id, status).await?;
            mark_deduction_running(state, &command_id).await
        }
        ControlFrame::CommandResult {
            command_id,
            status,
            payload,
            ..
        } => {
            require_active_generation(state, context).await?;
            let durable_payload = durable_command_result_payload(&payload);
            validate_command_result(state, node_id, &command_id, status, &durable_payload).await?;
            tracing::info!(
                command_state = ?status,
                "node command result received"
            );
            state
                .core_metrics
                .command_results
                .fetch_add(1, Ordering::Relaxed);
            update_command_result(state, &command_id, status, &durable_payload).await?;
            project_task_command_result(state, &command_id, status, &durable_payload).await?;
            project_deduction_command_result(state, &command_id, status, &durable_payload).await?;
            let project_terminal_call = !command_waiter_exists(state, &command_id)?;
            project_tooling_command_result(
                state,
                &command_id,
                status,
                &durable_payload,
                project_terminal_call,
            )
            .await?;
            let durable_notice = CommandResultNotice {
                command_id: command_id.clone(),
                status,
                payload: durable_payload,
            };
            let _ = state.command_result_tx.send(durable_notice.clone());
            let waiter = {
                let mut waiters = lock_command_waiters(state)?;
                waiters.remove(durable_notice.command_id.as_str())
            };
            if let Some(waiter) = waiter {
                let _ = waiter.send(CommandResultNotice {
                    command_id,
                    status,
                    payload,
                });
            }
            Ok(())
        }
        ControlFrame::EventBatch { events, .. } => {
            require_active_generation(state, context).await?;
            if events.len() > MAX_EVENT_BATCH_ITEMS {
                return Err(AppError::bad_request(
                    "control.event_batch_too_large",
                    "Control event batch exceeds the item limit",
                ));
            }
            let batch_bytes = events
                .iter()
                .map(serde_json::to_vec)
                .collect::<Result<Vec<_>, _>>()?
                .iter()
                .map(Vec::len)
                .sum::<usize>();
            if batch_bytes > MAX_CONTROL_FRAME_BYTES {
                return Err(AppError::bad_request(
                    "control.event_batch_bytes_too_large",
                    "Control event batch exceeds the byte limit",
                ));
            }
            let event_count = events.len();
            let mut accepted_event_ids = Vec::with_capacity(events.len());
            for event in events {
                validate_event_owner(state, node_id, &event).await?;
                let event_id = event.event_id.clone();
                accept_node_event(state, event).await?;
                accepted_event_ids.push(event_id);
            }
            let _ = send_control_frame(
                state,
                node_id,
                ControlFrame::EventBatchAck {
                    frame_id: Uuid::new_v4().to_string(),
                    protocol_version: API_VERSION.to_owned(),
                    sent_at: Utc::now(),
                    accepted_event_ids,
                },
            )
            .await;
            tracing::info!(event_count, "node event batch accepted");
            Ok(())
        }
        ControlFrame::WorkspaceTerminalOutput {
            terminal_id,
            seq,
            data,
            sent_at,
            ..
        } => {
            require_active_generation(state, context).await?;
            if data.chars().count() > MAX_TERMINAL_OUTPUT_CHARS {
                return Err(AppError::bad_request(
                    "control.terminal_output_too_large",
                    "Terminal output frame exceeds the character limit",
                ));
            }
            validate_terminal_owner(state, node_id, &terminal_id).await?;
            state
                .terminal_hub
                .publish(
                    &terminal_id,
                    TerminalFrameNotice::Output {
                        terminal_id: terminal_id.clone(),
                        seq,
                        data,
                        sent_at,
                    },
                )
                .await;
            state
                .core_metrics
                .pty_terminal_states
                .fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        ControlFrame::WorkspaceTerminalStatus {
            terminal_id,
            state: terminal_state,
            exit_code,
            message,
            sent_at,
            ..
        } => {
            require_active_generation(state, context).await?;
            validate_terminal_owner(state, node_id, &terminal_id).await?;
            if let Some(terminal) = state
                .workspace_terminals
                .write()
                .await
                .get_mut(terminal_id.as_str())
            {
                terminal.state = terminal_state;
                terminal.exit_code = exit_code;
                terminal.updated_at = sent_at;
            }
            state
                .terminal_hub
                .publish(
                    &terminal_id,
                    TerminalFrameNotice::Status {
                        terminal_id: terminal_id.clone(),
                        state: terminal_state,
                        exit_code,
                        message,
                        sent_at,
                    },
                )
                .await;
            Ok(())
        }
        _ => Ok(()),
    }
}

async fn reconcile_node_runtime_actual_state(
    state: &AppState,
    node_id: &NodeId,
    active_runtime_ids: &[RuntimeSessionId],
    actual_attempts: &[RuntimeAttemptActualState],
) -> Result<(), AppError> {
    let now = Utc::now();
    let mut transaction = state.pool.begin().await?;
    for actual in actual_attempts {
        let stored: Option<(Option<String>, Option<String>, String)> = sqlx::query_as(
            r#"
            select r.current_attempt_id, r.effective_policy_hash, r.execution_profile
            from runtime_sessions r
            join session_threads s on s.session_thread_id = r.session_thread_id
            join project_placements p on p.project_placement_id = s.project_placement_id
            where r.runtime_session_id = ?1 and p.node_id = ?2
            "#,
        )
        .bind(actual.runtime_session_id.as_str())
        .bind(node_id.as_str())
        .fetch_optional(&mut *transaction)
        .await?;
        let Some((current_attempt_id, stored_policy_hash, profile)) = stored else {
            continue;
        };
        if profile != "managed"
            || stored_policy_hash.as_deref() != Some(actual.effective_policy_hash.as_str())
        {
            sqlx::query(
                "update runtime_sessions set state = 'error', recovery_status = 'failed', degraded_reason = 'Node actual-state policy mismatch', updated_at = ?1 where runtime_session_id = ?2",
            )
            .bind(now)
            .bind(actual.runtime_session_id.as_str())
            .execute(&mut *transaction)
            .await?;
            continue;
        }
        if let Some(current_attempt_id) = current_attempt_id.as_deref() {
            if current_attempt_id != actual.runtime_attempt_id.as_str() {
                continue;
            }
        } else {
            sqlx::query(
                r#"
                insert into runtime_attempts (
                    runtime_attempt_id, runtime_session_id, state, execution_profile,
                    effective_policy_json, effective_policy_hash, provider_version,
                    provider_resume_ref_json, start_reason, stop_reason, recovery_reason,
                    started_at, ready_at, stopped_at, updated_at
                )
                select ?1, runtime_session_id, ?2, execution_profile,
                       effective_policy_json, effective_policy_hash, null, null,
                       'node_reconnect_reconciliation', null, 'core_restart',
                       ?3, null, null, ?4
                from runtime_sessions where runtime_session_id = ?5
                on conflict(runtime_attempt_id) do nothing
                "#,
            )
            .bind(actual.runtime_attempt_id.as_str())
            .bind(format_runtime_attempt_state(actual.state))
            .bind(actual.started_at)
            .bind(now)
            .bind(actual.runtime_session_id.as_str())
            .execute(&mut *transaction)
            .await?;
            sqlx::query(
                "update runtime_sessions set current_attempt_id = ?1 where runtime_session_id = ?2 and current_attempt_id is null",
            )
            .bind(actual.runtime_attempt_id.as_str())
            .bind(actual.runtime_session_id.as_str())
            .execute(&mut *transaction)
            .await?;
        }
        let recovery_status = match actual.state {
            RuntimeAttemptState::Disconnected => "degraded",
            RuntimeAttemptState::Reconnecting => "reconnecting",
            RuntimeAttemptState::Recovered => "recovered",
            RuntimeAttemptState::Failed | RuntimeAttemptState::Lost => "failed",
            _ => "live",
        };
        let pending_interactions: i64 = sqlx::query_scalar(
            "select count(*) from provider_interactions where runtime_session_id = ?1 and state in ('requested', 'resolving')",
        )
        .bind(actual.runtime_session_id.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        let runtime_state = if pending_interactions > 0 {
            "blocked"
        } else {
            match actual.state {
                RuntimeAttemptState::Starting => "starting",
                RuntimeAttemptState::Ready | RuntimeAttemptState::Recovered => {
                    if actual.active_turn_id.is_some() {
                        "running"
                    } else {
                        "ready"
                    }
                }
                RuntimeAttemptState::Disconnected => "stale",
                RuntimeAttemptState::Reconnecting => "resuming",
                RuntimeAttemptState::Stopping => "stopping",
                RuntimeAttemptState::Stopped => "stopped",
                RuntimeAttemptState::Failed | RuntimeAttemptState::Lost => "error",
            }
        };
        sqlx::query(
            "update runtime_attempts set state = ?1, recovery_reason = 'node_actual_state', updated_at = ?2 where runtime_attempt_id = ?3",
        )
        .bind(format_runtime_attempt_state(actual.state))
        .bind(now)
        .bind(actual.runtime_attempt_id.as_str())
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "update runtime_sessions set state = ?1, recovery_status = ?2, degraded_reason = null, updated_at = ?3 where runtime_session_id = ?4 and current_attempt_id = ?5",
        )
        .bind(runtime_state)
        .bind(recovery_status)
        .bind(now)
        .bind(actual.runtime_session_id.as_str())
        .bind(actual.runtime_attempt_id.as_str())
        .execute(&mut *transaction)
        .await?;
    }

    let managed_rows: Vec<(String,)> = sqlx::query_as(
        r#"
        select r.runtime_session_id
        from runtime_sessions r
        join session_threads s on s.session_thread_id = r.session_thread_id
        join project_placements p on p.project_placement_id = s.project_placement_id
        where p.node_id = ?1 and r.execution_profile = 'managed'
          and r.current_attempt_id is not null
          and r.state not in ('stopped', 'expired', 'error')
        "#,
    )
    .bind(node_id.as_str())
    .fetch_all(&mut *transaction)
    .await?;
    for (runtime_session_id,) in managed_rows {
        let reported_live = active_runtime_ids
            .iter()
            .any(|id| id.as_str() == runtime_session_id)
            || actual_attempts
                .iter()
                .any(|attempt| attempt.runtime_session_id.as_str() == runtime_session_id);
        if !reported_live {
            sqlx::query(
                "update runtime_sessions set state = 'stale', recovery_status = 'provider_resumable', degraded_reason = 'Node did not report the managed runtime after reconnect', updated_at = ?1 where runtime_session_id = ?2",
            )
            .bind(now)
            .bind(&runtime_session_id)
            .execute(&mut *transaction)
            .await?;
        }
    }
    transaction.commit().await?;
    Ok(())
}

pub(crate) fn durable_command_result_payload(payload: &JsonValue) -> JsonValue {
    let mut value = payload.0.clone();
    if let Some(object) = value.as_object_mut() {
        object.remove("authorization_url");
    }
    JsonValue(value)
}

pub(crate) fn validate_control_frame_limits(frame: &ControlFrame) -> Result<(), AppError> {
    let encoded = serde_json::to_vec(frame)?;
    if encoded.len() > MAX_CONTROL_FRAME_BYTES {
        return Err(AppError::bad_request(
            "control.frame_too_large",
            "Control frame exceeds the byte limit",
        ));
    }
    let value = serde_json::to_value(frame)?;
    validate_control_json_value(&value, 0)
}

pub(crate) fn validate_control_json_value(
    value: &serde_json::Value,
    depth: usize,
) -> Result<(), AppError> {
    if depth > MAX_CONTROL_JSON_DEPTH {
        return Err(AppError::bad_request(
            "control.frame_too_deep",
            "Control frame exceeds the nesting limit",
        ));
    }
    match value {
        serde_json::Value::String(value) if value.chars().count() > MAX_CONTROL_STRING_CHARS => {
            Err(AppError::bad_request(
                "control.string_too_large",
                "Control frame contains an oversized string",
            ))
        }
        serde_json::Value::Array(values) => {
            for value in values {
                validate_control_json_value(value, depth + 1)?;
            }
            Ok(())
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                validate_control_json_value(value, depth + 1)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

pub(crate) async fn require_active_generation(
    state: &AppState,
    context: &NodeContext,
) -> Result<(), AppError> {
    if state.control_connections.is_active(context).await {
        return Ok(());
    }
    Err(AppError::auth(
        "control.stale_generation",
        "Control frame belongs to a stale or uninitialized connection",
    ))
}

pub(crate) async fn validate_command_ack(
    state: &AppState,
    node_id: &NodeId,
    command_id: &CommandId,
    status: CommandState,
) -> Result<(), AppError> {
    let (target_node_id, current_state): (String, String) =
        sqlx::query_as("select target_node_id, state from commands where command_id = ?1")
            .bind(command_id.as_str())
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| AppError::not_found("control.command_not_found", "Command not found"))?;
    if target_node_id != node_id.as_str() {
        return Err(AppError::auth(
            "control.command_owner_mismatch",
            "Command belongs to another Node",
        ));
    }
    if status != CommandState::Acknowledged {
        return Err(AppError::bad_request(
            "control.command_ack_state_invalid",
            "Command ACK must use acknowledged state",
        ));
    }
    if !matches!(
        parse_command_state(&current_state),
        CommandState::Dispatched | CommandState::Acknowledged
    ) {
        return Err(AppError::bad_request(
            "control.command_transition_invalid",
            "Command cannot be acknowledged from its current state",
        ));
    }
    Ok(())
}

pub(crate) async fn validate_command_result(
    state: &AppState,
    node_id: &NodeId,
    command_id: &CommandId,
    status: CommandState,
    payload: &JsonValue,
) -> Result<(), AppError> {
    let (target_node_id, current_state, stored_payload, command_json): (
        String,
        String,
        Option<String>,
        String,
    ) = sqlx::query_as(
            "select target_node_id, state, result_payload_json, command_json from commands where command_id = ?1",
        )
        .bind(command_id.as_str())
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("control.command_not_found", "Command not found"))?;
    if target_node_id != node_id.as_str() {
        return Err(AppError::auth(
            "control.command_owner_mismatch",
            "Command belongs to another Node",
        ));
    }
    if !is_terminal_command_state(status) {
        return Err(AppError::bad_request(
            "control.command_result_state_invalid",
            "Command result must use a terminal state",
        ));
    }
    let command: CommandEnvelope = serde_json::from_str(&command_json)?;
    validate_command_result_echo(&command, status, payload)?;
    let current_state = parse_command_state(&current_state);
    if is_terminal_command_state(current_state) {
        let same_payload = stored_payload
            .as_deref()
            .map(serde_json::from_str::<JsonValue>)
            .transpose()?
            .as_ref()
            == Some(payload);
        if current_state != status || !same_payload {
            return Err(AppError::bad_request(
                "control.command_result_conflict",
                "Command already has a different terminal result",
            ));
        }
        return Ok(());
    }
    if !matches!(
        current_state,
        CommandState::PendingDispatch | CommandState::Dispatched | CommandState::Acknowledged
    ) {
        return Err(AppError::bad_request(
            "control.command_transition_invalid",
            "Command cannot accept a result from its current state",
        ));
    }
    Ok(())
}

pub(crate) fn validate_command_result_echo(
    command: &CommandEnvelope,
    status: CommandState,
    payload: &JsonValue,
) -> Result<(), AppError> {
    if status != CommandState::Completed {
        return Ok(());
    }
    let expected_placement = command.target.project_placement_id();
    let actual_placement = match command.kind {
        CommandKind::ListWorkspaceTree => {
            Some(decode_control_result::<WorkspaceTreeResponse>(payload)?.placement_id)
        }
        CommandKind::ReadWorkspaceFile => {
            Some(decode_control_result::<WorkspaceFileContentResponse>(payload)?.placement_id)
        }
        CommandKind::WriteWorkspaceFile => {
            Some(decode_control_result::<WorkspaceFileWriteResponse>(payload)?.placement_id)
        }
        CommandKind::RunWorkspaceCommand => {
            Some(decode_control_result::<WorkspaceCommandRunResponse>(payload)?.placement_id)
        }
        CommandKind::ReadWorkspaceDiff => {
            Some(decode_control_result::<WorkspaceDiffResponse>(payload)?.placement_id)
        }
        CommandKind::OpenWorkspaceTerminal => Some(
            decode_control_result::<WorkspaceTerminalOpenResponse>(payload)?
                .terminal
                .placement_id,
        ),
        CommandKind::RequestDeduction => {
            let result = decode_control_result::<DeductionProviderOutput>(payload)?;
            let CommandPayload::RequestDeduction { package } = &command.payload else {
                return Err(AppError::bad_request(
                    "control.command_result_payload_invalid",
                    "Deduction command payload is invalid",
                ));
            };
            if result.deduction_id != package.deduction_id
                || result.evidence_snapshot_hash != package.evidence_snapshot_hash
                || result.schema_version != DEDUCTION_SCHEMA_VERSION
            {
                return Err(AppError::bad_request(
                    "control.command_result_target_mismatch",
                    "Deduction result does not match its evidence package",
                ));
            }
            None
        }
        CommandKind::RunTask => {
            let result = decode_control_result::<TaskRunResultPackage>(payload)?;
            if command.target.task_run_id() != Some(&result.task_run_id) {
                return Err(AppError::bad_request(
                    "control.command_result_target_mismatch",
                    "Task Run result does not match the command target",
                ));
            }
            None
        }
        _ => None,
    };
    if let Some(actual_placement) = actual_placement {
        if expected_placement != Some(&actual_placement) {
            return Err(AppError::bad_request(
                "control.command_result_target_mismatch",
                "Command result Placement does not match the command target",
            ));
        }
    }
    Ok(())
}

pub(crate) fn decode_control_result<T: DeserializeOwned>(
    payload: &JsonValue,
) -> Result<T, AppError> {
    serde_json::from_value(payload.0.clone()).map_err(|_| {
        AppError::bad_request(
            "control.command_result_payload_invalid",
            "Command result payload does not match its command kind",
        )
    })
}

pub(crate) async fn mark_deduction_running(
    state: &AppState,
    command_id: &CommandId,
) -> Result<(), AppError> {
    sqlx::query(
        "update deductions set state = 'running', updated_at = ?2 where command_id = ?1 and state = 'requested'",
    )
    .bind(command_id.as_str())
    .bind(Utc::now())
    .execute(&state.pool)
    .await?;
    Ok(())
}

pub(crate) async fn project_deduction_command_result(
    state: &AppState,
    command_id: &CommandId,
    status: CommandState,
    payload: &JsonValue,
) -> Result<(), AppError> {
    let row = sqlx::query(
        r#"
        select deduction_id, session_thread_id, scope_ref_json, state, input_package_json
        from deductions where command_id = ?1
        "#,
    )
    .bind(command_id.as_str())
    .fetch_optional(&state.pool)
    .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let current_state: String = row.try_get("state")?;
    if matches!(
        current_state.as_str(),
        "completed" | "invalid" | "failed" | "cancelled"
    ) {
        return Ok(());
    }
    let deduction_id = DeductionId::from(row.try_get::<String, _>("deduction_id")?);
    let session_id = SessionThreadId::from(row.try_get::<String, _>("session_thread_id")?);
    let scope_ref: UpravaRef = serde_json::from_str(&row.try_get::<String, _>("scope_ref_json")?)?;
    let package: DeductionInputPackage =
        serde_json::from_str(&row.try_get::<String, _>("input_package_json")?)?;
    if status != CommandState::Completed {
        let code = payload
            .0
            .get("error_code")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("deduction.provider_failed");
        let message = payload
            .0
            .get("error_message")
            .or_else(|| payload.0.get("message"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Deduction provider execution failed");
        update_deduction_failure(
            state,
            &deduction_id,
            "failed",
            code,
            message,
            payload
                .0
                .get("raw_text")
                .and_then(serde_json::Value::as_str),
            payload
                .0
                .get("raw_truncated")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        )
        .await?;
        append_core_session_event(
            state,
            &session_id,
            Some(command_id.clone()),
            EventKind::DeductionFailed,
            json!({
                "deduction_id": deduction_id.as_str(),
                "code": code,
                "message": message,
            }),
            vec![UpravaRef::Deduction {
                deduction_id: deduction_id.clone(),
            }],
            vec![scope_ref],
            vec![],
        )
        .await?;
        return Ok(());
    }

    let output: DeductionProviderOutput = serde_json::from_value(payload.0.clone())?;
    match validate_deduction_output(&output, &package) {
        Ok(result) => {
            let provenance = DeductionProvenance {
                provider: output.provider,
                model: output.model,
                session_thread_id: session_id.clone(),
                schema_version: output.schema_version,
                evidence_snapshot_hash: output.evidence_snapshot_hash,
                generated_at: Utc::now(),
            };
            let evidence_refs = result
                .steps
                .iter()
                .flat_map(|step| step.support_refs.iter().cloned())
                .collect::<Vec<_>>();
            let block = DeductionBlock {
                deduction_id: deduction_id.clone(),
                scope_ref: scope_ref.clone(),
                result,
                provenance,
            };
            let (raw_fallback, raw_truncated) =
                bounded_deduction_raw(&output.raw_text, output.raw_truncated);
            sqlx::query(
                r#"
                update deductions
                set state = 'completed', block_json = ?2, raw_fallback = ?3,
                    raw_truncated = ?4, error_code = null, error_message = null,
                    updated_at = ?5
                where deduction_id = ?1 and state in ('requested', 'running')
                "#,
            )
            .bind(deduction_id.as_str())
            .bind(serde_json::to_string(&block)?)
            .bind(raw_fallback)
            .bind(i64::from(raw_truncated))
            .bind(Utc::now())
            .execute(&state.pool)
            .await?;
            append_core_session_event(
                state,
                &session_id,
                Some(command_id.clone()),
                EventKind::DeductionCompleted,
                json!({ "deduction_id": deduction_id.as_str() }),
                vec![UpravaRef::Deduction {
                    deduction_id: deduction_id.clone(),
                }],
                vec![scope_ref],
                evidence_refs,
            )
            .await?;
        }
        Err((code, message)) => {
            let (raw_fallback, raw_truncated) =
                bounded_deduction_raw(&output.raw_text, output.raw_truncated);
            update_deduction_failure(
                state,
                &deduction_id,
                "invalid",
                code,
                &message,
                raw_fallback.as_deref(),
                raw_truncated,
            )
            .await?;
            append_core_session_event(
                state,
                &session_id,
                Some(command_id.clone()),
                EventKind::DeductionInvalid,
                json!({
                    "deduction_id": deduction_id.as_str(),
                    "code": code,
                    "message": message,
                }),
                vec![UpravaRef::Deduction {
                    deduction_id: deduction_id.clone(),
                }],
                vec![scope_ref],
                vec![],
            )
            .await?;
        }
    }
    Ok(())
}

pub(crate) fn validate_deduction_output(
    output: &DeductionProviderOutput,
    package: &DeductionInputPackage,
) -> Result<DeductionProviderResult, (&'static str, String)> {
    if output.deduction_id != package.deduction_id
        || output.evidence_snapshot_hash != package.evidence_snapshot_hash
        || output.schema_version != DEDUCTION_SCHEMA_VERSION
    {
        return Err((
            "deduction.provenance_mismatch",
            "Deduction provenance does not match the requested evidence snapshot".to_owned(),
        ));
    }
    if let Some(code) = output.error_code.as_deref() {
        return Err((
            "deduction.provider_reported_error",
            output
                .error_message
                .clone()
                .unwrap_or_else(|| format!("Provider reported {code}")),
        ));
    }
    let result = output.result.clone().ok_or_else(|| {
        (
            "deduction.result_missing",
            "Structured Deduction result is missing".to_owned(),
        )
    })?;
    if result.title.trim().is_empty() || result.conclusion.trim().is_empty() {
        return Err((
            "deduction.result_incomplete",
            "Deduction title and conclusion are required".to_owned(),
        ));
    }
    if result.steps.is_empty() || result.steps.len() > 100 {
        return Err((
            "deduction.steps_invalid",
            "Deduction must contain between 1 and 100 steps".to_owned(),
        ));
    }
    let allowed = package
        .allowed_refs
        .iter()
        .filter_map(|reference| serde_json::to_string(reference).ok())
        .collect::<HashSet<_>>();
    let mut step_ids = HashSet::new();
    for step in &result.steps {
        if step.step_id.trim().is_empty() || !step_ids.insert(step.step_id.as_str()) {
            return Err((
                "deduction.step_id_invalid",
                "Deduction step ids must be non-empty and unique".to_owned(),
            ));
        }
        if step.summary.trim().is_empty() || step.summary.chars().count() > 2_000 {
            return Err((
                "deduction.step_summary_invalid",
                "Deduction step summary is empty or too large".to_owned(),
            ));
        }
        if step.classification == DeductionClassification::Observed && step.support_refs.is_empty()
        {
            return Err((
                "deduction.observed_step_unsupported",
                "Every observed Deduction step must carry a support reference".to_owned(),
            ));
        }
        if step.support_refs.iter().any(|reference| {
            serde_json::to_string(reference)
                .ok()
                .is_none_or(|key| !allowed.contains(&key))
        }) {
            return Err((
                "deduction.support_ref_invalid",
                "Deduction returned a reference outside its evidence package".to_owned(),
            ));
        }
    }
    Ok(result)
}

pub(crate) async fn update_deduction_failure(
    state: &AppState,
    deduction_id: &DeductionId,
    state_value: &str,
    code: &str,
    message: &str,
    raw: Option<&str>,
    raw_truncated: bool,
) -> Result<(), AppError> {
    let raw = raw.map(|value| truncate_chars(value, MAX_DEDUCTION_RAW_CHARS));
    sqlx::query(
        r#"
        update deductions
        set state = ?2, raw_fallback = ?3, raw_truncated = ?4,
            error_code = ?5, error_message = ?6, updated_at = ?7
        where deduction_id = ?1 and state in ('requested', 'running')
        "#,
    )
    .bind(deduction_id.as_str())
    .bind(state_value)
    .bind(raw)
    .bind(i64::from(raw_truncated))
    .bind(code)
    .bind(message)
    .bind(Utc::now())
    .execute(&state.pool)
    .await?;
    Ok(())
}

pub(crate) fn bounded_deduction_raw(
    value: &str,
    provider_truncated: bool,
) -> (Option<String>, bool) {
    if value.is_empty() {
        return (None, provider_truncated);
    }
    let core_truncated = value.chars().count() > MAX_DEDUCTION_RAW_CHARS;
    (
        Some(truncate_chars(value, MAX_DEDUCTION_RAW_CHARS)),
        provider_truncated || core_truncated,
    )
}

pub(crate) fn is_terminal_command_state(state: CommandState) -> bool {
    matches!(
        state,
        CommandState::Completed
            | CommandState::Failed
            | CommandState::Blocked
            | CommandState::Expired
    )
}

pub(crate) async fn validate_event_owner(
    state: &AppState,
    node_id: &NodeId,
    event: &EventEnvelope,
) -> Result<(), AppError> {
    if event.node_id.as_ref() != Some(node_id) {
        return Err(AppError::auth(
            "control.event_node_mismatch",
            "Event node id does not match the authenticated Node",
        ));
    }
    if let ActorRef::Node {
        node_id: actor_node,
    } = &event.actor_ref
    {
        if actor_node != node_id {
            return Err(AppError::auth(
                "control.event_actor_mismatch",
                "Event actor belongs to another Node",
            ));
        }
    }
    if let Some(command_id) = &event.command_id {
        let command_context: Option<CommandEventContextRow> =
            sqlx::query_as(
                "select target_node_id, session_thread_id, runtime_session_id, project_placement_id from commands where command_id = ?1",
            )
            .bind(command_id.as_str())
            .fetch_optional(&state.pool)
            .await?;
        let Some((owner, command_session, command_runtime, command_placement)) = command_context
        else {
            return Err(AppError::auth(
                "control.event_command_mismatch",
                "Event command does not belong to the authenticated Node",
            ));
        };
        let placement_mismatch = command_session.is_none()
            && command_runtime.is_none()
            && command_placement.is_some()
            && !matches!(&event.scope_ref, ScopeRef::TaskRun { .. })
            && event_scope_placement_id(event).map(ProjectPlacementId::as_str)
                != command_placement.as_deref();
        let task_scope_mismatch = if let ScopeRef::TaskRun { task_run_id } = &event.scope_ref {
            sqlx::query_scalar::<_, i64>(
                "select count(*) from task_runs where task_run_id = ?1 and command_id = ?2",
            )
            .bind(task_run_id.as_str())
            .bind(command_id.as_str())
            .fetch_one(&state.pool)
            .await?
                == 0
        } else {
            false
        };
        if owner != node_id.as_str()
            || event
                .session_thread_id
                .as_ref()
                .map(SessionThreadId::as_str)
                != command_session.as_deref()
            || event
                .runtime_session_id
                .as_ref()
                .map(RuntimeSessionId::as_str)
                != command_runtime.as_deref()
            || placement_mismatch
            || task_scope_mismatch
        {
            return Err(AppError::auth(
                "control.event_command_mismatch",
                "Event command context does not match its durable target",
            ));
        }
    }
    if let (Some(runtime_id), Some(session_id)) =
        (&event.runtime_session_id, &event.session_thread_id)
    {
        let runtime_session: Option<String> = sqlx::query_scalar(
            "select session_thread_id from runtime_sessions where runtime_session_id = ?1",
        )
        .bind(runtime_id.as_str())
        .fetch_optional(&state.pool)
        .await?;
        if runtime_session.as_deref() != Some(session_id.as_str()) {
            return Err(AppError::auth(
                "control.event_context_mismatch",
                "Event runtime and session do not belong to the same aggregate",
            ));
        }
    }
    if let Some(runtime_id) = &event.runtime_session_id {
        validate_runtime_owner(state, node_id, runtime_id).await?;
    }
    if let Some(session_id) = &event.session_thread_id {
        let owner: Option<String> = sqlx::query_scalar(
            "select pp.node_id from session_threads st join project_placements pp on pp.project_placement_id = st.project_placement_id where st.session_thread_id = ?1",
        )
        .bind(session_id.as_str())
        .fetch_optional(&state.pool)
        .await?;
        if owner.as_deref() != Some(node_id.as_str()) {
            return Err(AppError::auth(
                "control.event_session_mismatch",
                "Event session does not belong to the authenticated Node",
            ));
        }
    }
    match &event.scope_ref {
        ScopeRef::Node {
            node_id: scope_node,
        } if scope_node != node_id => Err(AppError::auth(
            "control.event_scope_mismatch",
            "Event scope belongs to another Node",
        )),
        ScopeRef::Runtime { runtime_session_id }
            if event.runtime_session_id.as_ref() != Some(runtime_session_id) =>
        {
            Err(AppError::auth(
                "control.event_scope_mismatch",
                "Event runtime scope does not match its envelope runtime",
            ))
        }
        ScopeRef::Runtime { runtime_session_id } => {
            validate_runtime_owner(state, node_id, runtime_session_id).await
        }
        ScopeRef::Session { session_thread_id }
            if event.session_thread_id.as_ref() != Some(session_thread_id) =>
        {
            Err(AppError::auth(
                "control.event_scope_mismatch",
                "Event session scope does not match its envelope session",
            ))
        }
        ScopeRef::Session { session_thread_id } => {
            let owner: Option<String> = sqlx::query_scalar(
                "select pp.node_id from session_threads st join project_placements pp on pp.project_placement_id = st.project_placement_id where st.session_thread_id = ?1",
            )
            .bind(session_thread_id.as_str())
            .fetch_optional(&state.pool)
            .await?;
            (owner.as_deref() == Some(node_id.as_str()))
                .then_some(())
                .ok_or_else(|| {
                    AppError::auth(
                        "control.event_scope_mismatch",
                        "Event scope belongs to another Node",
                    )
                })
        }
        ScopeRef::Placement {
            project_placement_id,
        } => validate_placement_owner(state, node_id, project_placement_id).await,
        ScopeRef::TaskRun { task_run_id } => {
            let owner: Option<String> =
                sqlx::query_scalar("select node_id from task_runs where task_run_id = ?1")
                    .bind(task_run_id.as_str())
                    .fetch_optional(&state.pool)
                    .await?;
            (owner.as_deref() == Some(node_id.as_str()))
                .then_some(())
                .ok_or_else(|| {
                    AppError::auth(
                        "control.event_scope_mismatch",
                        "Task Run event scope belongs to another Node",
                    )
                })
        }
        _ => Ok(()),
    }
}

pub(crate) fn event_scope_placement_id(event: &EventEnvelope) -> Option<&ProjectPlacementId> {
    match &event.scope_ref {
        ScopeRef::Placement {
            project_placement_id,
        } => Some(project_placement_id),
        _ => None,
    }
}

pub(crate) async fn validate_runtime_owner(
    state: &AppState,
    node_id: &NodeId,
    runtime_id: &RuntimeSessionId,
) -> Result<(), AppError> {
    let owner: Option<String> = sqlx::query_scalar(
        "select pp.node_id from runtime_sessions rs join session_threads st on st.session_thread_id = rs.session_thread_id join project_placements pp on pp.project_placement_id = st.project_placement_id where rs.runtime_session_id = ?1",
    )
    .bind(runtime_id.as_str())
    .fetch_optional(&state.pool)
    .await?;
    (owner.as_deref() == Some(node_id.as_str()))
        .then_some(())
        .ok_or_else(|| {
            AppError::auth(
                "control.event_runtime_mismatch",
                "Event runtime belongs to another Node",
            )
        })
}

pub(crate) async fn validate_placement_owner(
    state: &AppState,
    node_id: &NodeId,
    placement_id: &ProjectPlacementId,
) -> Result<(), AppError> {
    let owner: Option<String> = sqlx::query_scalar(
        "select node_id from project_placements where project_placement_id = ?1",
    )
    .bind(placement_id.as_str())
    .fetch_optional(&state.pool)
    .await?;
    (owner.as_deref() == Some(node_id.as_str()))
        .then_some(())
        .ok_or_else(|| {
            AppError::auth(
                "control.event_placement_mismatch",
                "Event Placement belongs to another Node",
            )
        })
}

pub(crate) async fn validate_terminal_owner(
    state: &AppState,
    node_id: &NodeId,
    terminal_id: &TerminalId,
) -> Result<(), AppError> {
    let placement_id = state
        .workspace_terminals
        .read()
        .await
        .get(terminal_id.as_str())
        .map(|terminal| terminal.placement_id.clone())
        .ok_or_else(|| AppError::not_found("control.terminal_not_found", "Terminal not found"))?;
    validate_placement_owner(state, node_id, &placement_id)
        .await
        .map_err(|_| {
            AppError::auth(
                "control.terminal_owner_mismatch",
                "Terminal belongs to another Node",
            )
        })
}

pub(crate) async fn send_control_error(
    state: &AppState,
    context: &NodeContext,
    error_code: &str,
    message: &str,
    retryable: bool,
) {
    if context
        .sender
        .try_send(ControlFrame::ControlError {
            frame_id: Uuid::new_v4().to_string(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
            error: ApiError {
                error_code: error_code.to_owned(),
                message: message.to_owned(),
                details: JsonValue(json!({})),
                retryable,
                correlation_id: CorrelationId::from(Uuid::new_v4().to_string()),
            },
        })
        .is_err()
    {
        state
            .core_metrics
            .control_queue_rejections
            .fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) async fn send_control_frame(
    state: &AppState,
    node_id: &NodeId,
    frame: ControlFrame,
) -> bool {
    try_send_control_frame(state, node_id, frame).await.is_ok()
}

pub(crate) async fn try_send_control_frame(
    state: &AppState,
    node_id: &NodeId,
    frame: ControlFrame,
) -> Result<(), ControlSendError> {
    let Some(channel) = state.control_connections.sender(node_id).await else {
        return Err(ControlSendError::Unavailable);
    };
    channel.try_send(frame).map_err(|error| {
        state
            .core_metrics
            .control_queue_rejections
            .fetch_add(1, Ordering::Relaxed);
        match error {
            mpsc::error::TrySendError::Full(_) => ControlSendError::Saturated,
            mpsc::error::TrySendError::Closed(_) => ControlSendError::Closed,
        }
    })
}

pub(crate) async fn handle_workspace_terminal_stream(
    state: Arc<AppState>,
    node_id: NodeId,
    terminal_id: TerminalId,
    socket: WebSocket,
) {
    let (mut sender, mut receiver) = socket.split();
    let mut terminal_rx = state.terminal_hub.subscribe(&terminal_id).await;
    if !send_control_frame(
        &state,
        &node_id,
        ControlFrame::WorkspaceTerminalAttach {
            frame_id: Uuid::new_v4().to_string(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
            terminal_id: terminal_id.clone(),
        },
    )
    .await
    {
        let _ = send_ws_json(
            &mut sender,
            WorkspaceTerminalStreamFrame::Error {
                terminal_id,
                message: "Node control channel is unavailable".to_owned(),
                sent_at: Utc::now(),
            },
        )
        .await;
        return;
    }

    loop {
        tokio::select! {
            message = receiver.next() => {
                let Some(Ok(message)) = message else {
                    break;
                };
                match message {
                    WsMessage::Text(text) => {
                        if handle_terminal_client_frame(&state, &node_id, &terminal_id, &mut sender, &text).await.is_err() {
                            break;
                        }
                    }
                    WsMessage::Close(_) => break,
                    _ => {}
                }
            }
            notice = terminal_rx.recv() => {
                match notice {
                    Ok(notice) => {
                        let Some(frame) = terminal_notice_to_stream_frame(notice, &terminal_id) else {
                            continue;
                        };
                        if send_ws_json(&mut sender, frame).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        let _ = send_ws_json(
                            &mut sender,
                            WorkspaceTerminalStreamFrame::Error {
                                terminal_id: terminal_id.clone(),
                                message: format!(
                                    "Terminal stream skipped {skipped} frames; reconnect to replay retained output"
                                ),
                                sent_at: Utc::now(),
                            },
                        )
                        .await;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

pub(crate) async fn handle_terminal_client_frame(
    state: &AppState,
    node_id: &NodeId,
    terminal_id: &TerminalId,
    sender: &mut futures_util::stream::SplitSink<WebSocket, WsMessage>,
    text: &str,
) -> Result<(), ()> {
    let frame = match serde_json::from_str::<WorkspaceTerminalClientFrame>(text) {
        Ok(frame) => frame,
        Err(error) => {
            send_ws_json(
                sender,
                WorkspaceTerminalStreamFrame::Error {
                    terminal_id: terminal_id.clone(),
                    message: format!("Invalid terminal frame: {error}"),
                    sent_at: Utc::now(),
                },
            )
            .await?;
            return Ok(());
        }
    };
    match frame {
        WorkspaceTerminalClientFrame::Input { data } => {
            if data.chars().count() > MAX_TERMINAL_INPUT_CHARS {
                send_ws_json(
                    sender,
                    WorkspaceTerminalStreamFrame::Error {
                        terminal_id: terminal_id.clone(),
                        message: "Terminal input frame is too large".to_owned(),
                        sent_at: Utc::now(),
                    },
                )
                .await?;
                return Ok(());
            }
            forward_terminal_control_frame(
                state,
                node_id,
                ControlFrame::WorkspaceTerminalInput {
                    frame_id: Uuid::new_v4().to_string(),
                    protocol_version: API_VERSION.to_owned(),
                    sent_at: Utc::now(),
                    terminal_id: terminal_id.clone(),
                    data,
                },
            )
            .await
        }
        WorkspaceTerminalClientFrame::Resize { cols, rows } => {
            forward_terminal_control_frame(
                state,
                node_id,
                ControlFrame::WorkspaceTerminalResize {
                    frame_id: Uuid::new_v4().to_string(),
                    protocol_version: API_VERSION.to_owned(),
                    sent_at: Utc::now(),
                    terminal_id: terminal_id.clone(),
                    cols: cols.clamp(MIN_TERMINAL_COLS, MAX_TERMINAL_COLS),
                    rows: rows.clamp(MIN_TERMINAL_ROWS, MAX_TERMINAL_ROWS),
                },
            )
            .await
        }
        WorkspaceTerminalClientFrame::Close => {
            forward_terminal_control_frame(
                state,
                node_id,
                ControlFrame::WorkspaceTerminalClose {
                    frame_id: Uuid::new_v4().to_string(),
                    protocol_version: API_VERSION.to_owned(),
                    sent_at: Utc::now(),
                    terminal_id: terminal_id.clone(),
                },
            )
            .await
        }
        WorkspaceTerminalClientFrame::Ping => {
            send_ws_json(
                sender,
                WorkspaceTerminalStreamFrame::Pong {
                    sent_at: Utc::now(),
                },
            )
            .await
        }
    }
}

pub(crate) async fn forward_terminal_control_frame(
    state: &AppState,
    node_id: &NodeId,
    frame: ControlFrame,
) -> Result<(), ()> {
    if send_control_frame(state, node_id, frame).await {
        Ok(())
    } else {
        Err(())
    }
}

pub(crate) fn terminal_notice_to_stream_frame(
    notice: TerminalFrameNotice,
    expected_terminal_id: &TerminalId,
) -> Option<WorkspaceTerminalStreamFrame> {
    match notice {
        TerminalFrameNotice::Output {
            terminal_id,
            seq,
            data,
            sent_at,
        } if terminal_id == *expected_terminal_id => Some(WorkspaceTerminalStreamFrame::Output {
            terminal_id,
            seq,
            data,
            sent_at,
        }),
        TerminalFrameNotice::Status {
            terminal_id,
            state,
            exit_code,
            message,
            sent_at,
        } if terminal_id == *expected_terminal_id => Some(WorkspaceTerminalStreamFrame::Status {
            terminal_id,
            state,
            exit_code,
            message,
            sent_at,
        }),
        _ => None,
    }
}

pub(crate) async fn send_ws_json<T: Serialize>(
    sender: &mut futures_util::stream::SplitSink<WebSocket, WsMessage>,
    value: T,
) -> Result<(), ()> {
    let text = serde_json::to_string(&value).map_err(|_| ())?;
    sender
        .send(WsMessage::Text(text.into()))
        .await
        .map_err(|_| ())
}

pub(crate) fn control_frame_protocol_version(frame: &ControlFrame) -> &str {
    match frame {
        ControlFrame::Hello {
            protocol_version, ..
        }
        | ControlFrame::HelloAck {
            protocol_version, ..
        }
        | ControlFrame::CommandDispatch {
            protocol_version, ..
        }
        | ControlFrame::CommandAck {
            protocol_version, ..
        }
        | ControlFrame::CommandResult {
            protocol_version, ..
        }
        | ControlFrame::EventBatch {
            protocol_version, ..
        }
        | ControlFrame::EventBatchAck {
            protocol_version, ..
        }
        | ControlFrame::WorkspaceTerminalAttach {
            protocol_version, ..
        }
        | ControlFrame::WorkspaceTerminalInput {
            protocol_version, ..
        }
        | ControlFrame::WorkspaceTerminalResize {
            protocol_version, ..
        }
        | ControlFrame::WorkspaceTerminalClose {
            protocol_version, ..
        }
        | ControlFrame::WorkspaceTerminalOutput {
            protocol_version, ..
        }
        | ControlFrame::WorkspaceTerminalStatus {
            protocol_version, ..
        }
        | ControlFrame::Ping {
            protocol_version, ..
        }
        | ControlFrame::Pong {
            protocol_version, ..
        }
        | ControlFrame::ControlError {
            protocol_version, ..
        } => protocol_version,
    }
}
