//! Runtime lifecycle, command dispatch and atomic event-ingest orchestration.

use super::super::*;

pub(crate) async fn resolve_approval_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((session_thread_id, approval_id)): Path<(String, String)>,
    Json(request): Json<ResolveApprovalRequest>,
) -> Result<Json<CommandAcceptedResponse>, AppError> {
    resolve_approval_with_correlation(
        &state,
        SessionThreadId::from(session_thread_id),
        ApprovalId::from(approval_id),
        request,
        request_correlation_id(&headers),
    )
    .await
    .map(Json)
}

#[cfg(test)]
pub(crate) async fn resolve_approval(
    State(state): State<Arc<AppState>>,
    Path((session_thread_id, approval_id)): Path<(String, String)>,
    Json(request): Json<ResolveApprovalRequest>,
) -> Result<Json<CommandAcceptedResponse>, AppError> {
    resolve_approval_with_correlation(
        &state,
        SessionThreadId::from(session_thread_id),
        ApprovalId::from(approval_id),
        request,
        CorrelationId::new(),
    )
    .await
    .map(Json)
}

pub(crate) async fn resolve_approval_with_correlation(
    state: &AppState,
    session_id: SessionThreadId,
    approval_id: ApprovalId,
    request: ResolveApprovalRequest,
    correlation_id: CorrelationId,
) -> Result<CommandAcceptedResponse, AppError> {
    let detail = load_session_detail(state, &session_id).await?;
    ensure_pending_approval(&detail, &approval_id)?;
    ensure_session_commandable(state, &detail, CommandKind::ResolveApproval).await?;
    let command_id = CommandId::new();
    let provider_interaction_id = provider_interaction_for_approval(state, &approval_id).await?;

    record_and_dispatch_command(
        state,
        CommandEnvelope {
            command_id: command_id.clone(),
            kind: CommandKind::ResolveApproval,
            target: CommandTarget::SessionRuntime {
                node_id: detail.placement.node_id.clone(),
                project_placement_id: detail.placement.project_placement_id.clone(),
                session_thread_id: session_id.clone(),
                runtime_session_id: detail.session.runtime.runtime_session_id.clone(),
            },
            actor_ref: ActorRef::local_user(),
            source_refs: vec![UpravaRef::Approval {
                approval_id: approval_id.clone(),
            }],
            cause_refs: vec![UpravaRef::Session {
                session_thread_id: session_id.clone(),
            }],
            issued_at: Utc::now(),
            correlation_id,
            payload: CommandPayload::ResolveApproval {
                approval_id,
                provider_interaction_id,
                approved: request.approved,
                message: request.message,
            },
        },
    )
    .await?;

    let session = load_session_detail(state, &session_id).await?;
    Ok(CommandAcceptedResponse {
        command_id,
        session: Some(session),
    })
}

pub(crate) async fn submit_provider_input_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((session_thread_id, provider_interaction_id)): Path<(String, String)>,
    Json(request): Json<SubmitProviderInputRequest>,
) -> Result<Json<CommandAcceptedResponse>, AppError> {
    let session_id = SessionThreadId::from(session_thread_id);
    let interaction_id = ProviderInteractionId::from(provider_interaction_id);
    let detail = load_session_detail(&state, &session_id).await?;
    let interaction = detail
        .pending_interactions
        .iter()
        .find(|interaction| interaction.provider_interaction_id == interaction_id)
        .ok_or_else(|| {
            AppError::conflict(
                "provider_interaction.not_pending",
                "Provider interaction is not pending for this session",
            )
        })?;
    if interaction.kind != ProviderInteractionKind::UserInput {
        return Err(AppError::bad_request(
            "provider_interaction.kind_mismatch",
            "Provider interaction does not accept typed user input",
        ));
    }
    if interaction.state != ProviderInteractionState::Requested {
        return Err(AppError::conflict(
            "provider_interaction.already_resolving",
            "Provider interaction already has a decision awaiting provider confirmation",
        ));
    }
    if request.answers.is_empty()
        || request
            .answers
            .iter()
            .any(|answer| answer.chars().count() > 16_384)
    {
        return Err(AppError::bad_request(
            "provider_interaction.input_invalid",
            "At least one bounded answer is required",
        ));
    }
    let response = resolve_provider_interaction(
        &state,
        &detail,
        interaction_id,
        interaction.runtime_attempt_id.clone(),
        ProviderResolution::Input(request.answers),
        request_correlation_id(&headers),
    )
    .await?;
    Ok(Json(response))
}

pub(crate) async fn resolve_provider_approval_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((session_thread_id, provider_interaction_id)): Path<(String, String)>,
    Json(request): Json<ResolveApprovalRequest>,
) -> Result<Json<CommandAcceptedResponse>, AppError> {
    let session_id = SessionThreadId::from(session_thread_id);
    let interaction_id = ProviderInteractionId::from(provider_interaction_id);
    let detail = load_session_detail(&state, &session_id).await?;
    if request
        .message
        .as_ref()
        .is_some_and(|message| message.chars().count() > 16_384)
    {
        return Err(AppError::bad_request(
            "provider_interaction.message_too_large",
            "Approval message exceeds the character limit",
        ));
    }
    let interaction = detail
        .pending_interactions
        .iter()
        .find(|interaction| interaction.provider_interaction_id == interaction_id)
        .ok_or_else(|| {
            AppError::conflict(
                "provider_interaction.not_pending",
                "Provider interaction is not pending for this session",
            )
        })?;
    if interaction.kind != ProviderInteractionKind::Approval {
        return Err(AppError::bad_request(
            "provider_interaction.kind_mismatch",
            "Provider interaction is not an approval request",
        ));
    }
    if interaction.state != ProviderInteractionState::Requested {
        return Err(AppError::conflict(
            "provider_interaction.already_resolving",
            "Provider interaction already has a decision awaiting provider confirmation",
        ));
    }
    let response = resolve_provider_interaction(
        &state,
        &detail,
        interaction_id,
        interaction.runtime_attempt_id.clone(),
        ProviderResolution::Approval {
            approved: request.approved,
            message: request.message,
        },
        request_correlation_id(&headers),
    )
    .await?;
    Ok(Json(response))
}

enum ProviderResolution {
    Approval {
        approved: bool,
        message: Option<String>,
    },
    Input(Vec<String>),
}

