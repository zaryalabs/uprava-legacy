//! Durable command, event, turn, approval and publication-outbox operations.

use super::super::*;

pub(crate) async fn record_command(
    state: &AppState,
    command: CommandEnvelope,
) -> Result<(), AppError> {
    let mut transaction = state.pool.begin().await?;
    record_command_on_connection(&mut transaction, &command).await?;
    transaction.commit().await?;
    Ok(())
}

pub(crate) async fn record_command_on_connection(
    connection: &mut sqlx::SqliteConnection,
    command: &CommandEnvelope,
) -> Result<(), AppError> {
    if !command.payload.matches_kind(command.kind) {
        return Err(AppError::bad_request(
            "protocol.command_payload_mismatch",
            "Command payload does not match its command kind",
        ));
    }
    let (actor_key, actor_kind, display_name) = actor_identity(&command.actor_ref);
    sqlx::query(
        r#"
        insert into actors (
            actor_key, actor_kind, display_name, actor_ref_json, first_seen_at, last_seen_at
        )
        values (?1, ?2, ?3, ?4, ?5, ?5)
        on conflict(actor_key) do update set
            actor_ref_json = excluded.actor_ref_json,
            display_name = excluded.display_name,
            last_seen_at = excluded.last_seen_at
        "#,
    )
    .bind(actor_key)
    .bind(actor_kind)
    .bind(display_name)
    .bind(serde_json::to_string(&command.actor_ref)?)
    .bind(command.issued_at)
    .execute(&mut *connection)
    .await?;
    tracing::info!(
        command_kind = ?command.kind,
        correlation_id = %command.correlation_id,
        "command recorded"
    );
    sqlx::query(
        r#"
        insert into commands (
            command_id, kind, state, target_node_id, session_thread_id,
            runtime_session_id, project_placement_id, actor_ref_json, correlation_id,
            source_refs_json, cause_refs_json, payload_json, dedupe_key, command_json,
            created_at, completed_at
        )
        values (?1, ?2, 'recorded', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, null)
        "#,
    )
    .bind(command.command_id.as_str())
    .bind(format!("{:?}", command.kind))
    .bind(command.target.node_id().as_str())
    .bind(
        command
            .target
            .session_thread_id()
            .map(SessionThreadId::as_str),
    )
    .bind(
        command
            .target
            .runtime_session_id()
            .map(RuntimeSessionId::as_str),
    )
    .bind(
        command
            .target
            .project_placement_id()
            .map(ProjectPlacementId::as_str),
    )
    .bind(serde_json::to_string(&command.actor_ref)?)
    .bind(command.correlation_id.as_str())
    .bind(serde_json::to_string(&command.source_refs)?)
    .bind(serde_json::to_string(&command.cause_refs)?)
    .bind(serde_json::to_string(&command.payload)?)
    .bind(command.command_id.as_str())
    .bind(serde_json::to_string(&command)?)
    .bind(command.issued_at)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        r#"
        insert into command_dispatch_outbox (
            command_id, target_node_id, command_json, enqueued_at
        ) values (?1, ?2, ?3, ?4)
        on conflict(command_id) do nothing
        "#,
    )
    .bind(command.command_id.as_str())
    .bind(command.target.node_id().as_str())
    .bind(serde_json::to_string(command)?)
    .bind(command.issued_at)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

