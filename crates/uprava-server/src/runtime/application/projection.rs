//! Core read-model, trace, evidence and agent projection assembly.

use super::super::*;

pub(crate) async fn load_inventory(state: &AppState) -> Result<InventorySnapshot, AppError> {
    expire_idle_runtimes(state).await?;
    Ok(InventorySnapshot {
        nodes: load_nodes(state).await?,
        placements: load_placements(state).await?,
        sessions: load_sessions(state).await?,
        generated_at: Utc::now(),
    })
}

pub(crate) async fn load_nodes(
    state: &AppState,
) -> Result<Vec<uprava_protocol::NodeSummary>, AppError> {
    let rows = sqlx::query(
        r#"
        select node_id, display_name, presence, sleep_hint, last_heartbeat_at,
               active_runtime_count, capabilities_json, diagnostics
        from nodes
        order by updated_at desc
        "#,
    )
    .fetch_all(&state.pool)
    .await?;
    let now = Utc::now();

    rows.into_iter()
        .map(|row| {
            let last_heartbeat_at: Option<DateTime<Utc>> = row.try_get("last_heartbeat_at")?;
            let heartbeat_age_seconds = last_heartbeat_at
                .map(|timestamp| now.signed_duration_since(timestamp).num_seconds().max(0));
            let presence = derive_presence(
                parse_presence(row.try_get::<String, _>("presence")?.as_str()),
                heartbeat_age_seconds,
                state.config.stale_after_seconds,
                state.config.offline_after_seconds,
            );
            let capabilities_json: String = row.try_get("capabilities_json")?;
            let capabilities = serde_json::from_str::<Vec<CapabilitySummary>>(&capabilities_json)?;
            Ok(uprava_protocol::NodeSummary {
                node_id: NodeId::from(row.try_get::<String, _>("node_id")?),
                display_name: row.try_get("display_name")?,
                presence,
                sleep_hint: parse_sleep_hint(row.try_get::<String, _>("sleep_hint")?.as_str()),
                heartbeat_age_seconds,
                active_runtime_count: row.try_get("active_runtime_count")?,
                capabilities,
                diagnostics: row.try_get("diagnostics")?,
            })
        })
        .collect()
}

pub(crate) async fn load_placements(
    state: &AppState,
) -> Result<Vec<ProjectPlacementSummary>, AppError> {
    expire_idle_runtimes(state).await?;
    let rows = sqlx::query(
        r#"
        select project_placement_id, project_id, node_id, display_name, workspace_path,
               state, resource_badges_json, git_snapshot_json, last_validated_at
        from project_placements
        order by updated_at desc
        "#,
    )
    .fetch_all(&state.pool)
    .await?;
    let mut placements = Vec::with_capacity(rows.len());
    for row in rows {
        let placement = row_to_placement(row)?;
        placements.push(add_core_resource_badges(state, placement, None).await?);
    }
    Ok(placements)
}

pub(crate) async fn load_placement(
    state: &AppState,
    placement_id: &ProjectPlacementId,
) -> Result<ProjectPlacementSummary, AppError> {
    load_placement_with_excluded_session(state, placement_id, None).await
}

pub(crate) async fn load_placement_for_session(
    state: &AppState,
    placement_id: &ProjectPlacementId,
    session_id: &SessionThreadId,
) -> Result<ProjectPlacementSummary, AppError> {
    load_placement_with_excluded_session(state, placement_id, Some(session_id)).await
}

pub(crate) async fn load_placement_with_excluded_session(
    state: &AppState,
    placement_id: &ProjectPlacementId,
    excluded_session_id: Option<&SessionThreadId>,
) -> Result<ProjectPlacementSummary, AppError> {
    expire_idle_runtimes(state).await?;
    let row = sqlx::query(
        r#"
        select project_placement_id, project_id, node_id, display_name, workspace_path,
               state, resource_badges_json, git_snapshot_json, last_validated_at
        from project_placements
        where project_placement_id = ?1
        "#,
    )
    .bind(placement_id.as_str())
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("placement.not_found", "Placement not found"))?;
    let placement = row_to_placement(row)?;
    add_core_resource_badges(state, placement, excluded_session_id).await
}

pub(crate) fn row_to_placement(
    row: sqlx::sqlite::SqliteRow,
) -> Result<ProjectPlacementSummary, AppError> {
    let badges_json: String = row.try_get("resource_badges_json")?;
    let project_id: Option<String> = row.try_get("project_id")?;
    let git_snapshot_json: Option<String> = row.try_get("git_snapshot_json")?;
    Ok(ProjectPlacementSummary {
        project_placement_id: ProjectPlacementId::from(
            row.try_get::<String, _>("project_placement_id")?,
        ),
        project_id: project_id.map(ProjectId::from),
        node_id: NodeId::from(row.try_get::<String, _>("node_id")?),
        display_name: row.try_get("display_name")?,
        workspace_path: row.try_get("workspace_path")?,
        state: parse_placement_state(row.try_get::<String, _>("state")?.as_str()),
        resource_badges: serde_json::from_str(&badges_json)?,
        git_snapshot: git_snapshot_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?,
        last_validated_at: row.try_get("last_validated_at")?,
    })
}

pub(crate) async fn add_core_resource_badges(
    state: &AppState,
    mut placement: ProjectPlacementSummary,
    excluded_session_id: Option<&SessionThreadId>,
) -> Result<ProjectPlacementSummary, AppError> {
    placement.resource_badges.retain(|badge| {
        badge.kind != "same_workspace_active" && badge.kind != "same_repo_branch_active"
    });
    let active_count =
        active_workspace_session_count(state, &placement, excluded_session_id).await?;
    if active_count > 0 {
        placement.resource_badges.push(ResourceBadge {
            kind: "same_workspace_active".to_owned(),
            severity: WarningSeverity::Warning,
            label: format!("Workspace already has {active_count} active session(s)"),
        });
    }
    let same_branch_count = active_same_repo_branch_count(state, &placement).await?;
    if same_branch_count > 0 {
        placement.resource_badges.push(ResourceBadge {
            kind: "same_repo_branch_active".to_owned(),
            severity: WarningSeverity::Warning,
            label: format!(
                "Same repository branch has {same_branch_count} active session(s) elsewhere"
            ),
        });
    }
    Ok(placement)
}

pub(crate) async fn active_same_repo_branch_count(
    state: &AppState,
    placement: &ProjectPlacementSummary,
) -> Result<i64, AppError> {
    let Some(snapshot) = &placement.git_snapshot else {
        return Ok(0);
    };
    let (Some(repo_id), Some(branch)) = (&snapshot.repo_id, &snapshot.branch) else {
        return Ok(0);
    };
    let rows = sqlx::query(
        r#"
        select pp.git_snapshot_json
        from project_placements pp
        join session_threads st on st.project_placement_id = pp.project_placement_id
        join runtime_sessions rs on rs.runtime_session_id = st.runtime_session_id
        where pp.project_placement_id != ?1
          and pp.git_snapshot_json is not null
          and st.state in ('active', 'detached', 'degraded')
          and rs.state in (
              'starting', 'ready', 'running', 'blocked',
              'stopping', 'interrupted', 'resuming', 'stale'
          )
        "#,
    )
    .bind(placement.project_placement_id.as_str())
    .fetch_all(&state.pool)
    .await?;
    let mut count = 0i64;
    for row in rows {
        let raw: String = row.try_get("git_snapshot_json")?;
        let candidate: GitWorkspaceSnapshot = serde_json::from_str(&raw)?;
        if candidate.repo_id.as_ref() == Some(repo_id) && candidate.branch.as_ref() == Some(branch)
        {
            count += 1;
        }
    }
    Ok(count)
}

pub(crate) async fn active_workspace_session_count(
    state: &AppState,
    placement: &ProjectPlacementSummary,
    excluded_session_id: Option<&SessionThreadId>,
) -> Result<i64, AppError> {
    sqlx::query_scalar(
        r#"
        select count(*)
        from session_threads st
        join project_placements pp on pp.project_placement_id = st.project_placement_id
        join runtime_sessions rs on rs.runtime_session_id = st.runtime_session_id
        where pp.node_id = ?1
          and pp.workspace_path = ?2
          and (?3 is null or st.session_thread_id != ?3)
          and st.state in ('active', 'detached', 'degraded')
          and rs.state in (
              'starting', 'ready', 'running', 'blocked',
              'stopping', 'interrupted', 'resuming', 'stale'
          )
        "#,
    )
    .bind(placement.node_id.as_str())
    .bind(&placement.workspace_path)
    .bind(excluded_session_id.map(SessionThreadId::as_str))
    .fetch_one(&state.pool)
    .await
    .map_err(AppError::from)
}