async fn resolve_provider_interaction(
    state: &AppState,
    detail: &SessionDetail,
    interaction_id: ProviderInteractionId,
    runtime_attempt_id: RuntimeAttemptId,
    resolution: ProviderResolution,
    correlation_id: CorrelationId,
) -> Result<CommandAcceptedResponse, AppError> {
    let current_attempt_id = detail
        .session
        .runtime
        .current_attempt
        .as_ref()
        .map(|attempt| &attempt.runtime_attempt_id);
    if current_attempt_id != Some(&runtime_attempt_id) {
        return Err(AppError::conflict(
            "provider_interaction.attempt_superseded",
            "Provider interaction belongs to a superseded runtime attempt",
        ));
    }
    let command_kind = match &resolution {
        ProviderResolution::Approval { .. } => CommandKind::ResolveApproval,
        ProviderResolution::Input(_) => CommandKind::SubmitUserInput,
    };
    ensure_session_commandable(state, detail, command_kind).await?;
    let command_id = CommandId::new();
    let now = Utc::now();
    let (payload, intent) = match resolution {
        ProviderResolution::Approval { approved, message } => (
            CommandPayload::ResolveApproval {
                approval_id: ApprovalId::from(interaction_id.as_str()),
                provider_interaction_id: Some(interaction_id.clone()),
                approved,
                message,
            },
            json!({ "decision": if approved { "approved" } else { "denied" } }),
        ),
        ProviderResolution::Input(answers) => {
            let answer_count = answers.len();
            (
                CommandPayload::SubmitUserInput {
                    provider_interaction_id: interaction_id.clone(),
                    answers,
                },
                json!({ "answer_count": answer_count }),
            )
        }
    };
    let command = CommandEnvelope {
        command_id: command_id.clone(),
        kind: command_kind,
        target: CommandTarget::SessionRuntime {
            node_id: detail.placement.node_id.clone(),
            project_placement_id: detail.placement.project_placement_id.clone(),
            session_thread_id: detail.session.session_thread_id.clone(),
            runtime_session_id: detail.session.runtime.runtime_session_id.clone(),
        },
        actor_ref: ActorRef::local_user(),
        source_refs: vec![UpravaRef::ProviderInteraction {
            provider_interaction_id: interaction_id.clone(),
        }],
        cause_refs: vec![UpravaRef::RuntimeAttempt {
            runtime_attempt_id: runtime_attempt_id.clone(),
        }],
        issued_at: now,
        correlation_id,
        payload,
    };

    let mut transaction = state.pool.begin().await?;
    let expired = sqlx::query(
        "update provider_interactions set state = 'expired', resolved_at = ?1 where provider_interaction_id = ?2 and state = 'requested' and expires_at is not null and expires_at <= ?1",
    )
    .bind(now)
    .bind(interaction_id.as_str())
    .execute(&mut *transaction)
    .await?;
    if expired.rows_affected() > 0 {
        transaction.commit().await?;
        return Err(AppError::conflict(
            "provider_interaction.expired",
            "Provider interaction expired before the decision was accepted",
        ));
    }
    record_command_on_connection(&mut transaction, &command).await?;
    let transitioned = sqlx::query(
        r#"
        update provider_interactions
        set state = 'resolving', resolution_intent_json = ?1,
            resolve_command_id = ?2, resolving_at = ?3
        where provider_interaction_id = ?4 and session_thread_id = ?5
          and runtime_attempt_id = ?6 and state = 'requested'
          and (expires_at is null or expires_at > ?3)
        "#,
    )
    .bind(serde_json::to_string(&intent)?)
    .bind(command_id.as_str())
    .bind(now)
    .bind(interaction_id.as_str())
    .bind(detail.session.session_thread_id.as_str())
    .bind(runtime_attempt_id.as_str())
    .execute(&mut *transaction)
    .await?;
    if transitioned.rows_affected() == 0 {
        transaction.rollback().await?;
        return Err(AppError::conflict(
            "provider_interaction.not_resolvable",
            "Provider interaction is expired, terminal, or already resolving",
        ));
    }
    sqlx::query("update commands set state = 'pending_dispatch' where command_id = ?1")
        .bind(command_id.as_str())
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "insert into security_audit_events (audit_event_id, kind, node_id, origin, outcome, metadata_json, happened_at) values (?1, 'provider.interaction.resolution_requested', ?2, 'local_user', 'accepted', ?3, ?4)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(detail.placement.node_id.as_str())
    .bind(serde_json::to_string(&json!({
        "provider_interaction_id": interaction_id,
        "runtime_attempt_id": runtime_attempt_id,
        "command_id": command_id,
        "kind": format!("{command_kind:?}"),
    }))?)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    dispatch_pending_commands(state, &detail.placement.node_id).await?;

    Ok(CommandAcceptedResponse {
        command_id,
        session: Some(load_session_detail(state, &detail.session.session_thread_id).await?),
    })
}

async fn provider_interaction_for_approval(
    state: &AppState,
    approval_id: &ApprovalId,
) -> Result<Option<ProviderInteractionId>, AppError> {
    let id: Option<String> = sqlx::query_scalar(
        "select provider_interaction_id from provider_interactions where provider_request_id = ?1 and state = 'requested'",
    )
    .bind(approval_id.as_str())
    .fetch_optional(&state.pool)
    .await?;
    Ok(id.map(ProviderInteractionId::from))
}

pub(crate) async fn acknowledge_warning_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((session_thread_id, warning_kind)): Path<(String, String)>,
    Json(request): Json<AcknowledgeWarningRequest>,
) -> Result<Json<WarningAcknowledgementResponse>, AppError> {
    acknowledge_warning_with_correlation(
        &state,
        SessionThreadId::from(session_thread_id),
        warning_kind,
        request,
        request_correlation_id(&headers),
    )
    .await
    .map(Json)
}

#[cfg(test)]
pub(crate) async fn acknowledge_warning(
    State(state): State<Arc<AppState>>,
    Path((session_thread_id, warning_kind)): Path<(String, String)>,
    Json(request): Json<AcknowledgeWarningRequest>,
) -> Result<Json<WarningAcknowledgementResponse>, AppError> {
    acknowledge_warning_with_correlation(
        &state,
        SessionThreadId::from(session_thread_id),
        warning_kind,
        request,
        CorrelationId::new(),
    )
    .await
    .map(Json)
}

pub(crate) async fn acknowledge_warning_with_correlation(
    state: &AppState,
    session_id: SessionThreadId,
    warning_kind: String,
    request: AcknowledgeWarningRequest,
    correlation_id: CorrelationId,
) -> Result<WarningAcknowledgementResponse, AppError> {
    if warning_kind.trim().is_empty() {
        return Err(AppError::bad_request(
            "validation.warning_kind_required",
            "Warning kind is required",
        ));
    }
    let detail = load_session_detail(state, &session_id).await?;
    let acknowledged = acknowledged_warning_kinds(state, &session_id).await?;
    let active_warnings =
        active_warnings(&detail.placement, &detail.session.runtime, &acknowledged);
    if !active_warnings
        .iter()
        .any(|warning| warning.kind == warning_kind)
    {
        return Err(AppError::bad_request(
            "warning.not_active",
            "Warning is not currently active for this session",
        ));
    }

    let event = record_warning_acknowledgement(
        state,
        &detail,
        warning_kind,
        request.message,
        correlation_id,
    )
    .await?;
    let session = load_session_detail(state, &session_id).await?;
    Ok(WarningAcknowledgementResponse {
        event_id: event.event_id,
        session,
    })
}

pub(crate) async fn interrupt_runtime_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(runtime_session_id): Path<String>,
) -> Result<Json<CommandAcceptedResponse>, AppError> {
    lifecycle_command(
        &state,
        RuntimeSessionId::from(runtime_session_id),
        CommandKind::InterruptRuntime,
        request_correlation_id(&headers),
    )
    .await
    .map(Json)
}

#[cfg(test)]
pub(crate) async fn interrupt_runtime(
    State(state): State<Arc<AppState>>,
    Path(runtime_session_id): Path<String>,
) -> Result<Json<CommandAcceptedResponse>, AppError> {
    lifecycle_command(
        &state,
        RuntimeSessionId::from(runtime_session_id),
        CommandKind::InterruptRuntime,
        CorrelationId::new(),
    )
    .await
    .map(Json)
}

pub(crate) async fn stop_runtime_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(runtime_session_id): Path<String>,
) -> Result<Json<CommandAcceptedResponse>, AppError> {
    lifecycle_command(
        &state,
        RuntimeSessionId::from(runtime_session_id),
        CommandKind::StopRuntime,
        request_correlation_id(&headers),
    )
    .await
    .map(Json)
}

pub(crate) async fn resume_runtime_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(runtime_session_id): Path<String>,
) -> Result<Json<CommandAcceptedResponse>, AppError> {
    lifecycle_command(
        &state,
        RuntimeSessionId::from(runtime_session_id),
        CommandKind::ResumeRuntime,
        request_correlation_id(&headers),
    )
    .await
    .map(Json)
}

#[cfg(test)]
pub(crate) async fn resume_runtime(
    State(state): State<Arc<AppState>>,
    Path(runtime_session_id): Path<String>,
) -> Result<Json<CommandAcceptedResponse>, AppError> {
    lifecycle_command(
        &state,
        RuntimeSessionId::from(runtime_session_id),
        CommandKind::ResumeRuntime,
        CorrelationId::new(),
    )
    .await
    .map(Json)
}