pub(crate) async fn record_turn_submission(
    state: &AppState,
    command: &CommandEnvelope,
    turn_id: &TurnId,
    user_message_id: &MessageId,
    session_id: &SessionThreadId,
    content: &str,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    let mut transaction = state.pool.begin().await?;
    let (actor_key, actor_kind, actor_display_name) = actor_identity(&command.actor_ref);
    sqlx::query(
        r#"
        insert into actors (
            actor_key, actor_kind, display_name, actor_ref_json, first_seen_at, last_seen_at
        ) values (?1, ?2, ?3, ?4, ?5, ?5)
        on conflict(actor_key) do update set
            actor_ref_json = excluded.actor_ref_json,
            display_name = excluded.display_name,
            last_seen_at = excluded.last_seen_at
        "#,
    )
    .bind(actor_key)
    .bind(actor_kind)
    .bind(actor_display_name)
    .bind(serde_json::to_string(&command.actor_ref)?)
    .bind(command.issued_at)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        insert into commands (
            command_id, kind, state, target_node_id, session_thread_id,
            runtime_session_id, project_placement_id, actor_ref_json, correlation_id,
            source_refs_json, cause_refs_json, payload_json, dedupe_key, command_json,
            created_at, completed_at
        ) values (?1, ?2, 'pending_dispatch', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, null)
        "#,
    )
    .bind(command.command_id.as_str())
    .bind(format!("{:?}", command.kind))
    .bind(command.target.node_id().as_str())
    .bind(command.target.session_thread_id().map(SessionThreadId::as_str))
    .bind(command.target.runtime_session_id().map(RuntimeSessionId::as_str))
    .bind(command.target.project_placement_id().map(ProjectPlacementId::as_str))
    .bind(serde_json::to_string(&command.actor_ref)?)
    .bind(command.correlation_id.as_str())
    .bind(serde_json::to_string(&command.source_refs)?)
    .bind(serde_json::to_string(&command.cause_refs)?)
    .bind(serde_json::to_string(&command.payload)?)
    .bind(command.command_id.as_str())
    .bind(serde_json::to_string(command)?)
    .bind(command.issued_at)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        insert into command_dispatch_outbox (
            command_id, target_node_id, command_json, enqueued_at
        ) values (?1, ?2, ?3, ?4)
        on conflict(command_id) do nothing
        "#,
    )
    .bind(command.command_id.as_str())
    .bind(command.target.node_id().as_str())
    .bind(serde_json::to_string(command)?)
    .bind(command.issued_at)
    .execute(&mut *transaction)
    .await?;

    let turn_index: i64 = sqlx::query_scalar(
        "select coalesce(max(turn_index), 0) + 1 from turns where session_thread_id = ?1",
    )
    .bind(session_id.as_str())
    .fetch_one(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        insert into turns (
            turn_id, session_thread_id, command_id, turn_index, state, content,
            blocked_approval_id, created_at, updated_at, completed_at
        ) values (?1, ?2, ?3, ?4, 'created', ?5, null, ?6, ?6, null)
        "#,
    )
    .bind(turn_id.as_str())
    .bind(session_id.as_str())
    .bind(command.command_id.as_str())
    .bind(turn_index)
    .bind(content)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        insert into messages (
            message_id, session_thread_id, turn_id, role, content,
            created_at, completed_at, source_event_id
        ) values (?1, ?2, ?3, 'user', ?4, ?5, ?5, null)
        "#,
    )
    .bind(user_message_id.as_str())
    .bind(session_id.as_str())
    .bind(turn_id.as_str())
    .bind(content)
    .bind(now)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(())
}

pub(crate) async fn update_turn_from_event_on_connection(
    connection: &mut SqliteConnection,
    event: &EventEnvelope,
) -> Result<(), AppError> {
    let Some(turn_id) = &event.turn_id else {
        return Ok(());
    };
    let Some(turn_state) = turn_state_for_event(event) else {
        return Ok(());
    };
    let completed_at = matches!(
        turn_state,
        TurnState::Completed | TurnState::Interrupted | TurnState::Failed
    )
    .then_some(event.happened_at);
    let blocked_approval_id = if turn_state == TurnState::BlockedOnApproval {
        event_approval_id(event)
    } else {
        None
    };

    sqlx::query(
        r#"
        update turns
        set state = ?1,
            blocked_approval_id = ?2,
            completed_at = coalesce(?3, completed_at),
            updated_at = ?4
        where turn_id = ?5
        "#,
    )
    .bind(format_turn_state(turn_state))
    .bind(blocked_approval_id.as_ref().map(ApprovalId::as_str))
    .bind(completed_at)
    .bind(event.happened_at)
    .bind(turn_id.as_str())
    .execute(&mut *connection)
    .await?;
    Ok(())
}