pub(crate) async fn expire_idle_runtimes(state: &AppState) -> Result<(), AppError> {
    if state.config.runtime_expiry_seconds <= 0 {
        return Ok(());
    }
    let now = Utc::now();
    let cutoff = now - chrono::Duration::seconds(state.config.runtime_expiry_seconds);
    let rows = sqlx::query(
        r#"
        select rs.runtime_session_id, rs.session_thread_id, pp.node_id,
               pp.project_placement_id
        from runtime_sessions rs
        join session_threads st on st.session_thread_id = rs.session_thread_id
        join project_placements pp on pp.project_placement_id = st.project_placement_id
        where rs.state in ('ready', 'running', 'blocked', 'stale')
          and coalesce(rs.last_runtime_step_at, rs.created_at) <= ?1
        "#,
    )
    .bind(cutoff)
    .fetch_all(&state.pool)
    .await?;

    for row in rows {
        let runtime_session_id =
            RuntimeSessionId::from(row.try_get::<String, _>("runtime_session_id")?);
        let session_thread_id =
            SessionThreadId::from(row.try_get::<String, _>("session_thread_id")?);
        let node_id = NodeId::from(row.try_get::<String, _>("node_id")?);
        let project_placement_id =
            ProjectPlacementId::from(row.try_get::<String, _>("project_placement_id")?);
        let stop_already_recorded: bool = sqlx::query_scalar(
            r#"
            select exists(
                select 1 from commands
                where runtime_session_id = ?1
                  and kind = 'StopRuntime'
                  and state not in ('completed', 'failed', 'expired')
            )
            "#,
        )
        .bind(runtime_session_id.as_str())
        .fetch_one(&state.pool)
        .await?;
        if !stop_already_recorded {
            record_and_dispatch_command(
                state,
                CommandEnvelope {
                    command_id: CommandId::new(),
                    kind: CommandKind::StopRuntime,
                    target: CommandTarget::SessionRuntime {
                        node_id: node_id.clone(),
                        project_placement_id,
                        session_thread_id: session_thread_id.clone(),
                        runtime_session_id: runtime_session_id.clone(),
                    },
                    actor_ref: ActorRef::System,
                    source_refs: vec![UpravaRef::Runtime {
                        runtime_session_id: runtime_session_id.clone(),
                    }],
                    cause_refs: vec![],
                    issued_at: now,
                    correlation_id: CorrelationId::new(),
                    payload: CommandPayload::StopRuntime {
                        runtime_attempt_id: None,
                        reason: Some("idle_expired".to_owned()),
                    },
                },
            )
            .await?;
        }
        revoke_session_mcp_leases(state, &session_thread_id, "runtime_idle_expired").await?;
        let seq = next_seq(
            state,
            &scope_key(&ScopeRef::Runtime {
                runtime_session_id: runtime_session_id.clone(),
            }),
        )
        .await?;
        accept_node_event(
            state,
            EventEnvelope {
                event_id: EventId::new(),
                command_id: None,
                correlation_id: None,
                actor_ref: ActorRef::System,
                scope_ref: ScopeRef::Runtime {
                    runtime_session_id: runtime_session_id.clone(),
                },
                node_id: Some(node_id),
                runtime_session_id: Some(runtime_session_id.clone()),
                session_thread_id: Some(session_thread_id),
                turn_id: None,
                seq,
                session_projection_seq: None,
                kind: EventKind::RuntimeExpired,
                happened_at: now,
                source_refs: vec![UpravaRef::Runtime { runtime_session_id }],
                evidence_refs: vec![],
                cause_refs: vec![],
                result_refs: vec![],
                payload: EventPayload::from_json(
                    EventKind::RuntimeExpired,
                    json!({
                        "code": "runtime.idle_expired",
                        "message": format!(
                            "Runtime expired after {} seconds without runtime activity",
                            state.config.runtime_expiry_seconds
                        ),
                        "expiry_seconds": state.config.runtime_expiry_seconds,
                    }),
                ),
            },
        )
        .await?;
    }
    Ok(())
}

pub(crate) async fn load_sessions(state: &AppState) -> Result<Vec<SessionSummary>, AppError> {
    expire_idle_runtimes(state).await?;
    let rows = sqlx::query(
        r#"
        select st.session_thread_id, st.project_placement_id, st.runtime_session_id, st.title,
               st.state as session_state, st.updated_at, rs.provider, rs.state as runtime_state,
               rs.execution_profile, rs.resume_supported, rs.degraded_reason,
               rs.last_runtime_step_at, rs.effective_policy_json, rs.effective_policy_hash,
               rs.recovery_status, ra.runtime_attempt_id, ra.state as attempt_state,
               ra.started_at as attempt_started_at, ra.ready_at as attempt_ready_at,
               ra.stopped_at as attempt_stopped_at, ra.start_reason as attempt_start_reason,
               ra.stop_reason as attempt_stop_reason, ra.recovery_reason as attempt_recovery_reason,
               (select count(*) from messages m where m.session_thread_id = st.session_thread_id) as message_count
        from session_threads st
        join runtime_sessions rs on rs.runtime_session_id = st.runtime_session_id
        left join runtime_attempts ra on ra.runtime_attempt_id = rs.current_attempt_id
        order by st.updated_at desc
        "#,
    )
    .fetch_all(&state.pool)
    .await?;
    rows.into_iter().map(row_to_session).collect()
}

pub(crate) fn row_to_session(row: sqlx::sqlite::SqliteRow) -> Result<SessionSummary, AppError> {
    let runtime_session_id =
        RuntimeSessionId::from(row.try_get::<String, _>("runtime_session_id")?);
    let effective_policy_json: Option<String> = row.try_get("effective_policy_json")?;
    let effective_policy = effective_policy_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;
    let effective_policy_hash: Option<String> = row.try_get("effective_policy_hash")?;
    let current_attempt_id: Option<String> = row.try_get("runtime_attempt_id")?;
    let current_attempt = current_attempt_id
        .map(
            |runtime_attempt_id| -> Result<RuntimeAttemptSummary, AppError> {
                Ok(RuntimeAttemptSummary {
                    runtime_attempt_id: RuntimeAttemptId::from(runtime_attempt_id),
                    state: parse_runtime_attempt_state(
                        row.try_get::<String, _>("attempt_state")?.as_str(),
                    ),
                    started_at: row.try_get("attempt_started_at")?,
                    ready_at: row.try_get("attempt_ready_at")?,
                    stopped_at: row.try_get("attempt_stopped_at")?,
                    start_reason: row.try_get("attempt_start_reason")?,
                    stop_reason: row.try_get("attempt_stop_reason")?,
                    recovery_reason: row.try_get("attempt_recovery_reason")?,
                })
            },
        )
        .transpose()?;
    Ok(SessionSummary {
        session_thread_id: SessionThreadId::from(row.try_get::<String, _>("session_thread_id")?),
        project_placement_id: ProjectPlacementId::from(
            row.try_get::<String, _>("project_placement_id")?,
        ),
        runtime_session_id: runtime_session_id.clone(),
        title: row.try_get("title")?,
        state: parse_session_state(row.try_get::<String, _>("session_state")?.as_str()),
        runtime: RuntimeSummary {
            runtime_session_id,
            provider: row.try_get("provider")?,
            execution_profile: parse_execution_profile(
                row.try_get::<String, _>("execution_profile")?.as_str(),
            ),
            state: parse_runtime_state(row.try_get::<String, _>("runtime_state")?.as_str()),
            resume_supported: row.try_get::<i64, _>("resume_supported")? != 0,
            degraded_reason: row.try_get("degraded_reason")?,
            last_runtime_step_at: row.try_get("last_runtime_step_at")?,
            current_attempt,
            effective_policy,
            effective_policy_hash: effective_policy_hash.map(RuntimePolicyHash),
            recovery_status: parse_runtime_recovery_status(
                row.try_get::<String, _>("recovery_status")?.as_str(),
            ),
        },
        message_count: row.try_get("message_count")?,
        updated_at: row.try_get("updated_at")?,
    })
}

pub(crate) async fn load_session_detail(
    state: &AppState,
    session_id: &SessionThreadId,
) -> Result<SessionDetail, AppError> {
    let session = load_sessions(state)
        .await?
        .into_iter()
        .find(|candidate| candidate.session_thread_id == *session_id)
        .ok_or_else(|| AppError::not_found("session.not_found", "Session not found"))?;
    let placement =
        load_placement_for_session(state, &session.project_placement_id, session_id).await?;
    let messages = load_messages(state, session_id).await?;
    let events = load_events(state, session_id, 0).await?;
    let scheduled_messages = load_scheduled_messages(state, session_id).await?;
    let pending_interactions = load_pending_provider_interactions(state, session_id).await?;
    Ok(SessionDetail {
        session,
        placement,
        messages,
        events,
        scheduled_messages,
        pending_interactions,
    })
}

