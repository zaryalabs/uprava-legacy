//! Session, turn and deduction application use cases.

use super::super::*;

pub(crate) async fn create_session_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CreateSessionRequest>,
) -> Result<Json<SessionDetail>, AppError> {
    create_session_with_correlation(&state, request, request_correlation_id(&headers))
        .await
        .map(Json)
}

#[cfg(test)]
pub(crate) async fn create_session(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CreateSessionRequest>,
) -> Result<Json<SessionDetail>, AppError> {
    create_session_with_correlation(&state, request, CorrelationId::new())
        .await
        .map(Json)
}

pub(crate) async fn create_session_with_correlation(
    state: &AppState,
    request: CreateSessionRequest,
    correlation_id: CorrelationId,
) -> Result<SessionDetail, AppError> {
    let placement = load_placement(state, &request.project_placement_id).await?;
    let provider = request.provider.trim().to_owned();
    if provider.is_empty() {
        return Err(AppError::bad_request(
            "validation.provider_required",
            "Provider is required",
        ));
    }
    ensure_node_commandable(state, &placement.node_id).await?;
    ensure_placement_startable(&placement)?;
    let profile_was_explicit = request.execution_profile.is_some();
    let execution_profile = request
        .execution_profile
        .unwrap_or(AgentExecutionProfile::Managed);
    let provider_capabilities = ensure_node_supports_execution_profile(
        state,
        &placement.node_id,
        &provider,
        execution_profile,
    )
    .await?;
    ensure_provider_quota_admission(state, &provider, request.force, "session.start").await?;
    let effective_policy = resolve_effective_runtime_policy(
        &provider,
        execution_profile,
        &placement.workspace_path,
        provider_capabilities,
    );
    let effective_policy_hash = effective_policy.policy_hash()?;
    let effective_policy_json = serde_json::to_string(&effective_policy)?;
    let now = Utc::now();
    let session_thread_id = SessionThreadId::new();
    let runtime_session_id = RuntimeSessionId::new();
    let title = request
        .title
        .unwrap_or_else(|| format!("Session for {}", placement.display_name));

    let mut aggregate_transaction = state.pool.begin().await?;
    sqlx::query(
        r#"
        insert into session_threads (
            session_thread_id, project_placement_id, runtime_session_id, title,
            state, provider, created_at, updated_at
        )
        values (?1, ?2, ?3, ?4, 'active', ?5, ?6, ?6)
        "#,
    )
    .bind(session_thread_id.as_str())
    .bind(request.project_placement_id.as_str())
    .bind(runtime_session_id.as_str())
    .bind(title)
    .bind(&provider)
    .bind(now)
    .execute(&mut *aggregate_transaction)
    .await?;

    sqlx::query(
        r#"
        insert into runtime_sessions (
            runtime_session_id, session_thread_id, provider, state,
            resume_supported, provider_resume_ref_json, degraded_reason,
            last_runtime_step_at, execution_profile, effective_policy_json,
            effective_policy_hash, recovery_status, created_at, updated_at
        )
        values (?1, ?2, ?3, 'starting', 1, null, null, ?4, ?5, ?6, ?7,
                'not_required', ?4, ?4)
        "#,
    )
    .bind(runtime_session_id.as_str())
    .bind(session_thread_id.as_str())
    .bind(&provider)
    .bind(now)
    .bind(format_execution_profile(execution_profile))
    .bind(&effective_policy_json)
    .bind(effective_policy_hash.as_str())
    .execute(&mut *aggregate_transaction)
    .await?;
    let command = CommandEnvelope {
        command_id: CommandId::new(),
        kind: CommandKind::StartRuntime,
        target: CommandTarget::SessionRuntime {
            node_id: placement.node_id.clone(),
            project_placement_id: request.project_placement_id.clone(),
            session_thread_id: session_thread_id.clone(),
            runtime_session_id: runtime_session_id.clone(),
        },
        actor_ref: ActorRef::local_user(),
        source_refs: vec![],
        cause_refs: vec![],
        issued_at: now,
        correlation_id,
        payload: CommandPayload::StartRuntime {
            provider: provider.clone(),
            workspace_path: placement.workspace_path,
            execution_profile,
            effective_policy: Some(effective_policy),
            effective_policy_hash: Some(effective_policy_hash.clone()),
        },
    };
    record_command_on_connection(&mut aggregate_transaction, &command).await?;
    for (kind, outcome, metadata) in [
        (
            "runtime.profile.selected",
            "accepted",
            json!({
                "session_thread_id": session_thread_id,
                "runtime_session_id": runtime_session_id,
                "profile": format_execution_profile(execution_profile),
                "explicit": profile_was_explicit,
            }),
        ),
        (
            "runtime.policy.effective",
            "accepted",
            json!({
                "runtime_session_id": runtime_session_id,
                "policy_hash": effective_policy_hash,
                "profile": format_execution_profile(execution_profile),
            }),
        ),
    ] {
        sqlx::query(
            "insert into security_audit_events (audit_event_id, kind, node_id, origin, outcome, metadata_json, happened_at) values (?1, ?2, ?3, 'local_user', ?4, ?5, ?6)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(kind)
        .bind(placement.node_id.as_str())
        .bind(outcome)
        .bind(serde_json::to_string(&metadata)?)
        .bind(now)
        .execute(&mut *aggregate_transaction)
        .await?;
    }
    if profile_was_explicit && execution_profile == AgentExecutionProfile::ExecCompatibility {
        sqlx::query(
            "insert into security_audit_events (audit_event_id, kind, node_id, origin, outcome, metadata_json, happened_at) values (?1, 'runtime.policy.unsafe_override', ?2, 'local_user', 'accepted', ?3, ?4)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(placement.node_id.as_str())
        .bind(serde_json::to_string(&json!({
            "runtime_session_id": runtime_session_id,
            "reason": "explicit_exec_compatibility_selection",
        }))?)
        .bind(now)
        .execute(&mut *aggregate_transaction)
        .await?;
    }
    aggregate_transaction.commit().await?;
    dispatch_pending_commands(state, command.target.node_id()).await?;

    load_session_detail(state, &session_thread_id).await
}

pub(crate) async fn preview_session_policy_route(
    State(state): State<Arc<AppState>>,
    Json(request): Json<PreviewSessionPolicyRequest>,
) -> Result<Json<SessionPolicyPreview>, AppError> {
    let placement = load_placement(&state, &request.project_placement_id).await?;
    let provider = request.provider.trim();
    if provider.is_empty() {
        return Err(AppError::bad_request(
            "validation.provider_required",
            "Provider is required",
        ));
    }
    ensure_node_commandable(&state, &placement.node_id).await?;
    ensure_placement_startable(&placement)?;
    let capabilities = ensure_node_supports_execution_profile(
        &state,
        &placement.node_id,
        provider,
        request.execution_profile,
    )
    .await?;
    let effective_policy = resolve_effective_runtime_policy(
        provider,
        request.execution_profile,
        &placement.workspace_path,
        capabilities,
    );
    let effective_policy_hash = effective_policy.policy_hash()?;
    Ok(Json(SessionPolicyPreview {
        project_placement_id: request.project_placement_id,
        node_id: placement.node_id,
        effective_policy,
        effective_policy_hash,
    }))
}

pub(crate) fn resolve_effective_runtime_policy(
    provider: &str,
    execution_profile: AgentExecutionProfile,
    workspace_root: &str,
    provider_capabilities: Vec<ProviderRuntimeCapability>,
) -> EffectiveRuntimePolicy {
    let (sandbox_mode, approval_mode, network_posture) = match execution_profile {
        AgentExecutionProfile::Managed => (
            ProviderSandboxMode::WorkspaceWrite,
            ProviderApprovalMode::Untrusted,
            RuntimeNetworkPosture::Unsupported,
        ),
        AgentExecutionProfile::ExecCompatibility => (
            ProviderSandboxMode::DangerFullAccess,
            ProviderApprovalMode::Never,
            RuntimeNetworkPosture::ProviderDefault,
        ),
    };
    EffectiveRuntimePolicy {
        contract_version: 1,
        execution_profile,
        provider: provider.to_owned(),
        provider_version: None,
        provider_capabilities,
        sandbox_mode,
        approval_mode,
        workspace_root: workspace_root.to_owned(),
        additional_writable_paths: Vec::new(),
        network_posture,
        tool_exposure: RuntimeToolExposureSummary {
            server_count: 0,
            tool_count: 0,
            server_names: Vec::new(),
        },
        credential_profile_ref: None,
        unsafe_override: None,
        capability_metadata: BTreeMap::from([(
            "policy_source".to_owned(),
            "core.foundation.v1".to_owned(),
        )]),
    }
}

pub(crate) async fn session_detail(
    State(state): State<Arc<AppState>>,
    Path(session_thread_id): Path<String>,
) -> Result<Json<SessionDetail>, AppError> {
    load_session_detail(&state, &SessionThreadId::from(session_thread_id))
        .await
        .map(Json)
}

pub(crate) async fn attach_session(
    State(state): State<Arc<AppState>>,
    Path(session_thread_id): Path<String>,
) -> Result<Json<SessionDetail>, AppError> {
    let session_id = SessionThreadId::from(session_thread_id);
    update_session_attachment_state(&state, &session_id, SessionThreadState::Active).await?;
    load_session_detail(&state, &session_id).await.map(Json)
}

pub(crate) async fn detach_session(
    State(state): State<Arc<AppState>>,
    Path(session_thread_id): Path<String>,
) -> Result<Json<SessionDetail>, AppError> {
    let session_id = SessionThreadId::from(session_thread_id);
    update_session_attachment_state(&state, &session_id, SessionThreadState::Detached).await?;
    load_session_detail(&state, &session_id).await.map(Json)
}

pub(crate) async fn session_messages(
    State(state): State<Arc<AppState>>,
    Path(session_thread_id): Path<String>,
) -> Result<Json<Vec<Message>>, AppError> {
    load_messages(&state, &SessionThreadId::from(session_thread_id))
        .await
        .map(Json)
}

#[derive(Debug, Deserialize)]
pub(crate) struct EventsQuery {
    pub(crate) after_seq: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct EventLogQuery {
    pub(crate) session_thread_id: Option<String>,
    pub(crate) placement_id: Option<String>,
    pub(crate) kind: Option<String>,
    pub(crate) cursor: Option<String>,
    pub(crate) limit: Option<usize>,
}

pub(crate) async fn session_events(
    State(state): State<Arc<AppState>>,
    Path(session_thread_id): Path<String>,
    Query(query): Query<EventsQuery>,
) -> Result<Json<Vec<EventEnvelope>>, AppError> {
    load_events(
        &state,
        &SessionThreadId::from(session_thread_id),
        query.after_seq.unwrap_or(0),
    )
    .await
    .map(Json)
}

pub(crate) async fn event_log_route(
    State(state): State<Arc<AppState>>,
    Query(query): Query<EventLogQuery>,
) -> Result<Json<EventLogPage>, AppError> {
    load_event_log_page(&state, query).await.map(Json)
}

pub(crate) async fn event_detail_route(
    State(state): State<Arc<AppState>>,
    Path(event_id): Path<String>,
) -> Result<Json<EventEnvelope>, AppError> {
    load_event_by_id(&state, &EventId::from(event_id))
        .await
        .map(Json)
}

pub(crate) async fn resolve_reference_route(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ResolveReferenceRequest>,
) -> Result<Json<ReferenceResolution>, AppError> {
    resolve_reference(&state, request.reference).await.map(Json)
}

pub(crate) async fn session_stream(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_thread_id): Path<String>,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<axum::response::sse::Event, Infallible>>>, AppError> {
    let session_id = SessionThreadId::from(session_thread_id);
    let after_seq = stream_resume_after_seq(&query, &headers);
    let mut event_rx = state.event_tx.subscribe();
    drain_event_publication_outbox(&state).await?;
    let events = load_events(&state, &session_id, after_seq).await?;

    let stream = async_stream::stream! {
        let mut last_seq = after_seq;
        for event in events {
            last_seq = last_seq.max(session_event_cursor(&event));
            yield Ok(sse_event_for_event(&event));
        }
        loop {
            match event_rx.recv().await {
                Ok(event) if event_matches_session_after_seq(&event, &session_id, last_seq) => {
                    last_seq = session_event_cursor(&event);
                    yield Ok(sse_event_for_event(&event));
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    yield Ok(axum::response::sse::Event::default()
                        .event("uprava.reload")
                        .data(r#"{"reason":"stream_lagged"}"#));
                    break;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Ok(Sse::new(stream))
}

pub(crate) fn stream_resume_after_seq(query: &EventsQuery, headers: &HeaderMap) -> i64 {
    query
        .after_seq
        .or_else(|| last_event_id_after_seq(headers))
        .unwrap_or(0)
}

pub(crate) fn last_event_id_after_seq(headers: &HeaderMap) -> Option<i64> {
    header_value(headers, "last-event-id").and_then(|value| {
        value
            .parse::<i64>()
            .ok()
            .filter(|after_seq| *after_seq >= 0)
    })
}

pub(crate) fn sse_event_for_event(event: &EventEnvelope) -> axum::response::sse::Event {
    let data = serde_json::to_string(event).unwrap_or_else(|_| "{}".to_owned());
    axum::response::sse::Event::default()
        .id(session_event_cursor(event).to_string())
        .event("uprava.event")
        .data(data)
}

pub(crate) fn event_matches_session_after_seq(
    event: &EventEnvelope,
    session_id: &SessionThreadId,
    after_seq: i64,
) -> bool {
    event
        .session_thread_id
        .as_ref()
        .is_some_and(|event_session_id| event_session_id == session_id)
        && session_event_cursor(event) > after_seq
}

pub(crate) fn session_event_cursor(event: &EventEnvelope) -> i64 {
    event.session_projection_seq.unwrap_or(event.seq)
}

pub(crate) async fn session_evidence_projection(
    State(state): State<Arc<AppState>>,
    Path(session_thread_id): Path<String>,
) -> Result<Json<SessionEvidenceProjection>, AppError> {
    build_session_evidence_projection(&state, &SessionThreadId::from(session_thread_id))
        .await
        .map(Json)
}

pub(crate) async fn session_trace_projection(
    State(state): State<Arc<AppState>>,
    Path(session_thread_id): Path<String>,
) -> Result<Json<SessionTraceProjection>, AppError> {
    build_session_trace_projection(&state, &SessionThreadId::from(session_thread_id))
        .await
        .map(Json)
}

pub(crate) async fn session_agent_projection(
    State(state): State<Arc<AppState>>,
    Path(session_thread_id): Path<String>,
) -> Result<Json<AgentProjection>, AppError> {
    build_agent_projection(&state, &SessionThreadId::from(session_thread_id))
        .await
        .map(Json)
}

pub(crate) async fn send_turn_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_thread_id): Path<String>,
    Json(request): Json<SendTurnRequest>,
) -> Result<Json<CommandAcceptedResponse>, AppError> {
    send_turn_with_correlation(
        &state,
        SessionThreadId::from(session_thread_id),
        request,
        request_correlation_id(&headers),
    )
    .await
    .map(Json)
}

#[cfg(test)]
pub(crate) async fn send_turn(
    State(state): State<Arc<AppState>>,
    Path(session_thread_id): Path<String>,
    Json(request): Json<SendTurnRequest>,
) -> Result<Json<CommandAcceptedResponse>, AppError> {
    send_turn_with_correlation(
        &state,
        SessionThreadId::from(session_thread_id),
        request,
        CorrelationId::new(),
    )
    .await
    .map(Json)
}

pub(crate) async fn send_turn_with_correlation(
    state: &AppState,
    session_id: SessionThreadId,
    request: SendTurnRequest,
    correlation_id: CorrelationId,
) -> Result<CommandAcceptedResponse, AppError> {
    submit_turn_with_correlation(state, session_id, request.content, correlation_id).await
}

pub(crate) async fn create_deduction_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(session_thread_id): Path<String>,
    Json(request): Json<CreateDeductionRequest>,
) -> Result<(StatusCode, Json<DeductionAcceptedResponse>), AppError> {
    create_deduction(
        &state,
        SessionThreadId::from(session_thread_id),
        request,
        request_correlation_id(&headers),
    )
    .await
    .map(|response| (StatusCode::ACCEPTED, Json(response)))
}

pub(crate) async fn deduction_detail_route(
    State(state): State<Arc<AppState>>,
    Path(deduction_id): Path<String>,
) -> Result<Json<DeductionRecord>, AppError> {
    load_deduction_record(&state, &DeductionId::from(deduction_id))
        .await
        .map(Json)
}

pub(crate) async fn cancel_deduction_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(deduction_id): Path<String>,
) -> Result<Json<DeductionAcceptedResponse>, AppError> {
    cancel_deduction(
        &state,
        &DeductionId::from(deduction_id),
        request_correlation_id(&headers),
    )
    .await
    .map(Json)
}

pub(crate) async fn persist_deduction_route(
    State(state): State<Arc<AppState>>,
    Path(deduction_id): Path<String>,
) -> Result<Json<PersistDeductionResponse>, AppError> {
    persist_deduction(&state, &DeductionId::from(deduction_id))
        .await
        .map(Json)
}

pub(crate) async fn create_deduction(
    state: &AppState,
    session_id: SessionThreadId,
    request: CreateDeductionRequest,
    correlation_id: CorrelationId,
) -> Result<DeductionAcceptedResponse, AppError> {
    let detail = load_session_detail(state, &session_id).await?;
    ensure_session_commandable(state, &detail, CommandKind::RequestDeduction).await?;
    ensure_provider_quota_admission(
        state,
        &detail.session.runtime.provider,
        false,
        "deduction.request",
    )
    .await?;
    let active_count: i64 = sqlx::query_scalar(
        "select count(*) from deductions where session_thread_id = ?1 and state in ('requested', 'running')",
    )
    .bind(session_id.as_str())
    .fetch_one(&state.pool)
    .await?;
    if active_count > 0 {
        return Err(AppError::bad_request(
            "deduction.already_active",
            "This session already has an active Deduction",
        ));
    }
    let question = request
        .question
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Explain the observable steps that led to this result")
        .to_owned();
    if question.chars().count() > MAX_DEDUCTION_QUESTION_CHARS {
        return Err(AppError::bad_request(
            "deduction.question_too_large",
            "Deduction question exceeds the character limit",
        ));
    }
    let deduction_id = DeductionId::new();
    let package = build_deduction_input_package(
        state,
        &detail,
        deduction_id.clone(),
        request.scope_ref.clone(),
        question.clone(),
    )
    .await?;
    let command_id = CommandId::new();
    let now = Utc::now();
    let command = CommandEnvelope {
        command_id: command_id.clone(),
        kind: CommandKind::RequestDeduction,
        target: CommandTarget::SessionRuntime {
            node_id: detail.placement.node_id.clone(),
            project_placement_id: detail.placement.project_placement_id.clone(),
            session_thread_id: session_id.clone(),
            runtime_session_id: detail.session.runtime.runtime_session_id.clone(),
        },
        actor_ref: ActorRef::local_user(),
        source_refs: vec![request.scope_ref.clone()],
        cause_refs: vec![UpravaRef::Session {
            session_thread_id: session_id.clone(),
        }],
        issued_at: now,
        correlation_id,
        payload: CommandPayload::RequestDeduction {
            package: Box::new(package.clone()),
        },
    };
    let mut transaction = state.pool.begin().await?;
    record_command_on_connection(&mut transaction, &command).await?;
    sqlx::query(
        r#"
        insert into deductions (
            deduction_id, session_thread_id, scope_ref_json, question, state,
            command_id, evidence_snapshot_hash, input_package_json, created_at, updated_at
        ) values (?1, ?2, ?3, ?4, 'requested', ?5, ?6, ?7, ?8, ?8)
        "#,
    )
    .bind(deduction_id.as_str())
    .bind(session_id.as_str())
    .bind(serde_json::to_string(&request.scope_ref)?)
    .bind(&question)
    .bind(command_id.as_str())
    .bind(&package.evidence_snapshot_hash)
    .bind(serde_json::to_string(&package)?)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    append_core_session_event(
        state,
        &session_id,
        Some(command_id.clone()),
        EventKind::DeductionRequested,
        json!({
            "deduction_id": deduction_id.as_str(),
            "scope_ref": request.scope_ref,
            "question": question,
        }),
        vec![UpravaRef::Deduction {
            deduction_id: deduction_id.clone(),
        }],
        vec![package.scope_ref.clone()],
        vec![],
    )
    .await?;
    dispatch_pending_commands(state, &detail.placement.node_id).await?;
    Ok(DeductionAcceptedResponse {
        deduction_id,
        command_id,
    })
}

pub(crate) async fn cancel_deduction(
    state: &AppState,
    deduction_id: &DeductionId,
    correlation_id: CorrelationId,
) -> Result<DeductionAcceptedResponse, AppError> {
    let deduction = load_deduction_record(state, deduction_id).await?;
    if !matches!(
        deduction.state,
        DeductionState::Requested | DeductionState::Running
    ) {
        return Err(AppError::bad_request(
            "deduction.not_active",
            "Only a requested or running Deduction can be cancelled",
        ));
    }
    let detail = load_session_detail(state, &deduction.session_thread_id).await?;
    ensure_node_commandable(state, &detail.placement.node_id).await?;
    let command_id = CommandId::new();
    let now = Utc::now();
    let command = CommandEnvelope {
        command_id: command_id.clone(),
        kind: CommandKind::CancelDeduction,
        target: CommandTarget::SessionRuntime {
            node_id: detail.placement.node_id.clone(),
            project_placement_id: detail.placement.project_placement_id.clone(),
            session_thread_id: detail.session.session_thread_id.clone(),
            runtime_session_id: detail.session.runtime.runtime_session_id.clone(),
        },
        actor_ref: ActorRef::local_user(),
        source_refs: vec![UpravaRef::Deduction {
            deduction_id: deduction_id.clone(),
        }],
        cause_refs: vec![UpravaRef::Command {
            command_id: deduction.command_id.clone(),
        }],
        issued_at: now,
        correlation_id,
        payload: CommandPayload::CancelDeduction {
            deduction_id: deduction_id.clone(),
        },
    };
    let mut transaction = state.pool.begin().await?;
    record_command_on_connection(&mut transaction, &command).await?;
    sqlx::query(
        r#"
        update commands
        set state = 'failed',
            result_payload_json = ?2,
            completed_at = ?3
        where command_id = ?1 and state in ('recorded', 'pending_dispatch')
        "#,
    )
    .bind(deduction.command_id.as_str())
    .bind(serde_json::to_string(&JsonValue(json!({
        "error_code": "deduction.cancelled",
        "message": "Deduction was cancelled before provider completion",
    })))?)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "update deductions set state = 'cancelled', updated_at = ?2 where deduction_id = ?1 and state in ('requested', 'running')",
    )
    .bind(deduction_id.as_str())
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    append_core_session_event(
        state,
        &deduction.session_thread_id,
        Some(command_id.clone()),
        EventKind::DeductionCancelled,
        json!({ "deduction_id": deduction_id.as_str() }),
        vec![UpravaRef::Deduction {
            deduction_id: deduction_id.clone(),
        }],
        vec![deduction.scope_ref],
        vec![],
    )
    .await?;
    dispatch_pending_commands(state, &detail.placement.node_id).await?;
    Ok(DeductionAcceptedResponse {
        deduction_id: deduction_id.clone(),
        command_id,
    })
}

pub(crate) async fn build_deduction_input_package(
    state: &AppState,
    detail: &SessionDetail,
    deduction_id: DeductionId,
    scope_ref: UpravaRef,
    question: String,
) -> Result<DeductionInputPackage, AppError> {
    let trace = build_session_trace_projection(state, &detail.session.session_thread_id).await?;
    let mut allowed_refs = vec![UpravaRef::Session {
        session_thread_id: detail.session.session_thread_id.clone(),
    }];
    for step in &trace.steps {
        allowed_refs.push(step.primary_ref.clone());
        extend_causality_refs(&mut allowed_refs, &step.links);
    }
    let events = detail
        .events
        .iter()
        .rev()
        .take(MAX_DEDUCTION_EVENTS)
        .rev()
        .map(|event| {
            let primary_event_ref = event_ref(event);
            allowed_refs.push(primary_event_ref.clone());
            allowed_refs.extend(event.source_refs.iter().cloned());
            allowed_refs.extend(event.evidence_refs.iter().cloned());
            allowed_refs.extend(event.cause_refs.iter().cloned());
            allowed_refs.extend(event.result_refs.iter().cloned());
            DeductionEvidenceEvent {
                event_ref: primary_event_ref.clone(),
                kind: event.kind,
                summary: event_summary(event),
                happened_at: event.happened_at,
                links: CausalityLinks {
                    source_refs: event.source_refs.clone(),
                    evidence_refs: event.evidence_refs.clone(),
                    cause_refs: event.cause_refs.clone(),
                    result_refs: event.result_refs.clone(),
                    raw_refs: vec![primary_event_ref],
                },
                raw_excerpt: serde_json::to_string(&event.payload)
                    .ok()
                    .map(|value| truncate_chars(&value, 1_500)),
            }
        })
        .collect::<Vec<_>>();
    deduplicate_refs(&mut allowed_refs);
    if !deduction_scope_belongs_to_session(state, detail, &scope_ref, &allowed_refs).await? {
        return Err(AppError::bad_request(
            "deduction.scope_invalid",
            "Deduction scope is not visible in this session",
        ));
    }
    allowed_refs.push(scope_ref.clone());
    deduplicate_refs(&mut allowed_refs);
    let trace_was_truncated = trace.steps.len() > MAX_DEDUCTION_TRACE_STEPS;
    let mut package = DeductionInputPackage {
        deduction_id,
        session_thread_id: detail.session.session_thread_id.clone(),
        scope_ref,
        question,
        evidence_snapshot_hash: String::new(),
        trace_steps: trace
            .steps
            .into_iter()
            .take(MAX_DEDUCTION_TRACE_STEPS)
            .collect(),
        events,
        allowed_refs,
        truncated: detail.events.len() > MAX_DEDUCTION_EVENTS || trace_was_truncated,
        generated_at: Utc::now(),
    };
    let encoded = serde_json::to_vec(&package)?;
    package.evidence_snapshot_hash = format!("{:x}", Sha256::digest(encoded));
    Ok(package)
}

pub(crate) fn extend_causality_refs(refs: &mut Vec<UpravaRef>, links: &CausalityLinks) {
    refs.extend(links.source_refs.iter().cloned());
    refs.extend(links.evidence_refs.iter().cloned());
    refs.extend(links.cause_refs.iter().cloned());
    refs.extend(links.result_refs.iter().cloned());
    refs.extend(links.raw_refs.iter().cloned());
}

pub(crate) fn deduplicate_refs(refs: &mut Vec<UpravaRef>) {
    let mut keys = HashSet::new();
    refs.retain(|reference| {
        serde_json::to_string(reference)
            .ok()
            .is_some_and(|key| keys.insert(key))
    });
}

pub(crate) async fn deduction_scope_belongs_to_session(
    state: &AppState,
    detail: &SessionDetail,
    scope_ref: &UpravaRef,
    visible_refs: &[UpravaRef],
) -> Result<bool, AppError> {
    if visible_refs.iter().any(|reference| reference == scope_ref) {
        return Ok(true);
    }
    let session_id = &detail.session.session_thread_id;
    let runtime_id = &detail.session.runtime.runtime_session_id;
    let placement_id = &detail.placement.project_placement_id;
    match scope_ref {
        UpravaRef::Session { session_thread_id } => Ok(session_thread_id == session_id),
        UpravaRef::Runtime { runtime_session_id } => Ok(runtime_session_id == runtime_id),
        UpravaRef::RuntimeAttempt { runtime_attempt_id } => {
            let count: i64 = sqlx::query_scalar(
                "select count(*) from runtime_attempts where runtime_attempt_id = ?1 and runtime_session_id = ?2",
            )
            .bind(runtime_attempt_id.as_str())
            .bind(runtime_id.as_str())
            .fetch_one(&state.pool)
            .await?;
            Ok(count > 0)
        }
        UpravaRef::ProviderInteraction {
            provider_interaction_id,
        } => {
            let count: i64 = sqlx::query_scalar(
                "select count(*) from provider_interactions where provider_interaction_id = ?1 and session_thread_id = ?2",
            )
            .bind(provider_interaction_id.as_str())
            .bind(session_id.as_str())
            .fetch_one(&state.pool)
            .await?;
            Ok(count > 0)
        }
        UpravaRef::Placement {
            placement_id: candidate,
        }
        | UpravaRef::Workspace {
            placement_id: candidate,
        }
        | UpravaRef::File {
            placement_id: candidate,
            ..
        }
        | UpravaRef::FileRange {
            placement_id: candidate,
            ..
        }
        | UpravaRef::WorkspaceDiff {
            placement_id: candidate,
            ..
        }
        | UpravaRef::Terminal {
            placement_id: candidate,
            ..
        } => Ok(candidate == placement_id),
        UpravaRef::Message { message_id } | UpravaRef::MessageRange { message_id, .. } => {
            let count: i64 = sqlx::query_scalar(
                "select count(*) from messages where message_id = ?1 and session_thread_id = ?2",
            )
            .bind(message_id.as_str())
            .bind(session_id.as_str())
            .fetch_one(&state.pool)
            .await?;
            Ok(count > 0)
        }
        UpravaRef::Turn { turn_id } => {
            let count: i64 = sqlx::query_scalar(
                "select count(*) from events where turn_id = ?1 and session_thread_id = ?2",
            )
            .bind(turn_id.as_str())
            .bind(session_id.as_str())
            .fetch_one(&state.pool)
            .await?;
            Ok(count > 0)
        }
        UpravaRef::Event { event_id, .. } => {
            let event = load_event_by_id(state, event_id).await.ok();
            Ok(event.is_some_and(|event| {
                event.session_thread_id.as_ref() == Some(session_id)
                    || event.runtime_session_id.as_ref() == Some(runtime_id)
                    || matches!(
                        &event.scope_ref,
                        ScopeRef::Placement {
                            project_placement_id
                        } if project_placement_id == placement_id
                    )
            }))
        }
        UpravaRef::Command { command_id } => {
            let command_json: Option<String> =
                sqlx::query_scalar("select command_json from commands where command_id = ?1")
                    .bind(command_id.as_str())
                    .fetch_optional(&state.pool)
                    .await?;
            Ok(command_json
                .and_then(|value| serde_json::from_str::<CommandEnvelope>(&value).ok())
                .is_some_and(|command| command.target.session_thread_id() == Some(session_id)))
        }
        UpravaRef::Deduction { deduction_id } => {
            let count: i64 = sqlx::query_scalar(
                "select count(*) from deductions where deduction_id = ?1 and session_thread_id = ?2",
            )
            .bind(deduction_id.as_str())
            .bind(session_id.as_str())
            .fetch_one(&state.pool)
            .await?;
            Ok(count > 0)
        }
        UpravaRef::Artifact { artifact_id }
        | UpravaRef::ArtifactVersion {
            artifact_id,
            version: _,
        } => {
            let count: i64 = sqlx::query_scalar(
                "select count(*) from artifacts where artifact_id = ?1 and scope_ref_json = ?2",
            )
            .bind(artifact_id.as_str())
            .bind(serde_json::to_string(&ScopeRef::Session {
                session_thread_id: session_id.clone(),
            })?)
            .fetch_one(&state.pool)
            .await?;
            Ok(count > 0)
        }
        UpravaRef::Node { .. }
        | UpravaRef::TaskRun { .. }
        | UpravaRef::Project { .. }
        | UpravaRef::Block { .. }
        | UpravaRef::Approval { .. }
        | UpravaRef::Warning { .. }
        | UpravaRef::ToolCall { .. }
        | UpravaRef::TerminalCommand { .. }
        | UpravaRef::TerminalOutputRange { .. }
        | UpravaRef::DiffHunk { .. }
        | UpravaRef::CheckResult { .. }
        | UpravaRef::WorkspaceEdit { .. }
        | UpravaRef::TraceEvent { .. }
        | UpravaRef::ExternalEntity { .. }
        | UpravaRef::Unknown { .. } => Ok(false),
    }
}

pub(crate) async fn load_deduction_record(
    state: &AppState,
    deduction_id: &DeductionId,
) -> Result<DeductionRecord, AppError> {
    let row = sqlx::query(
        r#"
        select deduction_id, session_thread_id, scope_ref_json, question, state,
               command_id, block_json, raw_fallback, raw_truncated, error_code,
               error_message, artifact_id, created_at, updated_at
        from deductions where deduction_id = ?1
        "#,
    )
    .bind(deduction_id.as_str())
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("deduction.not_found", "Deduction not found"))?;
    Ok(DeductionRecord {
        deduction_id: DeductionId::from(row.try_get::<String, _>("deduction_id")?),
        session_thread_id: SessionThreadId::from(row.try_get::<String, _>("session_thread_id")?),
        scope_ref: serde_json::from_str(&row.try_get::<String, _>("scope_ref_json")?)?,
        question: row.try_get("question")?,
        state: parse_deduction_state(&row.try_get::<String, _>("state")?),
        command_id: CommandId::from(row.try_get::<String, _>("command_id")?),
        block: row
            .try_get::<Option<String>, _>("block_json")?
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
        raw_fallback: row.try_get("raw_fallback")?,
        raw_truncated: row.try_get::<i64, _>("raw_truncated")? != 0,
        error_code: row.try_get("error_code")?,
        error_message: row.try_get("error_message")?,
        artifact_id: row
            .try_get::<Option<String>, _>("artifact_id")?
            .map(ArtifactId::from),
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

pub(crate) async fn persist_deduction(
    state: &AppState,
    deduction_id: &DeductionId,
) -> Result<PersistDeductionResponse, AppError> {
    let deduction = load_deduction_record(state, deduction_id).await?;
    let block = deduction.block.ok_or_else(|| {
        AppError::bad_request(
            "deduction.not_completed",
            "Only a completed valid Deduction can be persisted",
        )
    })?;
    let (owner_plugin_id, schema_version) =
        resolve_active_artifact_type(state, "uprava.causality-narrative").await?;
    let now = Utc::now();
    let mut transaction = state.pool.begin().await?;
    let existing: Option<(String, i64)> = sqlx::query_as(
        "select artifact_id, current_version from causality_narratives where deduction_id = ?1",
    )
    .bind(deduction_id.as_str())
    .fetch_optional(&mut *transaction)
    .await?;
    let (artifact_id, version) = existing.map_or_else(
        || (ArtifactId::new(), 1_i64),
        |(artifact_id, version)| (ArtifactId::from(artifact_id), version + 1),
    );
    sqlx::query(
        r#"
        insert into causality_narratives (
            artifact_id, deduction_id, session_thread_id, current_version, created_at, updated_at
        ) values (?1, ?2, ?3, ?4, ?5, ?5)
        on conflict(deduction_id) do update set
            current_version = excluded.current_version,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(artifact_id.as_str())
    .bind(deduction_id.as_str())
    .bind(deduction.session_thread_id.as_str())
    .bind(version)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    let block_json = serde_json::to_string(&block)?;
    let provenance_json = serde_json::to_string(&block.provenance)?;
    let source_refs_json = serde_json::to_string(std::slice::from_ref(&block.scope_ref))?;
    let evidence_refs = block
        .result
        .steps
        .iter()
        .flat_map(|step| step.support_refs.iter().cloned())
        .collect::<Vec<_>>();
    let evidence_refs_json = serde_json::to_string(&evidence_refs)?;
    sqlx::query(
        "insert into causality_narrative_versions (artifact_id, version, block_json, provenance_json, created_at) values (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(artifact_id.as_str())
    .bind(version)
    .bind(&block_json)
    .bind(&provenance_json)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        insert into artifacts (
            artifact_id, artifact_type, title, scope_ref_json, owner_plugin_id,
            current_version, state, created_by_json, created_at, updated_at
        ) values (?1, 'uprava.causality-narrative', ?2, ?3,
                  ?4, ?5, 'active', '{"kind":"system"}', ?6, ?6)
        on conflict(artifact_id) do update set
            title = excluded.title,
            current_version = excluded.current_version,
            state = 'active',
            updated_at = excluded.updated_at
        "#,
    )
    .bind(artifact_id.as_str())
    .bind(&block.result.title)
    .bind(serde_json::to_string(&ScopeRef::Session {
        session_thread_id: deduction.session_thread_id.clone(),
    })?)
    .bind(owner_plugin_id.as_str())
    .bind(version)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        insert into artifact_versions (
            artifact_id, version, schema_version, payload_json, fallback_text,
            source_version, source_refs_json, evidence_refs_json, cause_refs_json,
            trace_refs_json, provenance_json, created_at
        ) values (?1, ?2, ?3, ?4, ?5, null, ?6, ?7, '[]', ?7, ?8, ?9)
        "#,
    )
    .bind(artifact_id.as_str())
    .bind(version)
    .bind(i64::from(schema_version))
    .bind(&block_json)
    .bind(&block.result.conclusion)
    .bind(source_refs_json)
    .bind(evidence_refs_json)
    .bind(provenance_json)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("update deductions set artifact_id = ?2, updated_at = ?3 where deduction_id = ?1")
        .bind(deduction_id.as_str())
        .bind(artifact_id.as_str())
        .bind(now)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(PersistDeductionResponse {
        deduction_id: deduction_id.clone(),
        artifact_id,
        version: u64::try_from(version).unwrap_or(u64::MAX),
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "durable session events keep command identity, payload, and causal aspects explicit"
)]
pub(crate) async fn append_core_session_event(
    state: &AppState,
    session_id: &SessionThreadId,
    command_id: Option<CommandId>,
    kind: EventKind,
    payload: serde_json::Value,
    result_refs: Vec<UpravaRef>,
    source_refs: Vec<UpravaRef>,
    evidence_refs: Vec<UpravaRef>,
) -> Result<EventEnvelope, AppError> {
    let scope_ref = ScopeRef::Session {
        session_thread_id: session_id.clone(),
    };
    let seq = next_seq(state, &scope_key(&scope_ref)).await?;
    let event = EventEnvelope {
        event_id: EventId::new(),
        command_id: command_id.clone(),
        correlation_id: None,
        actor_ref: ActorRef::System,
        scope_ref,
        node_id: None,
        runtime_session_id: None,
        session_thread_id: Some(session_id.clone()),
        turn_id: None,
        seq,
        session_projection_seq: None,
        kind,
        happened_at: Utc::now(),
        source_refs,
        evidence_refs,
        cause_refs: command_id
            .map(|command_id| vec![UpravaRef::Command { command_id }])
            .unwrap_or_default(),
        result_refs,
        payload: EventPayload::from_json(kind, payload),
    };
    accept_node_event(state, event.clone()).await?;
    Ok(event)
}

pub(crate) async fn submit_turn_with_correlation(
    state: &AppState,
    session_id: SessionThreadId,
    content: String,
    correlation_id: CorrelationId,
) -> Result<CommandAcceptedResponse, AppError> {
    submit_turn_for_actor(
        state,
        session_id,
        content,
        correlation_id,
        ActorRef::local_user(),
    )
    .await
}

pub(crate) async fn submit_turn_for_actor(
    state: &AppState,
    session_id: SessionThreadId,
    content: String,
    correlation_id: CorrelationId,
    actor_ref: ActorRef,
) -> Result<CommandAcceptedResponse, AppError> {
    if content.trim().is_empty() {
        return Err(AppError::bad_request(
            "validation.empty_turn",
            "Turn content cannot be empty",
        ));
    }

    let detail = load_session_detail(state, &session_id).await?;
    ensure_session_commandable(state, &detail, CommandKind::SendTurn).await?;
    let now = Utc::now();
    let command_id = CommandId::new();
    let turn_id = TurnId::new();
    let user_message_id = MessageId::new();
    let command = CommandEnvelope {
        command_id: command_id.clone(),
        kind: CommandKind::SendTurn,
        target: CommandTarget::SessionRuntime {
            node_id: detail.placement.node_id.clone(),
            project_placement_id: detail.placement.project_placement_id.clone(),
            session_thread_id: session_id.clone(),
            runtime_session_id: detail.session.runtime.runtime_session_id.clone(),
        },
        actor_ref,
        source_refs: vec![],
        cause_refs: vec![UpravaRef::Session {
            session_thread_id: session_id.clone(),
        }],
        issued_at: now,
        correlation_id,
        payload: CommandPayload::SendTurn {
            content: content.clone(),
            turn_id: turn_id.clone(),
        },
    };
    record_turn_submission(
        state,
        &command,
        &turn_id,
        &user_message_id,
        &session_id,
        &content,
        now,
    )
    .await?;
    dispatch_pending_commands(state, &detail.placement.node_id).await?;

    let session = load_session_detail(state, &session_id).await?;
    Ok(CommandAcceptedResponse {
        command_id,
        session: Some(session),
    })
}