pub(crate) fn turn_state_for_event(event: &EventEnvelope) -> Option<TurnState> {
    match event.kind {
        EventKind::TurnStarted => Some(TurnState::Running),
        EventKind::TurnCompleted => Some(TurnState::Completed),
        EventKind::TurnInterrupted => Some(TurnState::Interrupted),
        EventKind::ApprovalRequested | EventKind::ProviderInteractionRequested => {
            Some(TurnState::BlockedOnApproval)
        }
        EventKind::ProviderInteractionResolved => Some(TurnState::Running),
        EventKind::RuntimeError => Some(TurnState::Failed),
        _ => None,
    }
}

pub(crate) async fn update_approval_from_event_on_connection(
    connection: &mut SqliteConnection,
    event: &EventEnvelope,
) -> Result<(), AppError> {
    match event.kind {
        EventKind::ApprovalRequested => {
            record_approval_request_on_connection(connection, event).await
        }
        EventKind::ApprovalResolved => {
            record_approval_resolution_on_connection(connection, event).await
        }
        _ => Ok(()),
    }
}

pub(crate) async fn record_approval_request_on_connection(
    connection: &mut SqliteConnection,
    event: &EventEnvelope,
) -> Result<(), AppError> {
    let Some(approval_id) = event_approval_id(event) else {
        return Ok(());
    };
    let Some(session_thread_id) = &event.session_thread_id else {
        return Ok(());
    };

    sqlx::query(
        r#"
        insert into approvals (
            approval_id, session_thread_id, runtime_session_id, turn_id, state,
            request_payload_json, response_payload_json, request_command_id,
            resolve_command_id, requested_event_id, resolved_event_id,
            created_at, updated_at, resolved_at
        )
        values (?1, ?2, ?3, ?4, ?5, ?6, null, ?7, null, ?8, null, ?9, ?9, null)
        on conflict(approval_id) do update set
            session_thread_id = excluded.session_thread_id,
            runtime_session_id = excluded.runtime_session_id,
            turn_id = excluded.turn_id,
            state = excluded.state,
            request_payload_json = excluded.request_payload_json,
            request_command_id = excluded.request_command_id,
            requested_event_id = excluded.requested_event_id,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(approval_id.as_str())
    .bind(session_thread_id.as_str())
    .bind(
        event
            .runtime_session_id
            .as_ref()
            .map(RuntimeSessionId::as_str),
    )
    .bind(event.turn_id.as_ref().map(TurnId::as_str))
    .bind(format_approval_state(ApprovalState::Requested))
    .bind(serde_json::to_string(&event.payload)?)
    .bind(event.command_id.as_ref().map(CommandId::as_str))
    .bind(event.event_id.as_str())
    .bind(event.happened_at)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

pub(crate) async fn record_approval_resolution_on_connection(
    connection: &mut SqliteConnection,
    event: &EventEnvelope,
) -> Result<(), AppError> {
    let Some(approval_id) = event_approval_id(event) else {
        return Ok(());
    };
    let Some(session_thread_id) = &event.session_thread_id else {
        return Ok(());
    };

    sqlx::query(
        r#"
        insert into approvals (
            approval_id, session_thread_id, runtime_session_id, turn_id, state,
            request_payload_json, response_payload_json, request_command_id,
            resolve_command_id, requested_event_id, resolved_event_id,
            created_at, updated_at, resolved_at
        )
        values (?1, ?2, ?3, ?4, ?5, '{}', ?6, null, ?7, null, ?8, ?9, ?9, ?9)
        on conflict(approval_id) do update set
            session_thread_id = excluded.session_thread_id,
            runtime_session_id = excluded.runtime_session_id,
            turn_id = coalesce(excluded.turn_id, approvals.turn_id),
            state = excluded.state,
            response_payload_json = excluded.response_payload_json,
            resolve_command_id = excluded.resolve_command_id,
            resolved_event_id = excluded.resolved_event_id,
            resolved_at = excluded.resolved_at,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(approval_id.as_str())
    .bind(session_thread_id.as_str())
    .bind(
        event
            .runtime_session_id
            .as_ref()
            .map(RuntimeSessionId::as_str),
    )
    .bind(event.turn_id.as_ref().map(TurnId::as_str))
    .bind(format_approval_state(ApprovalState::Resolved))
    .bind(serde_json::to_string(&event.payload)?)
    .bind(event.command_id.as_ref().map(CommandId::as_str))
    .bind(event.event_id.as_str())
    .bind(event.happened_at)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

#[cfg(test)]
pub(crate) struct NewEvent {
    pub(crate) command_id: Option<CommandId>,
    pub(crate) actor_ref: ActorRef,
    pub(crate) scope_ref: ScopeRef,
    pub(crate) node_id: Option<NodeId>,
    pub(crate) runtime_session_id: Option<RuntimeSessionId>,
    pub(crate) session_thread_id: Option<SessionThreadId>,
    pub(crate) turn_id: Option<TurnId>,
    pub(crate) kind: EventKind,
    pub(crate) payload: serde_json::Value,
}

#[cfg(test)]
pub(crate) async fn append_event(
    state: &AppState,
    new_event: NewEvent,
) -> Result<EventEnvelope, AppError> {
    let scope_key = scope_key(&new_event.scope_ref);
    let seq = next_seq(state, &scope_key).await?;
    let now = Utc::now();
    let session_projection_seq = match &new_event.session_thread_id {
        Some(session_id) => Some(next_session_projection_seq(state, session_id).await?),
        None => None,
    };
    let event = EventEnvelope {
        event_id: EventId::new(),
        command_id: new_event.command_id,
        correlation_id: None,
        actor_ref: new_event.actor_ref,
        scope_ref: new_event.scope_ref,
        node_id: new_event.node_id,
        runtime_session_id: new_event.runtime_session_id,
        session_thread_id: new_event.session_thread_id,
        turn_id: new_event.turn_id,
        seq,
        session_projection_seq,
        kind: new_event.kind,
        happened_at: now,
        source_refs: vec![],
        evidence_refs: vec![],
        cause_refs: vec![],
        result_refs: vec![],
        payload: EventPayload::from_json(new_event.kind, new_event.payload),
    };
    insert_event_record(state, &scope_key, &event).await?;
    enqueue_event_publication(state, &event).await?;
    drain_event_publication_outbox(state).await?;
    Ok(event)
}

#[cfg(test)]
pub(crate) async fn enqueue_event_publication(
    state: &AppState,
    event: &EventEnvelope,
) -> Result<(), AppError> {
    let mut transaction = state.pool.begin().await?;
    enqueue_event_publication_on_connection(&mut transaction, event).await?;
    transaction.commit().await?;
    Ok(())
}

pub(crate) async fn enqueue_event_publication_on_connection(
    connection: &mut SqliteConnection,
    event: &EventEnvelope,
) -> Result<(), AppError> {
    sqlx::query(
        "insert into event_publication_outbox (event_id, event_json, enqueued_at) values (?1, ?2, ?3) on conflict(event_id) do nothing",
    )
    .bind(event.event_id.as_str())
    .bind(serde_json::to_string(event)?)
    .bind(event.happened_at)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

pub(crate) async fn drain_event_publication_outbox(state: &AppState) -> Result<(), AppError> {
    let _publish_guard = state.event_publish_lock.lock().await;
    let rows: Vec<(String, String)> = sqlx::query_as(
        "select event_id, event_json from event_publication_outbox where published_at is null order by enqueued_at, event_id",
    )
    .fetch_all(&state.pool)
    .await?;
    for (event_id, event_json) in rows {
        let event: EventEnvelope = serde_json::from_str(&event_json)?;
        if state.event_tx.send(event).is_err() {
            sqlx::query(
                "update event_publication_outbox set attempts = attempts + 1 where event_id = ?1",
            )
            .bind(&event_id)
            .execute(&state.pool)
            .await?;
            continue;
        }
        sqlx::query(
            "update event_publication_outbox set published_at = ?2 where event_id = ?1 and published_at is null",
        )
        .bind(&event_id)
        .bind(Utc::now())
        .execute(&state.pool)
        .await?;
    }
    Ok(())
}

pub(crate) fn log_event_appended(event: &EventEnvelope, stream_gap_expected_seq: Option<i64>) {
    tracing::info!(
        event_kind = ?&event.kind,
        seq = event.seq,
        correlation_id = event
            .correlation_id
            .as_ref()
            .map(CorrelationId::as_str)
            .unwrap_or("none"),
        stream_gap = stream_gap_expected_seq.is_some(),
        stream_gap_expected_seq = stream_gap_expected_seq.unwrap_or(0),
        "event appended"
    );
}

pub(crate) async fn next_seq(state: &AppState, scope_key: &str) -> Result<i64, AppError> {
    let max_seq: Option<i64> =
        sqlx::query_scalar("select max(seq) from events where scope_key = ?1")
            .bind(scope_key)
            .fetch_one(&state.pool)
            .await?;
    Ok(max_seq.unwrap_or(0) + 1)
}

pub(crate) async fn find_session_for_runtime(
    state: &AppState,
    runtime_session_id: &RuntimeSessionId,
) -> Result<SessionThreadId, AppError> {
    let session_id: Option<String> = sqlx::query_scalar(
        "select session_thread_id from runtime_sessions where runtime_session_id = ?1",
    )
    .bind(runtime_session_id.as_str())
    .fetch_optional(&state.pool)
    .await?;
    session_id
        .map(SessionThreadId::from)
        .ok_or_else(|| AppError::not_found("runtime.not_found", "Runtime not found"))
}

pub(crate) async fn update_runtime_state_on_connection(
    connection: &mut SqliteConnection,
    runtime_session_id: &RuntimeSessionId,
    runtime_state: RuntimeSessionState,
    updated_at: DateTime<Utc>,
) -> Result<(), AppError> {
    let clears_degraded_reason = matches!(
        runtime_state,
        RuntimeSessionState::Starting
            | RuntimeSessionState::Ready
            | RuntimeSessionState::Running
            | RuntimeSessionState::Resuming
    );
    if clears_degraded_reason {
        sqlx::query(
            r#"
            update runtime_sessions
            set state = ?1, degraded_reason = null, updated_at = ?2
            where runtime_session_id = ?3
            "#,
        )
        .bind(format_runtime_state(runtime_state))
        .bind(updated_at)
        .bind(runtime_session_id.as_str())
        .execute(&mut *connection)
        .await?;
    } else {
        sqlx::query(
            r#"
            update runtime_sessions
            set state = ?1, updated_at = ?2
            where runtime_session_id = ?3
            "#,
        )
        .bind(format_runtime_state(runtime_state))
        .bind(updated_at)
        .bind(runtime_session_id.as_str())
        .execute(&mut *connection)
        .await?;
    }
    tracing::info!(
        runtime_state = ?runtime_state,
        "runtime state updated"
    );
    Ok(())
}

pub(crate) async fn update_runtime_provider_resume_ref_on_connection(
    connection: &mut SqliteConnection,
    runtime_session_id: &RuntimeSessionId,
    event: &EventEnvelope,
) -> Result<(), AppError> {
    let Some(provider_resume_ref_json) = provider_resume_ref_json(event)? else {
        return Ok(());
    };
    sqlx::query(
        r#"
        update runtime_sessions
        set provider_resume_ref_json = ?1, updated_at = ?2
        where runtime_session_id = ?3
        "#,
    )
    .bind(provider_resume_ref_json)
    .bind(event.happened_at)
    .bind(runtime_session_id.as_str())
    .execute(&mut *connection)
    .await?;
    Ok(())
}

pub(crate) fn provider_resume_ref_json(event: &EventEnvelope) -> Result<Option<String>, AppError> {
    if let Some(provider_resume_ref) = event
        .payload
        .0
        .get("provider_resume_ref")
        .filter(|value| !value.is_null())
    {
        return serde_json::to_string(provider_resume_ref)
            .map(Some)
            .map_err(AppError::from);
    }

    let mut resume_ref = serde_json::Map::new();
    if let Some(provider_session_id) = event
        .payload
        .0
        .get("provider_session_id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        resume_ref.insert(
            "provider_session_id".to_owned(),
            serde_json::Value::String(snippet(provider_session_id, 512)),
        );
    }
    if let Some(resume_cursor) = event
        .payload
        .0
        .get("resume_cursor")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        resume_ref.insert(
            "resume_cursor".to_owned(),
            serde_json::Value::String(snippet(resume_cursor, 512)),
        );
    }
    if resume_ref.is_empty() {
        Ok(None)
    } else {
        serde_json::to_string(&serde_json::Value::Object(resume_ref))
            .map(Some)
            .map_err(AppError::from)
    }
}

pub(crate) async fn touch_runtime_step_on_connection(
    connection: &mut SqliteConnection,
    runtime_session_id: &RuntimeSessionId,
    happened_at: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        update runtime_sessions
        set last_runtime_step_at = ?1, updated_at = ?2
        where runtime_session_id = ?3
        "#,
    )
    .bind(happened_at)
    .bind(happened_at)
    .bind(runtime_session_id.as_str())
    .execute(&mut *connection)
    .await?;
    Ok(())
}

pub(crate) async fn update_session_state_from_event_on_connection(
    connection: &mut SqliteConnection,
    event: &EventEnvelope,
    session_state: SessionThreadState,
) -> Result<(), AppError> {
    if let Some(session_thread_id) = &event.session_thread_id {
        if session_state == SessionThreadState::Active
            && load_session_state_on_connection(connection, session_thread_id).await?
                == Some(SessionThreadState::Detached)
        {
            return Ok(());
        }
        sqlx::query(
            "update session_threads set state = ?1, updated_at = ?2 where session_thread_id = ?3",
        )
        .bind(format_session_state(session_state))
        .bind(event.happened_at)
        .bind(session_thread_id.as_str())
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

pub(crate) async fn load_session_state_on_connection(
    connection: &mut SqliteConnection,
    session_id: &SessionThreadId,
) -> Result<Option<SessionThreadState>, AppError> {
    let state_value: Option<String> =
        sqlx::query_scalar("select state from session_threads where session_thread_id = ?1")
            .bind(session_id.as_str())
            .fetch_optional(&mut *connection)
            .await?;
    Ok(state_value.map(|value| parse_session_state(&value)))
}

pub(crate) async fn update_session_attachment_state(
    state: &AppState,
    session_id: &SessionThreadId,
    target_state: SessionThreadState,
) -> Result<(), AppError> {
    let current_state: String =
        sqlx::query_scalar("select state from session_threads where session_thread_id = ?1")
            .bind(session_id.as_str())
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| AppError::not_found("session.not_found", "Session not found"))?;
    let current_state = parse_session_state(&current_state);
    if current_state == SessionThreadState::Stopped {
        return Err(AppError::bad_request(
            "session.stopped",
            "Stopped sessions cannot be attached or detached",
        ));
    }
    if current_state == target_state {
        return Ok(());
    }

    sqlx::query(
        "update session_threads set state = ?1, updated_at = ?2 where session_thread_id = ?3",
    )
    .bind(format_session_state(target_state))
    .bind(Utc::now())
    .bind(session_id.as_str())
    .execute(&state.pool)
    .await?;
    Ok(())
}

pub(crate) async fn mark_event_stream_gap_on_connection(
    connection: &mut SqliteConnection,
    event: &EventEnvelope,
    expected_seq: i64,
) -> Result<(), AppError> {
    let reason = format!(
        "event sequence gap: expected {expected_seq}, received {}",
        event.seq
    );
    if let Some(runtime_session_id) = &event.runtime_session_id {
        sqlx::query(
            r#"
            update runtime_sessions
            set state = ?1, degraded_reason = ?2, updated_at = ?3
            where runtime_session_id = ?4
            "#,
        )
        .bind(format_runtime_state(RuntimeSessionState::Stale))
        .bind(&reason)
        .bind(event.happened_at)
        .bind(runtime_session_id.as_str())
        .execute(&mut *connection)
        .await?;
    }
    if let Some(session_thread_id) = &event.session_thread_id {
        sqlx::query(
            "update session_threads set state = ?1, updated_at = ?2 where session_thread_id = ?3",
        )
        .bind(format_session_state(SessionThreadState::Degraded))
        .bind(event.happened_at)
        .bind(session_thread_id.as_str())
        .execute(&mut *connection)
        .await?;
    }
    tracing::warn!(
        event_kind = ?&event.kind,
        expected_seq,
        received_seq = event.seq,
        "event stream marked degraded"
    );
    Ok(())
}