pub(crate) async fn load_pending_provider_interactions(
    state: &AppState,
    session_id: &SessionThreadId,
) -> Result<Vec<ProviderInteractionSummary>, AppError> {
    let rows = sqlx::query(
        r#"
        select provider_interaction_id, runtime_attempt_id, interaction_kind, state,
               request_payload_json, requested_at, resolved_at
        from provider_interactions
        where session_thread_id = ?1 and state in ('requested', 'resolving')
        order by requested_at, provider_interaction_id
        "#,
    )
    .bind(session_id.as_str())
    .fetch_all(&state.pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            let request_payload_json: String = row.try_get("request_payload_json")?;
            let request_payload: serde_json::Value = serde_json::from_str(&request_payload_json)?;
            let interaction_kind = match row.try_get::<String, _>("interaction_kind")?.as_str() {
                "user_input" => ProviderInteractionKind::UserInput,
                _ => ProviderInteractionKind::Approval,
            };
            let state = match row.try_get::<String, _>("state")?.as_str() {
                "resolving" => ProviderInteractionState::Resolving,
                "approved" => ProviderInteractionState::Approved,
                "denied" => ProviderInteractionState::Denied,
                "answered" => ProviderInteractionState::Answered,
                "expired" => ProviderInteractionState::Expired,
                "cancelled" => ProviderInteractionState::Cancelled,
                "superseded" => ProviderInteractionState::Superseded,
                _ => ProviderInteractionState::Requested,
            };
            Ok(ProviderInteractionSummary {
                provider_interaction_id: ProviderInteractionId::from(
                    row.try_get::<String, _>("provider_interaction_id")?,
                ),
                runtime_attempt_id: RuntimeAttemptId::from(
                    row.try_get::<String, _>("runtime_attempt_id")?,
                ),
                kind: interaction_kind,
                state,
                prompt: request_payload
                    .get("prompt")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                requested_at: row.try_get("requested_at")?,
                resolved_at: row.try_get("resolved_at")?,
            })
        })
        .collect()
}

pub(crate) async fn load_messages(
    state: &AppState,
    session_id: &SessionThreadId,
) -> Result<Vec<Message>, AppError> {
    let rows = sqlx::query(
        r#"
        select message_id, session_thread_id, turn_id, role, content,
               created_at, completed_at, source_event_id
        from messages
        where session_thread_id = ?1
        order by created_at asc
        "#,
    )
    .bind(session_id.as_str())
    .fetch_all(&state.pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            let turn_id: Option<String> = row.try_get("turn_id")?;
            let source_event_id: Option<String> = row.try_get("source_event_id")?;
            Ok(Message {
                message_id: MessageId::from(row.try_get::<String, _>("message_id")?),
                session_thread_id: SessionThreadId::from(
                    row.try_get::<String, _>("session_thread_id")?,
                ),
                turn_id: turn_id.map(TurnId::from),
                role: parse_message_role(row.try_get::<String, _>("role")?.as_str()),
                content: row.try_get("content")?,
                created_at: row.try_get("created_at")?,
                completed_at: row.try_get("completed_at")?,
                source_event_id: source_event_id.map(EventId::from),
            })
        })
        .collect()
}

pub(crate) async fn load_events(
    state: &AppState,
    session_id: &SessionThreadId,
    after_seq: i64,
) -> Result<Vec<EventEnvelope>, AppError> {
    let rows = sqlx::query(
        r#"
        select event_json, session_projection_seq
        from events
        where session_thread_id = ?1 and session_projection_seq > ?2
        order by session_projection_seq asc
        "#,
    )
    .bind(session_id.as_str())
    .bind(after_seq)
    .fetch_all(&state.pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            let event_json: String = row.try_get("event_json")?;
            let mut event = serde_json::from_str::<EventEnvelope>(&event_json)?;
            event.session_projection_seq = row.try_get("session_projection_seq")?;
            Ok(event)
        })
        .collect()
}

pub(crate) async fn load_event_log_page(
    state: &AppState,
    query: EventLogQuery,
) -> Result<EventLogPage, AppError> {
    let limit = query
        .limit
        .unwrap_or(DEFAULT_EVENT_LOG_LIMIT)
        .clamp(1, MAX_EVENT_LOG_LIMIT);
    let (cursor_at, cursor_id) = query
        .cursor
        .as_deref()
        .map(parse_event_log_cursor)
        .transpose()?
        .map_or((None, None), |(at, id)| (Some(at), Some(id)));
    let kind = query
        .kind
        .as_deref()
        .map(parse_event_kind_filter)
        .transpose()?;
    let rows = sqlx::query(
        r#"
        select event_json
        from events
        where (?1 is null or session_thread_id = ?1)
          and (
            ?2 is null
            or scope_key = ('placement:' || ?2)
            or session_thread_id in (
                select session_thread_id from session_threads where project_placement_id = ?2
            )
          )
          and (?3 is null or kind = ?3)
          and (
            ?4 is null
            or happened_at < ?4
            or (happened_at = ?4 and event_id < ?5)
          )
        order by happened_at desc, event_id desc
        limit ?6
        "#,
    )
    .bind(query.session_thread_id.as_deref())
    .bind(query.placement_id.as_deref())
    .bind(kind.as_deref())
    .bind(cursor_at)
    .bind(cursor_id.as_deref())
    .bind(i64::try_from(limit + 1).unwrap_or(i64::MAX))
    .fetch_all(&state.pool)
    .await?;
    let mut events = rows
        .into_iter()
        .map(|row| {
            let event_json: String = row.try_get("event_json")?;
            serde_json::from_str(&event_json).map_err(AppError::from)
        })
        .collect::<Result<Vec<EventEnvelope>, AppError>>()?;
    let has_more = events.len() > limit;
    events.truncate(limit);
    let next_cursor = has_more
        .then(|| events.last().map(event_log_cursor))
        .flatten();
    Ok(EventLogPage {
        events,
        next_cursor,
    })
}

pub(crate) fn parse_event_kind_filter(value: &str) -> Result<String, AppError> {
    serde_json::from_value::<EventKind>(json!(value))
        .map(|kind| format!("{kind:?}"))
        .map_err(|_| AppError::bad_request("event.kind_invalid", "Unknown event kind filter"))
}

pub(crate) fn parse_event_log_cursor(value: &str) -> Result<(DateTime<Utc>, String), AppError> {
    let (at, event_id) = value.rsplit_once('|').ok_or_else(|| {
        AppError::bad_request("event.cursor_invalid", "Event cursor is malformed")
    })?;
    let at = at.parse::<DateTime<Utc>>().map_err(|_| {
        AppError::bad_request("event.cursor_invalid", "Event cursor timestamp is invalid")
    })?;
    if event_id.is_empty() {
        return Err(AppError::bad_request(
            "event.cursor_invalid",
            "Event cursor id is missing",
        ));
    }
    Ok((at, event_id.to_owned()))
}

pub(crate) fn event_log_cursor(event: &EventEnvelope) -> String {
    format!("{}|{}", event.happened_at.to_rfc3339(), event.event_id)
}

pub(crate) async fn load_event_by_id(
    state: &AppState,
    event_id: &EventId,
) -> Result<EventEnvelope, AppError> {
    let event_json: String =
        sqlx::query_scalar("select event_json from events where event_id = ?1")
            .bind(event_id.as_str())
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| AppError::not_found("event.not_found", "Event not found"))?;
    serde_json::from_str(&event_json).map_err(AppError::from)
}

pub(crate) async fn build_session_trace_projection(
    state: &AppState,
    session_id: &SessionThreadId,
) -> Result<SessionTraceProjection, AppError> {
    let detail = load_session_detail(state, session_id).await?;
    let mut ordered_steps = Vec::new();
    for message in &detail.messages {
        let primary_ref = UpravaRef::Message {
            message_id: message.message_id.clone(),
        };
        let source_refs = message_source_refs(message, &detail.events);
        ordered_steps.push((
            message.created_at,
            TraceStep {
                block_id: BlockId::from(format!("trace:message:{}", message.message_id)),
                title: format!("{:?} message", message.role),
                summary: truncate_chars(&message.content, 500),
                actor_ref: match message.role {
                    MessageRole::User => ActorRef::local_user(),
                    MessageRole::Assistant => ActorRef::Provider {
                        provider: detail.session.runtime.provider.clone(),
                    },
                    _ => ActorRef::System,
                },
                started_at: message.created_at,
                completed_at: message.completed_at,
                precision: if source_refs.is_empty() {
                    TracePrecision::Unknown
                } else {
                    TracePrecision::Exact
                },
                primary_ref,
                links: CausalityLinks {
                    source_refs,
                    cause_refs: message
                        .turn_id
                        .clone()
                        .map(|turn_id| vec![UpravaRef::Turn { turn_id }])
                        .unwrap_or_default(),
                    ..CausalityLinks::default()
                },
            },
        ));
    }

    let mut activity_groups: HashMap<String, Vec<&EventEnvelope>> = HashMap::new();
    for event in &detail.events {
        if event.kind == EventKind::ProviderOutputDelta {
            continue;
        }
        if event.kind == EventKind::ProviderActivity {
            let key = event
                .turn_id
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| event.event_id.to_string());
            activity_groups.entry(key).or_default().push(event);
            continue;
        }
        ordered_steps.push((event.happened_at, trace_step_for_event(event)));
    }
    for events in activity_groups.into_values() {
        if let Some(step) = trace_step_for_activity_group(&events) {
            ordered_steps.push((step.started_at, step));
        }
    }
    ordered_steps.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.block_id.as_str().cmp(right.1.block_id.as_str()))
    });
    Ok(SessionTraceProjection {
        session_thread_id: session_id.clone(),
        precision: TracePrecision::Coarse,
        steps: ordered_steps.into_iter().map(|(_, step)| step).collect(),
        raw_event_count: u64::try_from(detail.events.len()).unwrap_or(u64::MAX),
        generated_at: Utc::now(),
    })
}