pub(crate) async fn lifecycle_command(
    state: &AppState,
    runtime_session_id: RuntimeSessionId,
    kind: CommandKind,
    correlation_id: CorrelationId,
) -> Result<CommandAcceptedResponse, AppError> {
    let session_id = find_session_for_runtime(state, &runtime_session_id).await?;
    let detail = load_session_detail(state, &session_id).await?;
    ensure_session_commandable(state, &detail, kind).await?;
    let command_id = CommandId::new();
    let payload = lifecycle_command_payload(state, &detail, kind).await?;
    record_and_dispatch_command(
        state,
        CommandEnvelope {
            command_id: command_id.clone(),
            kind,
            target: CommandTarget::SessionRuntime {
                node_id: detail.placement.node_id.clone(),
                project_placement_id: detail.placement.project_placement_id.clone(),
                session_thread_id: session_id.clone(),
                runtime_session_id: runtime_session_id.clone(),
            },
            actor_ref: ActorRef::local_user(),
            source_refs: vec![],
            cause_refs: vec![],
            issued_at: Utc::now(),
            correlation_id,
            payload,
        },
    )
    .await?;
    if kind == CommandKind::StopRuntime {
        revoke_session_mcp_leases(state, &session_id, "session_stop_requested").await?;
    }

    Ok(CommandAcceptedResponse {
        command_id,
        session: Some(load_session_detail(state, &session_id).await?),
    })
}

pub(crate) async fn lifecycle_command_payload(
    state: &AppState,
    detail: &SessionDetail,
    kind: CommandKind,
) -> Result<CommandPayload, AppError> {
    if kind == CommandKind::InterruptRuntime {
        return Ok(CommandPayload::InterruptRuntime {
            runtime_attempt_id: detail
                .session
                .runtime
                .current_attempt
                .as_ref()
                .map(|attempt| attempt.runtime_attempt_id.clone()),
        });
    }
    if kind == CommandKind::StopRuntime {
        return Ok(CommandPayload::StopRuntime {
            runtime_attempt_id: detail
                .session
                .runtime
                .current_attempt
                .as_ref()
                .map(|attempt| attempt.runtime_attempt_id.clone()),
            reason: Some("explicit_stop".to_owned()),
        });
    }
    if kind != CommandKind::ResumeRuntime {
        return Err(AppError::bad_request(
            "runtime.command_kind_invalid",
            "Unsupported runtime lifecycle command",
        ));
    }

    let provider_resume_ref =
        runtime_provider_resume_ref_json(state, &detail.session.runtime.runtime_session_id).await?;
    Ok(CommandPayload::ResumeRuntime {
        provider: detail.session.runtime.provider.clone(),
        workspace_path: detail.placement.workspace_path.clone(),
        provider_resume_ref: provider_resume_ref.map(JsonValue),
        execution_profile: detail.session.runtime.execution_profile,
        effective_policy: detail.session.runtime.effective_policy.clone(),
        effective_policy_hash: detail.session.runtime.effective_policy_hash.clone(),
    })
}

pub(crate) async fn runtime_provider_resume_ref_json(
    state: &AppState,
    runtime_session_id: &RuntimeSessionId,
) -> Result<Option<serde_json::Value>, AppError> {
    let provider_resume_ref_json: Option<String> = sqlx::query_scalar(
        "select provider_resume_ref_json from runtime_sessions where runtime_session_id = ?1",
    )
    .bind(runtime_session_id.as_str())
    .fetch_optional(&state.pool)
    .await?;
    let Some(provider_resume_ref_json) = provider_resume_ref_json else {
        return Ok(None);
    };
    let provider_resume_ref_json = provider_resume_ref_json.trim();
    if provider_resume_ref_json.is_empty() {
        return Ok(None);
    }
    let value = serde_json::from_str::<serde_json::Value>(provider_resume_ref_json)?;
    if value.is_null() {
        return Ok(None);
    }
    Ok(Some(value))
}

pub(crate) async fn record_and_dispatch_command(
    state: &AppState,
    command: CommandEnvelope,
) -> Result<(), AppError> {
    let command_id = command.command_id.clone();
    let target_node_id = command.target.node_id().clone();
    record_command(state, command).await?;
    update_command_state(state, &command_id, CommandState::PendingDispatch).await?;
    dispatch_pending_commands(state, &target_node_id).await
}

pub(crate) async fn dispatch_pending_commands(
    state: &AppState,
    node_id: &NodeId,
) -> Result<(), AppError> {
    let rows = sqlx::query(
        r#"
        select o.command_id, o.command_json
        from command_dispatch_outbox o
        join commands c on c.command_id = o.command_id
        where o.target_node_id = ?1
          and c.state in ('recorded', 'pending_dispatch', 'dispatched', 'acknowledged')
        order by o.enqueued_at asc, o.command_id asc
        "#,
    )
    .bind(node_id.as_str())
    .fetch_all(&state.pool)
    .await?;
    let pending_count = rows.len();
    let Some(channel) = state.control_connections.sender(node_id).await else {
        if pending_count > 0 {
            tracing::debug!(
                pending_count,
                "pending commands waiting for node control channel"
            );
        }
        return Ok(());
    };

    for row in rows {
        let command_id = CommandId::from(row.try_get::<String, _>("command_id")?);
        let command_json: String = row.try_get("command_json")?;
        let command = serde_json::from_str::<CommandEnvelope>(&command_json)?;
        let command_kind = command.kind;
        let correlation_id = command.correlation_id.clone();
        if channel
            .try_send(ControlFrame::CommandDispatch {
                frame_id: Uuid::new_v4().to_string(),
                protocol_version: API_VERSION.to_owned(),
                sent_at: Utc::now(),
                command: Box::new(command),
            })
            .is_ok()
        {
            sqlx::query(
                "update command_dispatch_outbox set attempts = attempts + 1, last_attempt_at = ?1 where command_id = ?2",
            )
            .bind(Utc::now())
            .bind(command_id.as_str())
            .execute(&state.pool)
            .await?;
            tracing::info!(
                command_kind = ?command_kind,
                correlation_id = %correlation_id,
                "command dispatched"
            );
            update_command_state(state, &command_id, CommandState::Dispatched).await?;
        } else {
            tracing::warn!(
                command_kind = ?command_kind,
                correlation_id = %correlation_id,
                "command dispatch channel closed"
            );
        }
    }
    Ok(())
}

pub(crate) async fn should_open_control_channel(
    state: &AppState,
    node_id: &NodeId,
) -> Result<bool, AppError> {
    if state.control_connections.contains(node_id).await {
        return Ok(false);
    }
    let pending: i64 = sqlx::query_scalar(
        "select count(*) from commands where target_node_id = ?1 and state in ('recorded', 'pending_dispatch', 'dispatched', 'acknowledged')",
    )
    .bind(node_id.as_str())
    .fetch_one(&state.pool)
    .await?;
    if pending > 0 {
        return Ok(true);
    }
    let desired_tool_dependencies: i64 = sqlx::query_scalar(
        "select count(*) from mcp_dependency_instances where node_id = ?1 and desired_state = 'enabled'",
    )
    .bind(node_id.as_str())
    .fetch_one(&state.pool)
    .await?;
    Ok(desired_tool_dependencies > 0)
}

pub(crate) async fn update_command_state(
    state: &AppState,
    command_id: &CommandId,
    command_state: CommandState,
) -> Result<(), AppError> {
    let completed_at = matches!(
        command_state,
        CommandState::Completed
            | CommandState::Failed
            | CommandState::Blocked
            | CommandState::Expired
    )
    .then(Utc::now);
    let mut transaction = state.pool.begin().await?;
    sqlx::query("update commands set state = ?1, completed_at = coalesce(?2, completed_at) where command_id = ?3")
        .bind(format_command_state(command_state))
        .bind(completed_at)
        .bind(command_id.as_str())
        .execute(&mut *transaction)
        .await?;
    if completed_at.is_some() {
        sqlx::query("delete from command_dispatch_outbox where command_id = ?1")
            .bind(command_id.as_str())
            .execute(&mut *transaction)
            .await?;
    }
    transaction.commit().await?;
    tracing::debug!(
        command_state = ?command_state,
        "command state updated"
    );
    Ok(())
}