pub(crate) fn trace_step_for_event(event: &EventEnvelope) -> TraceStep {
    let event_ref = event_ref(event);
    TraceStep {
        block_id: BlockId::from(format!("trace:event:{}", event.event_id)),
        title: event_kind_label(event.kind),
        summary: event_summary(event),
        actor_ref: event.actor_ref.clone(),
        started_at: event.happened_at,
        completed_at: Some(event.happened_at),
        precision: if event.source_refs.is_empty()
            && event.evidence_refs.is_empty()
            && event.cause_refs.is_empty()
        {
            TracePrecision::Unknown
        } else {
            TracePrecision::Exact
        },
        primary_ref: event_ref.clone(),
        links: CausalityLinks {
            source_refs: event.source_refs.clone(),
            evidence_refs: event.evidence_refs.clone(),
            cause_refs: event.cause_refs.clone(),
            result_refs: event.result_refs.clone(),
            raw_refs: vec![event_ref],
        },
    }
}

pub(crate) fn trace_step_for_activity_group(events: &[&EventEnvelope]) -> Option<TraceStep> {
    let first = *events.first()?;
    let last = *events.last()?;
    let raw_refs = events
        .iter()
        .map(|event| event_ref(event))
        .collect::<Vec<_>>();
    let summaries = events
        .iter()
        .map(|event| event_summary(event))
        .filter(|summary| !summary.is_empty())
        .take(4)
        .collect::<Vec<_>>();
    Some(TraceStep {
        block_id: BlockId::from(format!(
            "trace:activity:{}",
            first
                .turn_id
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| first.event_id.to_string())
        )),
        title: "Provider activity".to_owned(),
        summary: if summaries.is_empty() {
            format!("{} provider events", events.len())
        } else {
            summaries.join("; ")
        },
        actor_ref: first.actor_ref.clone(),
        started_at: first.happened_at,
        completed_at: Some(last.happened_at),
        precision: TracePrecision::Coarse,
        primary_ref: event_ref(first),
        links: CausalityLinks {
            source_refs: events
                .iter()
                .flat_map(|event| event.source_refs.iter().cloned())
                .collect(),
            evidence_refs: events
                .iter()
                .flat_map(|event| event.evidence_refs.iter().cloned())
                .collect(),
            cause_refs: events
                .iter()
                .flat_map(|event| event.cause_refs.iter().cloned())
                .collect(),
            result_refs: events
                .iter()
                .flat_map(|event| event.result_refs.iter().cloned())
                .collect(),
            raw_refs,
        },
    })
}

pub(crate) fn event_ref(event: &EventEnvelope) -> UpravaRef {
    UpravaRef::Event {
        event_id: event.event_id.clone(),
        scope_ref: Box::new(event.scope_ref.clone()),
        seq: event.seq,
    }
}

pub(crate) fn event_kind_label(kind: EventKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{kind:?}"))
}

pub(crate) fn event_summary(event: &EventEnvelope) -> String {
    for key in ["summary", "message", "content", "prompt", "question"] {
        if let Some(value) = event.payload.0.get(key).and_then(serde_json::Value::as_str) {
            return truncate_chars(value, 500);
        }
    }
    event_kind_label(event.kind)
}

pub(crate) async fn resolve_reference(
    state: &AppState,
    reference: UpravaRef,
) -> Result<ReferenceResolution, AppError> {
    let missing_reference = reference.clone();
    match resolve_reference_inner(state, reference).await {
        Err(AppError::NotFound { message, .. }) => {
            Ok(missing_resolution(missing_reference, &message))
        }
        result => result,
    }
}

pub(crate) async fn resolve_reference_inner(
    state: &AppState,
    reference: UpravaRef,
) -> Result<ReferenceResolution, AppError> {
    match &reference {
        UpravaRef::Event { event_id, .. } => {
            let event = load_event_by_id(state, event_id).await?;
            Ok(ReferenceResolution {
                reference: reference.clone(),
                status: ReferenceResolutionStatus::Resolved,
                title: event_kind_label(event.kind),
                summary: Some(event_summary(&event)),
                links: CausalityLinks {
                    source_refs: event.source_refs.clone(),
                    evidence_refs: event.evidence_refs.clone(),
                    cause_refs: event.cause_refs.clone(),
                    result_refs: event.result_refs.clone(),
                    raw_refs: vec![event_ref(&event)],
                },
                raw_payload: Some(JsonValue(event.payload.0.clone())),
                raw_truncated: event_payload_is_truncated(&event),
                unavailable_reason: None,
            })
        }
        UpravaRef::Message { message_id } | UpravaRef::MessageRange { message_id, .. } => {
            resolve_message_reference(state, reference.clone(), message_id).await
        }
        UpravaRef::Artifact { artifact_id }
        | UpravaRef::ArtifactVersion {
            artifact_id,
            version: _,
        } => {
            let version = match &reference {
                UpravaRef::ArtifactVersion { version, .. } => Some(*version),
                _ => None,
            };
            let detail = load_artifact_detail(state, artifact_id, version).await?;
            Ok(ReferenceResolution {
                reference,
                status: ReferenceResolutionStatus::Resolved,
                title: detail.artifact.title,
                summary: Some(format!(
                    "{} · version {} · {}",
                    detail.artifact.artifact_type,
                    detail.version.version,
                    truncate_chars(&detail.version.fallback_text, 500)
                )),
                links: CausalityLinks {
                    source_refs: detail.version.source_refs,
                    evidence_refs: detail.version.evidence_refs,
                    cause_refs: detail.version.cause_refs,
                    result_refs: Vec::new(),
                    raw_refs: detail.version.trace_refs,
                },
                raw_payload: Some(detail.version.payload),
                raw_truncated: false,
                unavailable_reason: None,
            })
        }
        UpravaRef::Command { command_id } => {
            resolve_command_reference(state, reference.clone(), command_id).await
        }
        UpravaRef::Deduction { deduction_id } => {
            let deduction = load_deduction_record(state, deduction_id).await?;
            Ok(ReferenceResolution {
                reference,
                status: if deduction.block.is_some() {
                    ReferenceResolutionStatus::Resolved
                } else if deduction.raw_fallback.is_some() {
                    ReferenceResolutionStatus::RawOnly
                } else {
                    ReferenceResolutionStatus::Missing
                },
                title: format!("Deduction {}", deduction.deduction_id),
                summary: deduction
                    .block
                    .as_ref()
                    .map(|block| block.result.conclusion.clone())
                    .or_else(|| deduction.error_message.clone()),
                links: deduction
                    .block
                    .as_ref()
                    .map(deduction_links)
                    .unwrap_or_default(),
                raw_payload: deduction
                    .raw_fallback
                    .as_ref()
                    .map(|raw| JsonValue(json!({ "raw": raw }))),
                raw_truncated: deduction.raw_truncated,
                unavailable_reason: deduction.error_message,
            })
        }
        UpravaRef::File {
            placement_id,
            path,
            version,
        }
        | UpravaRef::FileRange {
            placement_id,
            path,
            version,
            ..
        } => {
            let placement = load_placement(state, placement_id).await?;
            Ok(ReferenceResolution {
                reference: reference.clone(),
                status: if placement.state == PlacementState::Missing {
                    ReferenceResolutionStatus::Offline
                } else {
                    ReferenceResolutionStatus::Resolved
                },
                title: path.clone(),
                summary: Some(format!(
                    "Workspace file in {}{}",
                    placement.display_name,
                    version
                        .as_deref()
                        .map(|version| format!(" at {version}"))
                        .unwrap_or_default()
                )),
                links: CausalityLinks {
                    source_refs: vec![UpravaRef::Workspace {
                        placement_id: placement_id.clone(),
                    }],
                    ..CausalityLinks::default()
                },
                raw_payload: None,
                raw_truncated: false,
                unavailable_reason: None,
            })
        }
        UpravaRef::WorkspaceDiff {
            diff_id,
            placement_id,
        } => {
            resolve_workspace_diff_reference(state, reference.clone(), diff_id, placement_id).await
        }
        UpravaRef::DiffHunk { diff_id, hunk_id } => {
            resolve_workspace_diff_hunk_reference(state, reference.clone(), diff_id, hunk_id).await
        }
        UpravaRef::TerminalCommand {
            terminal_command_id,
            ..
        }
        | UpravaRef::TerminalOutputRange {
            terminal_command_id,
            ..
        } => {
            resolve_terminal_command_reference(state, reference.clone(), terminal_command_id).await
        }
        UpravaRef::CheckResult { check_run_id, .. } => {
            resolve_terminal_command_reference(state, reference.clone(), check_run_id).await
        }
        _ => Ok(ReferenceResolution {
            title: reference_title(&reference),
            reference,
            status: ReferenceResolutionStatus::Unsupported,
            summary: None,
            links: CausalityLinks::default(),
            raw_payload: None,
            raw_truncated: false,
            unavailable_reason: Some(
                "This reference type has no Core resolver in the current deployment".to_owned(),
            ),
        }),
    }
}