pub(crate) async fn update_command_result(
    state: &AppState,
    command_id: &CommandId,
    command_state: CommandState,
    result_payload: &JsonValue,
) -> Result<(), AppError> {
    let completed_at = matches!(
        command_state,
        CommandState::Completed
            | CommandState::Failed
            | CommandState::Blocked
            | CommandState::Expired
    )
    .then(Utc::now);
    let mut transaction = state.pool.begin().await?;
    sqlx::query(
        r#"
        update commands
        set state = ?1,
            completed_at = coalesce(?2, completed_at),
            result_payload_json = ?3
        where command_id = ?4
        "#,
    )
    .bind(format_command_state(command_state))
    .bind(completed_at)
    .bind(serde_json::to_string(result_payload)?)
    .bind(command_id.as_str())
    .execute(&mut *transaction)
    .await?;
    if matches!(
        command_state,
        CommandState::Completed
            | CommandState::Failed
            | CommandState::Blocked
            | CommandState::Expired
    ) {
        sqlx::query("delete from command_dispatch_outbox where command_id = ?1")
            .bind(command_id.as_str())
            .execute(&mut *transaction)
            .await?;
    }
    transaction.commit().await?;
    tracing::debug!(
        command_state = ?command_state,
        "command result stored"
    );
    Ok(())
}

pub(crate) async fn accept_node_event(
    state: &AppState,
    event: EventEnvelope,
) -> Result<(), AppError> {
    if !event.payload.matches_kind(event.kind) {
        return Err(AppError::bad_request(
            "protocol.event_payload_mismatch",
            "Event payload does not match its event kind",
        ));
    }
    // SQLite serializes the durable unit of work; this lock also keeps cursor
    // allocation deterministic before the connection-level projection API is
    // moved into a dedicated module.
    let _ingest_guard = state.event_ingest_lock.lock().await;
    let mut event = event;
    // Event projection reads dedupe/cursor state before writing several read
    // models. Reserve the SQLite writer slot before that snapshot so command
    // acknowledgements and HTTP mutations cannot invalidate it with
    // SQLITE_BUSY_SNAPSHOT and strand durable Node outbox events.
    let connection = state.pool.acquire().await?;
    let mut transaction =
        sqlx::Transaction::begin(connection, Some("BEGIN IMMEDIATE".into())).await?;
    let existing_event: Option<(String, String)> =
        sqlx::query_as("select projection_state, event_json from events where event_id = ?1")
            .bind(event.event_id.as_str())
            .fetch_optional(&mut *transaction)
            .await?;
    if let Some((projection_state, persisted_event_json)) = existing_event {
        let persisted_event: EventEnvelope = serde_json::from_str(&persisted_event_json)?;
        if projection_state == "projected" {
            tracing::debug!(
                event_kind = ?&persisted_event.kind,
                seq = persisted_event.seq,
                "duplicate projected node event ignored"
            );
            enqueue_event_publication_on_connection(&mut transaction, &persisted_event).await?;
            transaction.commit().await?;
            drain_event_publication_outbox(state).await?;
            return Ok(());
        }
        sqlx::query(
            "update events set projection_attempts = projection_attempts + 1 where event_id = ?1",
        )
        .bind(event.event_id.as_str())
        .execute(&mut *transaction)
        .await?;
        apply_event_projection_on_connection(&mut transaction, &persisted_event).await?;
        complete_event_projection_on_connection(&mut transaction, &persisted_event).await?;
        transaction.commit().await?;
        state
            .core_metrics
            .accepted_events
            .fetch_add(1, Ordering::Relaxed);
        record_event_metrics(state, &persisted_event);
        drain_event_publication_outbox(state).await?;
        return Ok(());
    }
    if sqlx::query_scalar::<_, i64>("select count(*) from events where event_id = ?1")
        .bind(event.event_id.as_str())
        .fetch_one(&mut *transaction)
        .await?
        > 0
    {
        tracing::debug!(
            event_kind = ?&event.kind,
            seq = event.seq,
            "duplicate node event ignored"
        );
        return Ok(());
    }
    let scope_key = scope_key(&event.scope_ref);
    let seq_conflict: Option<String> =
        sqlx::query_scalar("select event_id from events where scope_key = ?1 and seq = ?2")
            .bind(&scope_key)
            .bind(event.seq)
            .fetch_optional(&mut *transaction)
            .await?;
    if let Some(conflict) = seq_conflict {
        tracing::warn!(
            event_kind = ?&event.kind,
            seq = event.seq,
            "event sequence conflict rejected"
        );
        return Err(AppError::bad_request(
            "event.seq_conflict",
            format!("Event seq conflicts with {conflict}"),
        ));
    }
    let max_seq: Option<i64> =
        sqlx::query_scalar("select max(seq) from events where scope_key = ?1")
            .bind(&scope_key)
            .fetch_one(&mut *transaction)
            .await?;
    let expected_seq = max_seq.unwrap_or(0) + 1;
    // Task Run events use stable sparse lifecycle ranks so retries can safely
    // replay or diverge without reusing a sequence for another state.
    let stream_gap = (!matches!(&event.scope_ref, ScopeRef::TaskRun { .. })
        && event.seq > expected_seq)
        .then_some(expected_seq);
    if let Some(expected_seq) = stream_gap {
        tracing::warn!(
            event_kind = ?&event.kind,
            expected_seq,
            received_seq = event.seq,
            "event stream gap detected"
        );
    }
    if event.correlation_id.is_none() {
        event.correlation_id =
            command_correlation_id_on_connection(&mut transaction, event.command_id.as_ref())
                .await?;
    }
    if let Some(session_id) = event.session_thread_id.clone() {
        event.session_projection_seq =
            Some(next_session_projection_seq_on_connection(&mut transaction, &session_id).await?);
    } else {
        event.session_projection_seq = None;
    }
    upsert_actor_on_connection(&mut transaction, &event.actor_ref, event.happened_at).await?;
    insert_event_record_on_connection(&mut transaction, &scope_key, &event).await?;
    apply_event_projection_on_connection(&mut transaction, &event).await?;
    if let Some(expected_seq) = stream_gap {
        mark_event_stream_gap_on_connection(&mut transaction, &event, expected_seq).await?;
    }
    complete_event_projection_on_connection(&mut transaction, &event).await?;
    transaction.commit().await?;
    state
        .core_metrics
        .accepted_events
        .fetch_add(1, Ordering::Relaxed);
    record_event_metrics(state, &event);
    drain_event_publication_outbox(state).await?;
    log_event_appended(&event, stream_gap);
    Ok(())
}