pub(crate) async fn resolve_message_reference(
    state: &AppState,
    reference: UpravaRef,
    message_id: &MessageId,
) -> Result<ReferenceResolution, AppError> {
    let row = sqlx::query(
        "select session_thread_id, turn_id, role, content, source_event_id from messages where message_id = ?1",
    )
    .bind(message_id.as_str())
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("message.not_found", "Message not found"))?;
    let source_event_id: Option<String> = row.try_get("source_event_id")?;
    let turn_id: Option<String> = row.try_get("turn_id")?;
    let session_thread_id: String = row.try_get("session_thread_id")?;
    let source_refs = source_event_id
        .map(|event_id| {
            vec![UpravaRef::Event {
                event_id: EventId::from(event_id),
                scope_ref: Box::new(ScopeRef::Session {
                    session_thread_id: SessionThreadId::from(session_thread_id),
                }),
                seq: 0,
            }]
        })
        .unwrap_or_default();
    Ok(ReferenceResolution {
        reference,
        status: ReferenceResolutionStatus::Resolved,
        title: format!("{} message", row.try_get::<String, _>("role")?),
        summary: Some(truncate_chars(&row.try_get::<String, _>("content")?, 500)),
        links: CausalityLinks {
            source_refs,
            cause_refs: turn_id
                .map(|turn_id| {
                    vec![UpravaRef::Turn {
                        turn_id: TurnId::from(turn_id),
                    }]
                })
                .unwrap_or_default(),
            ..CausalityLinks::default()
        },
        raw_payload: Some(JsonValue(json!({
            "content": row.try_get::<String, _>("content")?
        }))),
        raw_truncated: false,
        unavailable_reason: None,
    })
}

pub(crate) async fn resolve_command_reference(
    state: &AppState,
    reference: UpravaRef,
    command_id: &CommandId,
) -> Result<ReferenceResolution, AppError> {
    let row = sqlx::query(
        "select state, command_json, result_payload_json from commands where command_id = ?1",
    )
    .bind(command_id.as_str())
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("command.not_found", "Command not found"))?;
    let command: CommandEnvelope =
        serde_json::from_str(&row.try_get::<String, _>("command_json")?)?;
    let result_payload = row
        .try_get::<Option<String>, _>("result_payload_json")?
        .map(|value| serde_json::from_str::<JsonValue>(&value))
        .transpose()?;
    let links = command_result_links(&command, result_payload.as_ref());
    Ok(ReferenceResolution {
        reference,
        status: ReferenceResolutionStatus::Resolved,
        title: format!("{:?}", command.kind),
        summary: Some(format!(
            "Command state: {}",
            row.try_get::<String, _>("state")?
        )),
        links,
        raw_payload: Some(JsonValue(json!({
            "command": command,
            "result": result_payload,
        }))),
        raw_truncated: result_payload
            .as_ref()
            .is_some_and(command_result_is_truncated),
        unavailable_reason: None,
    })
}

pub(crate) async fn resolve_workspace_diff_reference(
    state: &AppState,
    reference: UpravaRef,
    diff_id: &str,
    placement_id: &ProjectPlacementId,
) -> Result<ReferenceResolution, AppError> {
    let rows = sqlx::query(
        "select command_id, result_payload_json from commands where project_placement_id = ?1 and kind = 'ReadWorkspaceDiff' and result_payload_json is not null order by completed_at desc limit 100",
    )
    .bind(placement_id.as_str())
    .fetch_all(&state.pool)
    .await?;
    for row in rows {
        let raw: String = row.try_get("result_payload_json")?;
        let result: WorkspaceDiffResponse = serde_json::from_str(&raw)?;
        if result.diff_id != diff_id {
            continue;
        }
        let command_id = CommandId::from(row.try_get::<String, _>("command_id")?);
        return Ok(ReferenceResolution {
            reference,
            status: ReferenceResolutionStatus::Resolved,
            title: "Workspace diff".to_owned(),
            summary: Some(result.summary.clone()),
            links: CausalityLinks {
                source_refs: vec![UpravaRef::Workspace {
                    placement_id: placement_id.clone(),
                }],
                cause_refs: vec![UpravaRef::Command { command_id }],
                ..CausalityLinks::default()
            },
            raw_payload: Some(JsonValue(json!({ "diff": result.diff }))),
            raw_truncated: result.diff_truncated || result.summary_truncated,
            unavailable_reason: None,
        });
    }
    Ok(raw_only_resolution(
        reference,
        "Workspace diff snapshot is no longer present in bounded command history",
    ))
}

pub(crate) async fn resolve_workspace_diff_hunk_reference(
    state: &AppState,
    reference: UpravaRef,
    diff_id: &str,
    hunk_id: &str,
) -> Result<ReferenceResolution, AppError> {
    let rows = sqlx::query(
        "select command_id, result_payload_json from commands where kind = 'ReadWorkspaceDiff' and result_payload_json is not null order by completed_at desc limit 500",
    )
    .fetch_all(&state.pool)
    .await?;
    for row in rows {
        let raw: String = row.try_get("result_payload_json")?;
        let Ok(result) = serde_json::from_str::<WorkspaceDiffResponse>(&raw) else {
            continue;
        };
        if result.diff_id != diff_id {
            continue;
        }
        let Some(hunk) = result.hunks.iter().find(|hunk| hunk.hunk_id == hunk_id) else {
            continue;
        };
        let command_id = CommandId::from(row.try_get::<String, _>("command_id")?);
        return Ok(ReferenceResolution {
            reference,
            status: ReferenceResolutionStatus::Resolved,
            title: hunk.header.clone(),
            summary: result.path.clone().or_else(|| Some(result.summary.clone())),
            links: CausalityLinks {
                source_refs: vec![UpravaRef::WorkspaceDiff {
                    diff_id: result.diff_id,
                    placement_id: result.placement_id,
                }],
                cause_refs: vec![UpravaRef::Command { command_id }],
                ..CausalityLinks::default()
            },
            raw_payload: Some(JsonValue(json!({ "patch": hunk.patch }))),
            raw_truncated: result.diff_truncated,
            unavailable_reason: None,
        });
    }
    Ok(raw_only_resolution(
        reference,
        "Diff hunk is no longer present in bounded command history",
    ))
}

pub(crate) async fn resolve_terminal_command_reference(
    state: &AppState,
    reference: UpravaRef,
    terminal_command_id: &str,
) -> Result<ReferenceResolution, AppError> {
    let rows = sqlx::query(
        "select command_id, result_payload_json from commands where kind = 'RunWorkspaceCommand' and result_payload_json is not null order by completed_at desc limit 500",
    )
    .fetch_all(&state.pool)
    .await?;
    for row in rows {
        let raw: String = row.try_get("result_payload_json")?;
        let result: WorkspaceCommandRunResponse = serde_json::from_str(&raw)?;
        if result.terminal_command_id != terminal_command_id {
            continue;
        }
        let command_id = CommandId::from(row.try_get::<String, _>("command_id")?);
        return Ok(ReferenceResolution {
            reference,
            status: ReferenceResolutionStatus::Resolved,
            title: result
                .label
                .clone()
                .unwrap_or_else(|| result.command.clone()),
            summary: Some(format!(
                "{} · exit {} · {} ms",
                if result.success {
                    "succeeded"
                } else {
                    "failed"
                },
                result
                    .exit_code
                    .map_or_else(|| "unknown".to_owned(), |code| code.to_string()),
                result.duration_ms
            )),
            links: CausalityLinks {
                source_refs: vec![UpravaRef::Workspace {
                    placement_id: result.placement_id.clone(),
                }],
                evidence_refs: vec![UpravaRef::TerminalOutputRange {
                    terminal_command_id: result.terminal_command_id.clone(),
                    range: TextRange {
                        start_line: None,
                        end_line: None,
                        start_offset: Some(0),
                        end_offset: i64::try_from(result.stdout.len() + result.stderr.len()).ok(),
                    },
                }],
                cause_refs: vec![UpravaRef::Command { command_id }],
                ..CausalityLinks::default()
            },
            raw_payload: Some(JsonValue(json!({
                "stdout": result.stdout,
                "stderr": result.stderr,
            }))),
            raw_truncated: result.stdout_truncated || result.stderr_truncated,
            unavailable_reason: None,
        });
    }
    Ok(raw_only_resolution(
        reference,
        "Terminal command output is no longer present in bounded command history",
    ))
}

pub(crate) fn command_result_links(
    command: &CommandEnvelope,
    result_payload: Option<&JsonValue>,
) -> CausalityLinks {
    let result_refs = result_payload
        .and_then(|payload| result_refs_for_command(command.kind, payload))
        .unwrap_or_default();
    CausalityLinks {
        source_refs: command.source_refs.clone(),
        cause_refs: command.cause_refs.clone(),
        result_refs,
        ..CausalityLinks::default()
    }
}

pub(crate) fn result_refs_for_command(
    kind: CommandKind,
    payload: &JsonValue,
) -> Option<Vec<UpravaRef>> {
    match kind {
        CommandKind::WriteWorkspaceFile => {
            let result: WorkspaceFileWriteResponse =
                serde_json::from_value(payload.0.clone()).ok()?;
            Some(vec![
                UpravaRef::WorkspaceEdit {
                    edit_id: result.edit_id,
                    placement_id: Some(result.placement_id.clone()),
                    path: Some(result.path.clone()),
                },
                UpravaRef::File {
                    placement_id: result.placement_id,
                    path: result.path,
                    version: None,
                },
            ])
        }
        CommandKind::RunWorkspaceCommand => {
            let result: WorkspaceCommandRunResponse =
                serde_json::from_value(payload.0.clone()).ok()?;
            let mut refs = vec![UpravaRef::TerminalCommand {
                terminal_command_id: result.terminal_command_id.clone(),
                terminal_id: None,
            }];
            if result.intent == uprava_protocol::WorkspaceCommandIntent::Check {
                refs.push(UpravaRef::CheckResult {
                    check_run_id: result.terminal_command_id,
                    failure_id: (!result.success).then(|| "command_failed".to_owned()),
                });
            }
            Some(refs)
        }
        CommandKind::ReadWorkspaceDiff => {
            let result: WorkspaceDiffResponse = serde_json::from_value(payload.0.clone()).ok()?;
            let diff_id = result.diff_id;
            let mut refs = vec![UpravaRef::WorkspaceDiff {
                diff_id: diff_id.clone(),
                placement_id: result.placement_id,
            }];
            refs.extend(result.hunks.into_iter().map(|hunk| UpravaRef::DiffHunk {
                diff_id: diff_id.clone(),
                hunk_id: hunk.hunk_id,
            }));
            Some(refs)
        }
        _ => None,
    }
}

pub(crate) fn event_payload_is_truncated(event: &EventEnvelope) -> bool {
    [
        "raw_event_truncated",
        "stdout_truncated",
        "stderr_truncated",
        "diff_truncated",
        "summary_truncated",
    ]
    .iter()
    .any(|key| {
        event
            .payload
            .0
            .get(key)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    })
}

pub(crate) fn command_result_is_truncated(payload: &JsonValue) -> bool {
    [
        "stdout_truncated",
        "stderr_truncated",
        "diff_truncated",
        "summary_truncated",
    ]
    .iter()
    .any(|key| {
        payload
            .0
            .get(key)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    })
}

pub(crate) fn deduction_links(block: &DeductionBlock) -> CausalityLinks {
    CausalityLinks {
        evidence_refs: block
            .result
            .steps
            .iter()
            .flat_map(|step| step.support_refs.iter().cloned())
            .collect(),
        cause_refs: vec![block.scope_ref.clone()],
        ..CausalityLinks::default()
    }
}

pub(crate) fn raw_only_resolution(reference: UpravaRef, reason: &str) -> ReferenceResolution {
    ReferenceResolution {
        title: reference_title(&reference),
        reference,
        status: ReferenceResolutionStatus::RawOnly,
        summary: None,
        links: CausalityLinks::default(),
        raw_payload: None,
        raw_truncated: false,
        unavailable_reason: Some(reason.to_owned()),
    }
}

pub(crate) fn missing_resolution(reference: UpravaRef, reason: &str) -> ReferenceResolution {
    ReferenceResolution {
        title: reference_title(&reference),
        reference,
        status: ReferenceResolutionStatus::Missing,
        summary: None,
        links: CausalityLinks::default(),
        raw_payload: None,
        raw_truncated: false,
        unavailable_reason: Some(reason.to_owned()),
    }
}

pub(crate) fn reference_title(reference: &UpravaRef) -> String {
    match reference {
        UpravaRef::Node { node_id } => format!("Node {node_id}"),
        UpravaRef::Project { project_id } => format!("Project {project_id}"),
        UpravaRef::Placement { placement_id } | UpravaRef::Workspace { placement_id } => {
            format!("Workspace {placement_id}")
        }
        UpravaRef::Session { session_thread_id } => format!("Session {session_thread_id}"),
        UpravaRef::Runtime { runtime_session_id } => format!("Runtime {runtime_session_id}"),
        UpravaRef::RuntimeAttempt { runtime_attempt_id } => {
            format!("Runtime attempt {runtime_attempt_id}")
        }
        UpravaRef::ProviderInteraction {
            provider_interaction_id,
        } => format!("Provider interaction {provider_interaction_id}"),
        UpravaRef::TaskRun { task_run_id } => format!("Task run {task_run_id}"),
        UpravaRef::Turn { turn_id } => format!("Turn {turn_id}"),
        UpravaRef::Message { message_id } | UpravaRef::MessageRange { message_id, .. } => {
            format!("Message {message_id}")
        }
        UpravaRef::Block { block_id } => format!("Block {block_id}"),
        UpravaRef::Artifact { artifact_id } => format!("Artifact {artifact_id}"),
        UpravaRef::ArtifactVersion {
            artifact_id,
            version,
        } => format!("Artifact {artifact_id} version {version}"),
        UpravaRef::Deduction { deduction_id } => format!("Deduction {deduction_id}"),
        UpravaRef::Event { event_id, .. } => format!("Event {event_id}"),
        UpravaRef::Command { command_id } => format!("Command {command_id}"),
        UpravaRef::Approval { approval_id } => format!("Approval {approval_id}"),
        UpravaRef::Warning { warning_kind, .. } => format!("Warning {warning_kind}"),
        UpravaRef::ToolCall { tool_call_id } => format!("Tool call {tool_call_id}"),
        UpravaRef::File { path, .. } | UpravaRef::FileRange { path, .. } => path.clone(),
        UpravaRef::Terminal { terminal_id, .. } => format!("Terminal {terminal_id}"),
        UpravaRef::TerminalCommand {
            terminal_command_id,
            ..
        }
        | UpravaRef::TerminalOutputRange {
            terminal_command_id,
            ..
        } => format!("Terminal command {terminal_command_id}"),
        UpravaRef::DiffHunk { diff_id, hunk_id } => format!("Diff {diff_id} hunk {hunk_id}"),
        UpravaRef::WorkspaceDiff { diff_id, .. } => format!("Workspace diff {diff_id}"),
        UpravaRef::CheckResult { check_run_id, .. } => format!("Check {check_run_id}"),
        UpravaRef::WorkspaceEdit { edit_id, .. } => format!("Workspace edit {edit_id}"),
        UpravaRef::TraceEvent { trace_event_id } => format!("Trace event {trace_event_id}"),
        UpravaRef::ExternalEntity { external_id, .. } => format!("External entity {external_id}"),
        UpravaRef::Unknown { ref_type, .. } => format!("Unknown {ref_type}"),
    }
}

pub(crate) async fn build_session_evidence_projection(
    state: &AppState,
    session_id: &SessionThreadId,
) -> Result<SessionEvidenceProjection, AppError> {
    let detail = load_session_detail(state, session_id).await?;
    let mut children = Vec::with_capacity(detail.messages.len() + detail.events.len());
    for message in &detail.messages {
        children.push(SessionEvidenceProjectionNode {
            evidence_id: EvidenceId::from(format!("message:{}", message.message_id)),
            label: format!("{:?} message", message.role),
            primary_ref: UpravaRef::Message {
                message_id: message.message_id.clone(),
            },
            source_refs: message_source_refs(message, &detail.events),
            evidence_refs: vec![],
            cause_refs: vec![],
            children: vec![],
        });
    }
    for event in &detail.events {
        children.push(SessionEvidenceProjectionNode {
            evidence_id: EvidenceId::from(format!("event:{}", event.event_id)),
            label: artifact_label_for_event(event),
            primary_ref: primary_ref_for_event(event),
            source_refs: event.source_refs.clone(),
            evidence_refs: event.evidence_refs.clone(),
            cause_refs: event.cause_refs.clone(),
            children: vec![],
        });
    }

    Ok(SessionEvidenceProjection {
        session_thread_id: session_id.clone(),
        root: SessionEvidenceProjectionNode {
            evidence_id: EvidenceId::from(format!("session:{session_id}")),
            label: detail.session.title,
            primary_ref: UpravaRef::Session {
                session_thread_id: session_id.clone(),
            },
            source_refs: vec![],
            evidence_refs: vec![],
            cause_refs: vec![],
            children,
        },
        generated_at: Utc::now(),
    })
}