pub(crate) fn record_event_metrics(state: &AppState, event: &EventEnvelope) {
    let is_truncation = event.kind == EventKind::ProviderActivity
        && event
            .payload
            .0
            .get("provider_event_type")
            .and_then(serde_json::Value::as_str)
            == Some("output_truncated");
    if is_truncation {
        state
            .core_metrics
            .provider_truncations
            .fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
pub(crate) async fn insert_event_record(
    state: &AppState,
    scope_key: &str,
    event: &EventEnvelope,
) -> Result<(), AppError> {
    let mut transaction = state.pool.begin().await?;
    insert_event_record_on_connection(&mut transaction, scope_key, event).await?;
    transaction.commit().await?;
    Ok(())
}

pub(crate) async fn insert_event_record_on_connection(
    connection: &mut SqliteConnection,
    scope_key: &str,
    event: &EventEnvelope,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        insert into events (
            event_id, scope_key, seq, kind, node_id, runtime_session_id,
            session_thread_id, session_projection_seq, command_id, actor_ref_json,
            scope_ref_json, correlation_id, source_refs_json, evidence_refs_json,
            cause_refs_json, result_refs_json, payload_json, event_json, happened_at
        )
        values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
        "#,
    )
    .bind(event.event_id.as_str())
    .bind(scope_key)
    .bind(event.seq)
    .bind(format!("{:?}", event.kind))
    .bind(event.node_id.as_ref().map(NodeId::as_str))
    .bind(
        event
            .runtime_session_id
            .as_ref()
            .map(RuntimeSessionId::as_str),
    )
    .bind(
        event
            .session_thread_id
            .as_ref()
            .map(SessionThreadId::as_str),
    )
    .bind(event.session_projection_seq)
    .bind(event.command_id.as_ref().map(CommandId::as_str))
    .bind(serde_json::to_string(&event.actor_ref)?)
    .bind(serde_json::to_string(&event.scope_ref)?)
    .bind(event.correlation_id.as_ref().map(CorrelationId::as_str))
    .bind(serde_json::to_string(&event.source_refs)?)
    .bind(serde_json::to_string(&event.evidence_refs)?)
    .bind(serde_json::to_string(&event.cause_refs)?)
    .bind(serde_json::to_string(&event.result_refs)?)
    .bind(serde_json::to_string(&event.payload)?)
    .bind(serde_json::to_string(event)?)
    .bind(event.happened_at)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

#[cfg(test)]
pub(crate) async fn complete_event_projection(
    state: &AppState,
    event: &EventEnvelope,
) -> Result<(), AppError> {
    let mut transaction = state.pool.begin().await?;
    complete_event_projection_on_connection(&mut transaction, event).await?;
    transaction.commit().await?;
    Ok(())
}

pub(crate) async fn complete_event_projection_on_connection(
    connection: &mut SqliteConnection,
    event: &EventEnvelope,
) -> Result<(), AppError> {
    sqlx::query(
        "update events set projection_state = 'projected', projected_at = ?2 where event_id = ?1 and projection_state <> 'projected'",
    )
    .bind(event.event_id.as_str())
    .bind(event.happened_at)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "insert into event_publication_outbox (event_id, event_json, enqueued_at) values (?1, ?2, ?3) on conflict(event_id) do nothing",
    )
    .bind(event.event_id.as_str())
    .bind(serde_json::to_string(event)?)
    .bind(event.happened_at)
    .execute(&mut *connection)
    .await?;
    let durable_message = match event.kind {
        EventKind::ProviderMessageCompleted => Some((
            MessageRole::Assistant,
            event
                .payload
                .0
                .get("content")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Provider completed a message"),
        )),
        EventKind::RuntimeError => Some((
            MessageRole::Runtime,
            event
                .payload
                .0
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Runtime error"),
        )),
        EventKind::ApprovalRequested => Some((
            MessageRole::Approval,
            event
                .payload
                .0
                .get("prompt")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Approval requested"),
        )),
        EventKind::ApprovalResolved => Some((
            MessageRole::Approval,
            event
                .payload
                .0
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Approval resolved"),
        )),
        _ => None,
    };
    if let (Some((role, content)), Some(session_thread_id)) =
        (durable_message, event.session_thread_id.as_ref())
    {
        let message_id = MessageId::new();
        sqlx::query(
            r#"
                insert into messages (
                    message_id, session_thread_id, turn_id, role, content,
                    created_at, completed_at, source_event_id
                )
                values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                on conflict do nothing
                "#,
        )
        .bind(message_id.as_str())
        .bind(session_thread_id.as_str())
        .bind(event.turn_id.as_ref().map(TurnId::as_str))
        .bind(format_message_role(role))
        .bind(content)
        .bind(event.happened_at)
        .bind(event.happened_at)
        .bind(event.event_id.as_str())
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

pub(crate) async fn next_session_projection_seq(
    state: &AppState,
    session_id: &SessionThreadId,
) -> Result<i64, AppError> {
    let max_seq: Option<i64> = sqlx::query_scalar(
        "select max(session_projection_seq) from events where session_thread_id = ?1",
    )
    .bind(session_id.as_str())
    .fetch_one(&state.pool)
    .await?;
    Ok(max_seq.unwrap_or(0) + 1)
}

pub(crate) async fn next_session_projection_seq_on_connection(
    connection: &mut SqliteConnection,
    session_id: &SessionThreadId,
) -> Result<i64, AppError> {
    let max_seq: Option<i64> = sqlx::query_scalar(
        "select max(session_projection_seq) from events where session_thread_id = ?1",
    )
    .bind(session_id.as_str())
    .fetch_one(&mut *connection)
    .await?;
    Ok(max_seq.unwrap_or(0) + 1)
}

pub(crate) async fn command_correlation_id_on_connection(
    connection: &mut SqliteConnection,
    command_id: Option<&CommandId>,
) -> Result<Option<CorrelationId>, AppError> {
    let Some(command_id) = command_id else {
        return Ok(None);
    };
    let command_json: Option<String> =
        sqlx::query_scalar("select command_json from commands where command_id = ?1")
            .bind(command_id.as_str())
            .fetch_optional(&mut *connection)
            .await?;
    command_json
        .map(|command_json| serde_json::from_str::<CommandEnvelope>(&command_json))
        .transpose()
        .map(|command| command.map(|command| command.correlation_id))
        .map_err(AppError::from)
}

pub(crate) async fn apply_event_projection_on_connection(
    connection: &mut SqliteConnection,
    event: &EventEnvelope,
) -> Result<(), AppError> {
    if let Some(runtime_session_id) = &event.runtime_session_id {
        touch_runtime_step_on_connection(connection, runtime_session_id, event.happened_at).await?;
    }
    update_turn_from_event_on_connection(connection, event).await?;
    update_approval_from_event_on_connection(connection, event).await?;
    project_managed_runtime_event_on_connection(connection, event).await?;
    project_task_event_on_connection(connection, event).await?;

    match event.kind {
        EventKind::RuntimeStarting => {
            if let Some(runtime_session_id) = &event.runtime_session_id {
                update_runtime_state_on_connection(
                    connection,
                    runtime_session_id,
                    RuntimeSessionState::Starting,
                    event.happened_at,
                )
                .await?;
            }
            update_session_state_from_event_on_connection(
                connection,
                event,
                SessionThreadState::Active,
            )
            .await?;
        }
        EventKind::RuntimeReady => {
            if let Some(runtime_session_id) = &event.runtime_session_id {
                update_runtime_state_on_connection(
                    connection,
                    runtime_session_id,
                    RuntimeSessionState::Ready,
                    event.happened_at,
                )
                .await?;
                update_runtime_provider_resume_ref_on_connection(
                    connection,
                    runtime_session_id,
                    event,
                )
                .await?;
            }
            update_session_state_from_event_on_connection(
                connection,
                event,
                SessionThreadState::Active,
            )
            .await?;
        }
        EventKind::RuntimeResuming => {
            if let Some(runtime_session_id) = &event.runtime_session_id {
                update_runtime_state_on_connection(
                    connection,
                    runtime_session_id,
                    RuntimeSessionState::Resuming,
                    event.happened_at,
                )
                .await?;
            }
            update_session_state_from_event_on_connection(
                connection,
                event,
                SessionThreadState::Active,
            )
            .await?;
        }
        EventKind::RuntimeRunning => {
            if let Some(runtime_session_id) = &event.runtime_session_id {
                update_runtime_state_on_connection(
                    connection,
                    runtime_session_id,
                    RuntimeSessionState::Running,
                    event.happened_at,
                )
                .await?;
            }
            update_session_state_from_event_on_connection(
                connection,
                event,
                SessionThreadState::Active,
            )
            .await?;
        }
        EventKind::RuntimeBlocked => {
            if let Some(runtime_session_id) = &event.runtime_session_id {
                update_runtime_state_on_connection(
                    connection,
                    runtime_session_id,
                    RuntimeSessionState::Blocked,
                    event.happened_at,
                )
                .await?;
            }
            update_session_state_from_event_on_connection(
                connection,
                event,
                SessionThreadState::Active,
            )
            .await?;
        }
        EventKind::RuntimeExpired => {
            if let Some(runtime_session_id) = &event.runtime_session_id {
                update_runtime_state_on_connection(
                    connection,
                    runtime_session_id,
                    RuntimeSessionState::Expired,
                    event.happened_at,
                )
                .await?;
            }
            update_session_state_from_event_on_connection(
                connection,
                event,
                SessionThreadState::Degraded,
            )
            .await?;
        }
        EventKind::RuntimeStopped => {
            if let Some(runtime_session_id) = &event.runtime_session_id {
                let stop_reason = match event.payload.kind() {
                    EventPayloadKind::RuntimeStopped { data } => {
                        data.reason.as_deref().unwrap_or("runtime_stopped")
                    }
                    _ => "runtime_stopped",
                };
                let idle_expired = stop_reason == "idle_expired";
                update_runtime_state_on_connection(
                    connection,
                    runtime_session_id,
                    if idle_expired {
                        RuntimeSessionState::Expired
                    } else {
                        RuntimeSessionState::Stopped
                    },
                    event.happened_at,
                )
                .await?;
                sqlx::query(
                    r#"
                    update runtime_attempts
                    set state = 'stopped', stop_reason = ?1, stopped_at = ?2, updated_at = ?2
                    where runtime_attempt_id = (
                        select current_attempt_id
                        from runtime_sessions
                        where runtime_session_id = ?3
                    )
                      and state not in ('stopped', 'failed', 'lost')
                    "#,
                )
                .bind(stop_reason)
                .bind(event.happened_at)
                .bind(runtime_session_id.as_str())
                .execute(&mut *connection)
                .await?;
                sqlx::query(if idle_expired {
                    "update provider_interactions set state = 'expired', resolved_at = ?1 where runtime_session_id = ?2 and state in ('requested', 'resolving')"
                } else {
                    "update provider_interactions set state = 'cancelled', resolved_at = ?1 where runtime_session_id = ?2 and state in ('requested', 'resolving')"
                })
                .bind(event.happened_at)
                .bind(runtime_session_id.as_str())
                .execute(&mut *connection)
                .await?;
            }
            update_session_state_from_event_on_connection(
                connection,
                event,
                if event
                    .payload
                    .0
                    .get("reason")
                    .and_then(serde_json::Value::as_str)
                    == Some("idle_expired")
                {
                    SessionThreadState::Degraded
                } else {
                    SessionThreadState::Stopped
                },
            )
            .await?;
        }
        EventKind::RuntimeError => {
            if let Some(runtime_session_id) = &event.runtime_session_id {
                update_runtime_state_on_connection(
                    connection,
                    runtime_session_id,
                    RuntimeSessionState::Error,
                    event.happened_at,
                )
                .await?;
                sqlx::query(
                    r#"
                    update runtime_attempts
                    set state = 'failed', stop_reason = 'runtime_error',
                        stopped_at = ?1, updated_at = ?1
                    where runtime_attempt_id = (
                        select current_attempt_id from runtime_sessions
                        where runtime_session_id = ?2
                    )
                      and state not in ('stopped', 'failed', 'lost')
                    "#,
                )
                .bind(event.happened_at)
                .bind(runtime_session_id.as_str())
                .execute(&mut *connection)
                .await?;
                sqlx::query(
                    "update provider_interactions set state = 'cancelled', resolved_at = ?1 where runtime_session_id = ?2 and state in ('requested', 'resolving')",
                )
                .bind(event.happened_at)
                .bind(runtime_session_id.as_str())
                .execute(&mut *connection)
                .await?;
            }
            update_session_state_from_event_on_connection(
                connection,
                event,
                SessionThreadState::Degraded,
            )
            .await?;
        }
        EventKind::TurnInterrupted => {
            if let Some(runtime_session_id) = &event.runtime_session_id {
                update_runtime_state_on_connection(
                    connection,
                    runtime_session_id,
                    RuntimeSessionState::Interrupted,
                    event.happened_at,
                )
                .await?;
            }
            update_session_state_from_event_on_connection(
                connection,
                event,
                SessionThreadState::Active,
            )
            .await?;
        }
        EventKind::ProviderMessageCompleted => {}
        EventKind::ApprovalRequested | EventKind::ProviderInteractionRequested => {
            if let Some(runtime_session_id) = &event.runtime_session_id {
                update_runtime_state_on_connection(
                    connection,
                    runtime_session_id,
                    RuntimeSessionState::Blocked,
                    event.happened_at,
                )
                .await?;
            }
            update_session_state_from_event_on_connection(
                connection,
                event,
                SessionThreadState::Active,
            )
            .await?;
        }
        EventKind::ApprovalResolved => {}
        EventKind::WorkspaceValidated | EventKind::ResourceSnapshotUpdated => {
            update_placement_from_workspace_event_on_connection(connection, event).await?;
        }
        _ => {}
    }
    Ok(())
}

pub(crate) async fn project_managed_runtime_event_on_connection(
    connection: &mut SqliteConnection,
    event: &EventEnvelope,
) -> Result<(), AppError> {
    match event.payload.kind() {
        EventPayloadKind::RuntimeAttemptStarted {
            runtime_attempt_id,
            state,
            reason,
            ..
        } => {
            let Some(runtime_session_id) = event.runtime_session_id.as_ref() else {
                return Err(AppError::bad_request(
                    "runtime_attempt.runtime_missing",
                    "Runtime attempt event is missing runtime session identity",
                ));
            };
            let current_attempt: Option<(String, String)> = sqlx::query_as(
                r#"
                select a.runtime_attempt_id, a.state
                from runtime_sessions r
                join runtime_attempts a on a.runtime_attempt_id = r.current_attempt_id
                where r.runtime_session_id = ?1
                "#,
            )
            .bind(runtime_session_id.as_str())
            .fetch_optional(&mut *connection)
            .await?;
            if current_attempt.as_ref().is_some_and(|(current_id, state)| {
                current_id != runtime_attempt_id.as_str()
                    && !matches!(state.as_str(), "stopped" | "failed" | "lost")
            }) {
                return Err(AppError::conflict(
                    "runtime_attempt.active_conflict",
                    "A new runtime attempt cannot supersede an active attempt",
                ));
            }
            let inserted = sqlx::query(
                r#"
                insert into runtime_attempts (
                    runtime_attempt_id, runtime_session_id, state, execution_profile,
                    effective_policy_json, effective_policy_hash, provider_version,
                    provider_resume_ref_json, start_reason, stop_reason, recovery_reason,
                    started_at, ready_at, stopped_at, updated_at
                )
                select ?1, runtime_session_id, ?2, execution_profile,
                       effective_policy_json, effective_policy_hash, null, null,
                       ?3, null, null, ?4, null, null, ?4
                from runtime_sessions
                where runtime_session_id = ?5
                  and effective_policy_json is not null
                  and effective_policy_hash is not null
                on conflict(runtime_attempt_id) do nothing
                "#,
            )
            .bind(runtime_attempt_id.as_str())
            .bind(format_runtime_attempt_state(*state))
            .bind(reason.as_deref().unwrap_or("provider_start"))
            .bind(event.happened_at)
            .bind(runtime_session_id.as_str())
            .execute(&mut *connection)
            .await?;
            if inserted.rows_affected() == 0 {
                let exists: i64 = sqlx::query_scalar(
                    "select count(*) from runtime_attempts where runtime_attempt_id = ?1",
                )
                .bind(runtime_attempt_id.as_str())
                .fetch_one(&mut *connection)
                .await?;
                if exists == 0 {
                    return Err(AppError::conflict(
                        "runtime_attempt.policy_missing",
                        "Runtime attempt cannot start without an effective policy snapshot",
                    ));
                }
            }
            sqlx::query(
                "update provider_interactions set state = 'superseded', resolved_at = ?1 where runtime_session_id = ?2 and runtime_attempt_id <> ?3 and state in ('requested', 'resolving')",
            )
            .bind(event.happened_at)
            .bind(runtime_session_id.as_str())
            .bind(runtime_attempt_id.as_str())
            .execute(&mut *connection)
            .await?;
            sqlx::query(
                "update runtime_sessions set current_attempt_id = ?1, recovery_status = 'live', updated_at = ?2 where runtime_session_id = ?3",
            )
            .bind(runtime_attempt_id.as_str())
            .bind(event.happened_at)
            .bind(runtime_session_id.as_str())
            .execute(&mut *connection)
            .await?;
        }
        EventPayloadKind::RuntimeAttemptReady {
            runtime_attempt_id,
            state,
            ..
        }
        | EventPayloadKind::RuntimeAttemptRecovered {
            runtime_attempt_id,
            state,
            ..
        } => {
            update_attempt_from_event(
                connection,
                runtime_attempt_id,
                *state,
                event,
                Some(event.happened_at),
                None,
            )
            .await?;
        }
        EventPayloadKind::RuntimeAttemptDisconnected {
            runtime_attempt_id,
            state,
            reason,
            ..
        }
        | EventPayloadKind::RuntimeAttemptReconnecting {
            runtime_attempt_id,
            state,
            reason,
            ..
        } => {
            update_attempt_from_event(
                connection,
                runtime_attempt_id,
                *state,
                event,
                None,
                reason.as_deref(),
            )
            .await?;
        }
        EventPayloadKind::RuntimeAttemptFailed {
            runtime_attempt_id,
            state,
            reason,
            ..
        } => {
            update_attempt_from_event(
                connection,
                runtime_attempt_id,
                *state,
                event,
                None,
                reason.as_deref(),
            )
            .await?;
            sqlx::query(
                "update runtime_attempts set stopped_at = ?2 where runtime_attempt_id = ?1",
            )
            .bind(runtime_attempt_id.as_str())
            .bind(event.happened_at)
            .execute(&mut *connection)
            .await?;
            sqlx::query(
                "update provider_interactions set state = 'cancelled', resolved_at = ?1 where runtime_attempt_id = ?2 and state in ('requested', 'resolving')",
            )
            .bind(event.happened_at)
            .bind(runtime_attempt_id.as_str())
            .execute(&mut *connection)
            .await?;
        }
        EventPayloadKind::ProviderInteractionRequested {
            provider_interaction_id,
            runtime_attempt_id,
            interaction_kind,
            prompt,
            expires_at,
        } => {
            let (Some(runtime_session_id), Some(session_thread_id)) = (
                event.runtime_session_id.as_ref(),
                event.session_thread_id.as_ref(),
            ) else {
                return Err(AppError::bad_request(
                    "provider_interaction.scope_missing",
                    "Provider interaction event is missing runtime or session identity",
                ));
            };
            let current_attempt_id: Option<String> = sqlx::query_scalar(
                "select current_attempt_id from runtime_sessions where runtime_session_id = ?1",
            )
            .bind(runtime_session_id.as_str())
            .fetch_optional(&mut *connection)
            .await?
            .flatten();
            if current_attempt_id.as_deref() != Some(runtime_attempt_id.as_str()) {
                return Err(AppError::conflict(
                    "provider_interaction.attempt_superseded",
                    "Provider interaction belongs to a non-current runtime attempt",
                ));
            }
            let inserted = sqlx::query(
                r#"
                insert into provider_interactions (
                    provider_interaction_id, runtime_attempt_id, runtime_session_id,
                    session_thread_id, turn_id, interaction_kind, state,
                    provider_request_id, request_payload_json, response_payload_json,
                    requested_event_id, resolved_event_id, resolve_command_id,
                    requested_at, resolved_at, expires_at
                ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?1, ?8, null,
                          ?9, null, null, ?10, null, ?11)
                on conflict(provider_interaction_id) do nothing
                "#,
            )
            .bind(provider_interaction_id.as_str())
            .bind(runtime_attempt_id.as_str())
            .bind(runtime_session_id.as_str())
            .bind(session_thread_id.as_str())
            .bind(event.turn_id.as_ref().map(TurnId::as_str))
            .bind(format_provider_interaction_kind(*interaction_kind))
            .bind("requested")
            .bind(serde_json::to_string(&json!({ "prompt": prompt }))?)
            .bind(event.event_id.as_str())
            .bind(event.happened_at)
            .bind(expires_at)
            .execute(&mut *connection)
            .await?;
            if inserted.rows_affected() == 0 {
                let existing: Option<(String, String, String)> = sqlx::query_as(
                    "select runtime_attempt_id, runtime_session_id, interaction_kind from provider_interactions where provider_interaction_id = ?1",
                )
                .bind(provider_interaction_id.as_str())
                .fetch_optional(&mut *connection)
                .await?;
                if existing
                    .as_ref()
                    .is_some_and(|(attempt_id, runtime_id, kind)| {
                        attempt_id != runtime_attempt_id.as_str()
                            || runtime_id != runtime_session_id.as_str()
                            || kind != format_provider_interaction_kind(*interaction_kind)
                    })
                {
                    return Err(AppError::conflict(
                        "provider_interaction.identity_conflict",
                        "Provider reused an interaction identity with a different scope or kind",
                    ));
                }
            }
        }
        EventPayloadKind::ProviderInteractionResolved {
            provider_interaction_id,
            runtime_attempt_id,
            interaction_kind,
            approved,
            answers,
        } => {
            let terminal_state = match (interaction_kind, approved) {
                (ProviderInteractionKind::Approval, Some(true)) => "approved",
                (ProviderInteractionKind::Approval, Some(false)) => "denied",
                (ProviderInteractionKind::Approval, None) => {
                    return Err(AppError::bad_request(
                        "provider_interaction.approval_decision_missing",
                        "Provider approval resolution is missing its decision",
                    ));
                }
                (ProviderInteractionKind::UserInput, _) => "answered",
            };
            let updated = sqlx::query(
                r#"
                update provider_interactions
                set state = ?1, response_payload_json = ?2,
                    resolved_event_id = ?3, resolved_at = ?4
                where provider_interaction_id = ?5 and runtime_attempt_id = ?6
                  and interaction_kind = ?7 and state = 'resolving'
                "#,
            )
            .bind(terminal_state)
            .bind(serde_json::to_string(&json!({
                "approved": approved,
                "answers": answers,
            }))?)
            .bind(event.event_id.as_str())
            .bind(event.happened_at)
            .bind(provider_interaction_id.as_str())
            .bind(runtime_attempt_id.as_str())
            .bind(format_provider_interaction_kind(*interaction_kind))
            .execute(&mut *connection)
            .await?;
            if updated.rows_affected() == 0 {
                return Err(AppError::conflict(
                    "provider_interaction.resolution_conflict",
                    "Provider resolution does not match a resolving interaction",
                ));
            }
            sqlx::query(
                "insert into security_audit_events (audit_event_id, kind, node_id, origin, outcome, metadata_json, happened_at) values (?1, 'provider.interaction.resolved', ?2, 'node', ?3, ?4, ?5)",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(event.node_id.as_ref().map(NodeId::as_str))
            .bind(terminal_state)
            .bind(serde_json::to_string(&json!({
                "provider_interaction_id": provider_interaction_id,
                "runtime_attempt_id": runtime_attempt_id,
                "kind": format_provider_interaction_kind(*interaction_kind),
            }))?)
            .bind(event.happened_at)
            .execute(&mut *connection)
            .await?;
            if let Some(runtime_session_id) = event.runtime_session_id.as_ref() {
                let pending_count: i64 = sqlx::query_scalar(
                    "select count(*) from provider_interactions where runtime_session_id = ?1 and state in ('requested', 'resolving')",
                )
                .bind(runtime_session_id.as_str())
                .fetch_one(&mut *connection)
                .await?;
                if pending_count == 0 {
                    update_runtime_state_on_connection(
                        connection,
                        runtime_session_id,
                        RuntimeSessionState::Running,
                        event.happened_at,
                    )
                    .await?;
                }
            }
        }
        EventPayloadKind::RuntimePolicyEffective {
            policy: Some(policy),
            policy_hash: Some(policy_hash),
        } => {
            let computed_hash = policy.policy_hash()?;
            if computed_hash != *policy_hash {
                return Err(AppError::bad_request(
                    "runtime.policy_hash_mismatch",
                    "Effective runtime policy does not match its hash",
                ));
            }
            let Some(runtime_session_id) = event.runtime_session_id.as_ref() else {
                return Ok(());
            };
            let stored_hash: Option<String> = sqlx::query_scalar(
                "select effective_policy_hash from runtime_sessions where runtime_session_id = ?1",
            )
            .bind(runtime_session_id.as_str())
            .fetch_optional(&mut *connection)
            .await?;
            if stored_hash
                .as_deref()
                .is_some_and(|hash| hash != policy_hash.as_str())
            {
                return Err(AppError::conflict(
                    "runtime.policy_echo_conflict",
                    "Provider effective policy differs from the Core-authorized snapshot",
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

async fn update_attempt_from_event(
    connection: &mut SqliteConnection,
    runtime_attempt_id: &RuntimeAttemptId,
    state: RuntimeAttemptState,
    event: &EventEnvelope,
    ready_at: Option<DateTime<Utc>>,
    recovery_reason: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        update runtime_attempts
        set state = ?1, ready_at = coalesce(?2, ready_at),
            recovery_reason = coalesce(?3, recovery_reason), updated_at = ?4
        where runtime_attempt_id = ?5
        "#,
    )
    .bind(format_runtime_attempt_state(state))
    .bind(ready_at)
    .bind(recovery_reason)
    .bind(event.happened_at)
    .bind(runtime_attempt_id.as_str())
    .execute(&mut *connection)
    .await?;
    if let Some(runtime_session_id) = event.runtime_session_id.as_ref() {
        let recovery_status = match state {
            RuntimeAttemptState::Disconnected => "degraded",
            RuntimeAttemptState::Reconnecting => "reconnecting",
            RuntimeAttemptState::Recovered => "recovered",
            RuntimeAttemptState::Failed | RuntimeAttemptState::Lost => "failed",
            _ => "live",
        };
        let current_attempt_updated = sqlx::query(
            "update runtime_sessions set recovery_status = ?1, updated_at = ?2 where runtime_session_id = ?3 and current_attempt_id = ?4",
        )
        .bind(recovery_status)
        .bind(event.happened_at)
        .bind(runtime_session_id.as_str())
        .bind(runtime_attempt_id.as_str())
        .execute(&mut *connection)
        .await?;
        let audit_kind = match state {
            RuntimeAttemptState::Disconnected | RuntimeAttemptState::Reconnecting => {
                Some("runtime.recovery.started")
            }
            RuntimeAttemptState::Recovered => Some("runtime.recovery.completed"),
            RuntimeAttemptState::Failed | RuntimeAttemptState::Lost => {
                Some("runtime.recovery.failed")
            }
            _ => None,
        };
        if current_attempt_updated.rows_affected() > 0 {
            if let Some(audit_kind) = audit_kind {
                sqlx::query(
                "insert into security_audit_events (audit_event_id, kind, node_id, origin, outcome, metadata_json, happened_at) values (?1, ?2, ?3, 'node', ?4, ?5, ?6)",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(audit_kind)
            .bind(event.node_id.as_ref().map(NodeId::as_str))
            .bind(recovery_status)
            .bind(serde_json::to_string(&json!({
                "runtime_attempt_id": runtime_attempt_id,
                "reason": recovery_reason,
            }))?)
            .bind(event.happened_at)
            .execute(&mut *connection)
                .await?;
            }
        }
    }
    Ok(())
}

pub(crate) async fn update_placement_from_workspace_event_on_connection(
    connection: &mut SqliteConnection,
    event: &EventEnvelope,
) -> Result<(), AppError> {
    let placement_id = match &event.scope_ref {
        ScopeRef::Placement {
            project_placement_id,
        } => project_placement_id.clone(),
        _ => event
            .payload
            .0
            .get("placement_id")
            .and_then(serde_json::Value::as_str)
            .map(ProjectPlacementId::from)
            .ok_or_else(|| {
                AppError::bad_request(
                    "placement.missing_ref",
                    "Workspace validation event is missing placement ref",
                )
            })?,
    };
    let state_value = event
        .payload
        .0
        .get("state")
        .and_then(serde_json::Value::as_str)
        .map(parse_placement_state)
        .unwrap_or(PlacementState::Validated);
    let resource_badges = event
        .payload
        .0
        .get("resource_badges")
        .cloned()
        .map(serde_json::from_value::<Vec<ResourceBadge>>)
        .transpose()?
        .unwrap_or_default();
    let git_snapshot = event
        .payload
        .0
        .get("git_snapshot")
        .cloned()
        .map(serde_json::from_value::<Option<GitWorkspaceSnapshot>>)
        .transpose()?
        .flatten();

    sqlx::query(
        r#"
        update project_placements
        set display_name = coalesce(?1, display_name),
            workspace_path = coalesce(?2, workspace_path),
            state = ?3,
            resource_badges_json = ?4,
            git_snapshot_json = ?5,
            last_validated_at = ?6,
            updated_at = ?6
        where project_placement_id = ?7
        "#,
    )
    .bind(
        event
            .payload
            .0
            .get("display_name")
            .and_then(serde_json::Value::as_str),
    )
    .bind(
        event
            .payload
            .0
            .get("workspace_path")
            .and_then(serde_json::Value::as_str),
    )
    .bind(format_placement_state(state_value))
    .bind(serde_json::to_string(&resource_badges)?)
    .bind(
        git_snapshot
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?,
    )
    .bind(event.happened_at)
    .bind(placement_id.as_str())
    .execute(&mut *connection)
    .await?;

    Ok(())
}

pub(crate) async fn record_warning_acknowledgement(
    state: &AppState,
    detail: &SessionDetail,
    warning_kind: String,
    message: Option<String>,
    correlation_id: CorrelationId,
) -> Result<EventEnvelope, AppError> {
    let scope_ref = ScopeRef::Session {
        session_thread_id: detail.session.session_thread_id.clone(),
    };
    let scope_key = scope_key(&scope_ref);
    let seq = next_seq(state, &scope_key).await?;
    let happened_at = Utc::now();
    let affected_refs = vec![
        UpravaRef::Warning {
            warning_kind: warning_kind.clone(),
            command_id: None,
        },
        UpravaRef::Session {
            session_thread_id: detail.session.session_thread_id.clone(),
        },
        UpravaRef::Placement {
            placement_id: detail.placement.project_placement_id.clone(),
        },
    ];
    let event = EventEnvelope {
        event_id: EventId::new(),
        command_id: None,
        correlation_id: Some(correlation_id),
        actor_ref: ActorRef::local_user(),
        scope_ref,
        node_id: Some(detail.placement.node_id.clone()),
        runtime_session_id: Some(detail.session.runtime.runtime_session_id.clone()),
        session_thread_id: Some(detail.session.session_thread_id.clone()),
        turn_id: None,
        seq,
        session_projection_seq: Some(
            next_session_projection_seq(state, &detail.session.session_thread_id).await?,
        ),
        kind: EventKind::CoordinationWarningAcknowledged,
        happened_at,
        source_refs: vec![UpravaRef::Warning {
            warning_kind: warning_kind.clone(),
            command_id: None,
        }],
        evidence_refs: vec![],
        cause_refs: vec![],
        result_refs: affected_refs.clone(),
        payload: EventPayload::from_json(
            EventKind::CoordinationWarningAcknowledged,
            json!({
                "warning_kind": warning_kind,
                "message": message,
                "affected_refs": affected_refs,
            }),
        ),
    };

    let mut transaction = state.pool.begin().await?;
    upsert_actor_on_connection(&mut transaction, &event.actor_ref, event.happened_at).await?;
    insert_event_record_on_connection(&mut transaction, &scope_key, &event).await?;

    sqlx::query(
        r#"
        insert into warning_acknowledgements (
            event_id, session_thread_id, actor_ref_json, warning_kind,
            command_id, affected_refs_json, acknowledged_at
        )
        values (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#,
    )
    .bind(event.event_id.as_str())
    .bind(detail.session.session_thread_id.as_str())
    .bind(serde_json::to_string(&event.actor_ref)?)
    .bind(
        event
            .payload
            .0
            .get("warning_kind")
            .and_then(serde_json::Value::as_str),
    )
    .bind(event.command_id.as_ref().map(CommandId::as_str))
    .bind(serde_json::to_string(&event.result_refs)?)
    .bind(event.happened_at)
    .execute(&mut *transaction)
    .await?;
    complete_event_projection_on_connection(&mut transaction, &event).await?;
    transaction.commit().await?;
    drain_event_publication_outbox(state).await?;
    log_event_appended(&event, None);
    Ok(event)
}