pub(crate) async fn build_agent_projection(
    state: &AppState,
    session_id: &SessionThreadId,
) -> Result<AgentProjection, AppError> {
    let detail = load_session_detail(state, session_id).await?;
    let node_presence = effective_node_presence(state, &detail.placement.node_id).await?;
    let provider_available = node_supports_provider(
        state,
        &detail.placement.node_id,
        &detail.session.runtime.provider,
    )
    .await?;
    let current_turn = current_turn(&detail.events);
    let pending_approvals = pending_approvals(&detail.events);
    let acknowledged = acknowledged_warning_kinds(state, session_id).await?;
    let mut active_warnings =
        active_warnings(&detail.placement, &detail.session.runtime, &acknowledged);
    if let Some(warning) = node_presence_warning(node_presence) {
        if !acknowledged.contains(&warning.kind) {
            active_warnings.push(warning);
        }
    }
    if !provider_available && !acknowledged.contains("provider_unavailable") {
        active_warnings.push(ResourceBadge {
            kind: "provider_unavailable".to_owned(),
            severity: WarningSeverity::HardBlock,
            label: format!(
                "Provider `{}` is not advertised by this node",
                detail.session.runtime.provider
            ),
        });
    }
    let recent_turn_summaries = recent_turn_summaries(&detail.messages);
    let recent_message_refs = detail
        .messages
        .iter()
        .rev()
        .take(5)
        .map(|message| UpravaRef::Message {
            message_id: message.message_id.clone(),
        })
        .collect::<Vec<_>>();
    let visible_refs = visible_refs(
        &detail,
        &pending_approvals,
        &active_warnings,
        &recent_message_refs,
    );
    let source_cause_summary = source_cause_summary(&detail.events);
    let resume_context = resume_context(
        &detail,
        current_turn.as_ref(),
        &pending_approvals,
        &recent_turn_summaries,
    );
    let evidence_projection_summary = format!(
        "Session evidence projection: {} messages, {} events, {} pending approvals",
        detail.messages.len(),
        detail.events.len(),
        pending_approvals.len()
    );
    let available_commands = available_commands(
        detail.session.state,
        detail.session.runtime.state,
        PendingInteractionCapabilities {
            approval: !pending_approvals.is_empty(),
            user_input: detail
                .pending_interactions
                .iter()
                .any(|interaction| interaction.kind == ProviderInteractionKind::UserInput),
        },
        !active_warnings.is_empty(),
        node_accepts_commands(node_presence),
        placement_has_hard_block(&detail.placement),
        provider_available,
    );
    Ok(AgentProjection {
        session_thread_id: session_id.clone(),
        project_placement: detail.placement,
        runtime_summary: detail.session.runtime,
        current_turn,
        pending_approvals,
        pending_interactions: detail.pending_interactions,
        active_warnings,
        recent_turn_summaries,
        recent_message_refs,
        evidence_projection_summary,
        available_block_types: vec![
            "core.user-message".to_owned(),
            "core.assistant-message".to_owned(),
            "core.provider-output-stream".to_owned(),
            "core.approval-request".to_owned(),
            "core.runtime-event".to_owned(),
            "core.workspace-validation".to_owned(),
            "core.resource-snapshot".to_owned(),
            "core.warning".to_owned(),
            "core.error".to_owned(),
            "core.agent-projection-summary".to_owned(),
            "core.unknown".to_owned(),
        ],
        available_commands,
        visible_refs,
        source_cause_summary,
        resume_context,
        generated_at: Utc::now(),
    })
}

pub(crate) fn message_source_refs(message: &Message, events: &[EventEnvelope]) -> Vec<UpravaRef> {
    let Some(source_event_id) = &message.source_event_id else {
        return vec![];
    };
    events
        .iter()
        .find(|event| event.event_id == *source_event_id)
        .map(|event| {
            vec![UpravaRef::Event {
                event_id: event.event_id.clone(),
                scope_ref: Box::new(event.scope_ref.clone()),
                seq: event.seq,
            }]
        })
        .unwrap_or_else(|| {
            vec![UpravaRef::Event {
                event_id: source_event_id.clone(),
                scope_ref: Box::new(ScopeRef::Session {
                    session_thread_id: message.session_thread_id.clone(),
                }),
                seq: 0,
            }]
        })
}

pub(crate) fn artifact_label_for_event(event: &EventEnvelope) -> String {
    match event.kind {
        EventKind::ApprovalRequested => {
            let prompt = event
                .payload
                .0
                .get("prompt")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("approval requested");
            format!("Approval requested: {}", snippet(prompt, 80))
        }
        EventKind::ApprovalResolved => "Approval resolved".to_owned(),
        EventKind::RuntimeError => {
            let message = event
                .payload
                .0
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("runtime error");
            format!("Runtime error: {}", snippet(message, 80))
        }
        _ => format!("{:?} #{}", event.kind, event.seq),
    }
}

pub(crate) fn primary_ref_for_event(event: &EventEnvelope) -> UpravaRef {
    if matches!(
        event.kind,
        EventKind::ApprovalRequested | EventKind::ApprovalResolved
    ) {
        if let Some(approval_id) = event_approval_id(event) {
            return UpravaRef::Approval { approval_id };
        }
    }
    UpravaRef::Event {
        event_id: event.event_id.clone(),
        scope_ref: Box::new(event.scope_ref.clone()),
        seq: event.seq,
    }
}

pub(crate) fn current_turn(events: &[EventEnvelope]) -> Option<TurnId> {
    let mut active_turns = Vec::<TurnId>::new();
    for event in events {
        match event.kind {
            EventKind::TurnStarted => {
                if let Some(turn_id) = &event.turn_id {
                    active_turns.retain(|candidate| candidate != turn_id);
                    active_turns.push(turn_id.clone());
                }
            }
            EventKind::TurnCompleted | EventKind::TurnInterrupted | EventKind::RuntimeError => {
                if let Some(turn_id) = &event.turn_id {
                    active_turns.retain(|candidate| candidate != turn_id);
                }
            }
            _ => {}
        }
    }
    active_turns.last().cloned()
}

pub(crate) fn pending_approvals(events: &[EventEnvelope]) -> Vec<ApprovalId> {
    let mut pending = Vec::<ApprovalId>::new();
    for event in events {
        match event.kind {
            EventKind::ApprovalRequested => {
                if let Some(approval_id) = event_approval_id(event) {
                    pending.retain(|candidate| candidate != &approval_id);
                    pending.push(approval_id);
                }
            }
            EventKind::ApprovalResolved => {
                if let Some(approval_id) = event_approval_id(event) {
                    pending.retain(|candidate| candidate != &approval_id);
                }
            }
            _ => {}
        }
    }
    pending
}

pub(crate) fn event_approval_id(event: &EventEnvelope) -> Option<ApprovalId> {
    event
        .payload
        .0
        .get("approval_id")
        .and_then(serde_json::Value::as_str)
        .filter(|approval_id| !approval_id.is_empty())
        .map(ApprovalId::from)
}

pub(crate) fn active_warnings(
    placement: &ProjectPlacementSummary,
    runtime: &RuntimeSummary,
    acknowledged: &HashSet<String>,
) -> Vec<ResourceBadge> {
    let mut warnings = placement
        .resource_badges
        .iter()
        .filter(|badge| badge.severity != WarningSeverity::Info)
        .filter(|badge| !acknowledged.contains(&badge.kind))
        .cloned()
        .collect::<Vec<_>>();
    if let Some(reason) = &runtime.degraded_reason {
        if !acknowledged.contains("runtime_degraded") {
            warnings.push(ResourceBadge {
                kind: "runtime_degraded".to_owned(),
                severity: WarningSeverity::Warning,
                label: reason.clone(),
            });
        }
    }
    warnings
}

pub(crate) fn node_presence_warning(presence: NodePresence) -> Option<ResourceBadge> {
    match presence {
        NodePresence::Reachable => None,
        NodePresence::Stale => Some(ResourceBadge {
            kind: "node_stale".to_owned(),
            severity: WarningSeverity::Warning,
            label: "Node heartbeat is stale".to_owned(),
        }),
        NodePresence::Offline => Some(ResourceBadge {
            kind: "node_offline".to_owned(),
            severity: WarningSeverity::HardBlock,
            label: "Node is offline".to_owned(),
        }),
        NodePresence::Revoked => Some(ResourceBadge {
            kind: "node_revoked".to_owned(),
            severity: WarningSeverity::HardBlock,
            label: "Node is revoked".to_owned(),
        }),
    }
}

pub(crate) fn node_accepts_commands(presence: NodePresence) -> bool {
    matches!(presence, NodePresence::Reachable | NodePresence::Stale)
}

pub(crate) async fn acknowledged_warning_kinds(
    state: &AppState,
    session_id: &SessionThreadId,
) -> Result<HashSet<String>, AppError> {
    let rows = sqlx::query_scalar::<_, String>(
        "select warning_kind from warning_acknowledgements where session_thread_id = ?1",
    )
    .bind(session_id.as_str())
    .fetch_all(&state.pool)
    .await?;
    Ok(rows.into_iter().collect())
}

pub(crate) fn recent_turn_summaries(messages: &[Message]) -> Vec<String> {
    messages
        .iter()
        .rev()
        .filter(|message| {
            matches!(
                message.role,
                MessageRole::User
                    | MessageRole::Assistant
                    | MessageRole::Approval
                    | MessageRole::Runtime
            )
        })
        .take(6)
        .map(|message| format!("{:?}: {}", message.role, snippet(&message.content, 140)))
        .collect()
}

pub(crate) fn visible_refs(
    detail: &SessionDetail,
    pending_approvals: &[ApprovalId],
    active_warnings: &[ResourceBadge],
    recent_message_refs: &[UpravaRef],
) -> Vec<UpravaRef> {
    let mut refs = vec![
        UpravaRef::Session {
            session_thread_id: detail.session.session_thread_id.clone(),
        },
        UpravaRef::Runtime {
            runtime_session_id: detail.session.runtime.runtime_session_id.clone(),
        },
        UpravaRef::Placement {
            placement_id: detail.placement.project_placement_id.clone(),
        },
    ];
    refs.extend(recent_message_refs.iter().cloned());
    refs.extend(
        detail
            .events
            .iter()
            .rev()
            .take(5)
            .map(|event| UpravaRef::Event {
                event_id: event.event_id.clone(),
                scope_ref: Box::new(event.scope_ref.clone()),
                seq: event.seq,
            }),
    );
    refs.extend(
        pending_approvals
            .iter()
            .cloned()
            .map(|approval_id| UpravaRef::Approval { approval_id }),
    );
    refs.extend(active_warnings.iter().map(|warning| UpravaRef::Warning {
        warning_kind: warning.kind.clone(),
        command_id: None,
    }));
    dedupe_refs(refs)
}

pub(crate) fn dedupe_refs(refs: Vec<UpravaRef>) -> Vec<UpravaRef> {
    let mut seen = HashSet::<String>::new();
    refs.into_iter()
        .filter(|reference| seen.insert(ref_key(reference)))
        .collect()
}

pub(crate) fn ref_key(reference: &UpravaRef) -> String {
    serde_json::to_string(reference).unwrap_or_else(|_| format!("{reference:?}"))
}

pub(crate) fn source_cause_summary(events: &[EventEnvelope]) -> String {
    let source_count = events
        .iter()
        .filter(|event| !event.source_refs.is_empty())
        .count();
    let evidence_count = events
        .iter()
        .filter(|event| !event.evidence_refs.is_empty())
        .count();
    let cause_count = events
        .iter()
        .filter(|event| !event.cause_refs.is_empty())
        .count();
    format!(
        "{} events; {source_count} with source refs, {evidence_count} with evidence refs, {cause_count} with cause refs. Missing causality remains explicit.",
        events.len()
    )
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PendingInteractionCapabilities {
    approval: bool,
    user_input: bool,
}

pub(crate) fn available_commands(
    session_state: SessionThreadState,
    runtime_state: RuntimeSessionState,
    pending_interactions: PendingInteractionCapabilities,
    has_active_warnings: bool,
    node_accepts_commands: bool,
    placement_has_hard_block: bool,
    provider_available: bool,
) -> Vec<ActionCapability> {
    let mut commands = Vec::new();
    let session_is_attached = session_state != SessionThreadState::Detached;
    if session_state == SessionThreadState::Detached {
        commands.push(ActionCapability::SessionAttach);
    } else if session_state != SessionThreadState::Stopped {
        commands.push(ActionCapability::SessionDetach);
    }
    let can_start_or_continue_runtime =
        node_accepts_commands && !placement_has_hard_block && provider_available;
    if matches!(
        runtime_state,
        RuntimeSessionState::Ready | RuntimeSessionState::Running
    ) && can_start_or_continue_runtime
        && session_is_attached
    {
        commands.push(ActionCapability::SessionSendTurn);
    }
    if matches!(
        runtime_state,
        RuntimeSessionState::Running | RuntimeSessionState::Blocked
    ) && node_accepts_commands
    {
        commands.push(ActionCapability::RuntimeInterrupt);
    }
    if !matches!(
        runtime_state,
        RuntimeSessionState::Stopped | RuntimeSessionState::Expired
    ) && node_accepts_commands
    {
        commands.push(ActionCapability::RuntimeStop);
    }
    if matches!(
        runtime_state,
        RuntimeSessionState::Stopped
            | RuntimeSessionState::Expired
            | RuntimeSessionState::Stale
            | RuntimeSessionState::Error
            | RuntimeSessionState::Interrupted
    ) && can_start_or_continue_runtime
    {
        commands.push(ActionCapability::RuntimeResume);
    }
    if pending_interactions.approval
        && runtime_state == RuntimeSessionState::Blocked
        && node_accepts_commands
        && provider_available
        && session_is_attached
    {
        commands.push(ActionCapability::ApprovalResolve);
    }
    if pending_interactions.user_input
        && runtime_state == RuntimeSessionState::Blocked
        && node_accepts_commands
        && provider_available
        && session_is_attached
    {
        commands.push(ActionCapability::ProviderInteractionSubmitInput);
    }
    if has_active_warnings {
        commands.push(ActionCapability::WarningAcknowledge);
    }
    if matches!(
        runtime_state,
        RuntimeSessionState::Ready | RuntimeSessionState::Running
    ) && can_start_or_continue_runtime
    {
        commands.push(ActionCapability::DeductionRequest);
    }
    commands.push(ActionCapability::ReferenceOpenInInspector);
    commands.push(ActionCapability::ReferenceCopy);
    commands
}

pub(crate) fn resume_context(
    detail: &SessionDetail,
    current_turn: Option<&TurnId>,
    pending_approvals: &[ApprovalId],
    recent_turn_summaries: &[String],
) -> String {
    let mut parts = vec![
        format!("runtime_state={:?}", detail.session.runtime.state),
        format!("provider={}", detail.session.runtime.provider),
    ];
    if let Some(turn_id) = current_turn {
        parts.push(format!("current_turn={turn_id}"));
    }
    if !pending_approvals.is_empty() {
        parts.push(format!(
            "pending_approvals={}",
            pending_approvals
                .iter()
                .map(ApprovalId::as_str)
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    if let Some(reason) = &detail.session.runtime.degraded_reason {
        parts.push(format!("degraded_reason={}", snippet(reason, 160)));
    }
    if !recent_turn_summaries.is_empty() {
        parts.push(format!(
            "recent={}",
            recent_turn_summaries
                .iter()
                .map(|summary| snippet(summary, 100))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    parts.join("; ")
}

pub(crate) fn snippet(value: &str, max_chars: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    let mut result = normalized.chars().take(max_chars).collect::<String>();
    result.push_str("...");
    result
}

pub(crate) async fn upsert_actor_on_connection(
    connection: &mut SqliteConnection,
    actor_ref: &ActorRef,
    seen_at: DateTime<Utc>,
) -> Result<(), AppError> {
    let (actor_key, actor_kind, display_name) = actor_identity(actor_ref);
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
    .bind(serde_json::to_string(actor_ref)?)
    .bind(seen_at)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

pub(crate) fn actor_identity(actor_ref: &ActorRef) -> (String, &'static str, String) {
    match actor_ref {
        ActorRef::LocalUser { actor_id } => {
            let actor_key = actor_id
                .as_ref()
                .map(|actor_id| format!("local_user:{actor_id}"))
                .unwrap_or_else(|| "local_user".to_owned());
            (actor_key, "local_user", "Local user".to_owned())
        }
        ActorRef::System => ("system".to_owned(), "system", "System".to_owned()),
        ActorRef::Node { node_id } => {
            (format!("node:{node_id}"), "node", format!("Node {node_id}"))
        }
        ActorRef::Provider { provider } => (
            format!("provider:{provider}"),
            "provider",
            format!("Provider {provider}"),
        ),
        ActorRef::Unknown => ("unknown".to_owned(), "unknown", "Unknown".to_owned()),
    }
}
