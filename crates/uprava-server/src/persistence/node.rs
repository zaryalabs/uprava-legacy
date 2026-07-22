//! Node enrollment, heartbeat, inventory ownership and placement persistence.

use super::super::*;

pub(crate) async fn create_enrollment(
    state: &AppState,
    display_name: &str,
    daemon_version: Option<&str>,
    capabilities: Vec<CapabilitySummary>,
) -> Result<NodeEnrollmentRequestedResponse, AppError> {
    if display_name.trim().is_empty() {
        return Err(AppError::bad_request(
            "validation.display_name_required",
            "Display name is required",
        ));
    }

    let _create_guard = state.enrollment_create_lock.lock().await;
    let now = Utc::now();
    sqlx::query(
        "update node_enrollments set status = 'expired', updated_at = ?1 where status in ('pending_user_approval', 'approved') and expires_at <= ?1",
    )
    .bind(now)
    .execute(&state.pool)
    .await?;
    let pending_count: i64 = sqlx::query_scalar(
        "select count(*) from node_enrollments where status in ('pending_user_approval', 'approved') and claimed_node_id is null and expires_at > ?1",
    )
    .bind(now)
    .fetch_one(&state.pool)
    .await?;
    if pending_count >= state.config.max_pending_enrollments.max(1) {
        return Err(AppError::rate_limited(
            "node_enrollment.pending_limit",
            "Too many pending enrollment requests",
        ));
    }
    let enrollment_id = EnrollmentId::new();
    let pairing_code = new_secret("pair");
    let expires_at = now + chrono::Duration::seconds(state.config.enrollment_ttl_seconds);
    let normalized_display_name = display_name.trim();
    let existing_identity_count: i64 = sqlx::query_scalar(
        r#"
        select
            (select count(*) from nodes
             where display_name = ?1 and presence != 'revoked')
          + (select count(*) from node_enrollments
             where display_name = ?1
               and status in ('pending_user_approval', 'approved')
               and claimed_node_id is null
               and expires_at > ?2)
        "#,
    )
    .bind(normalized_display_name)
    .bind(now)
    .fetch_one(&state.pool)
    .await?;
    let scoped_auto_approval = state.config.auto_approve_node_name.as_deref()
        == Some(normalized_display_name)
        && existing_identity_count == 0;
    let approved_at =
        (state.config.auto_approve_enrollments || scoped_auto_approval).then_some(now);
    let status = if approved_at.is_some() {
        EnrollmentState::Approved
    } else {
        EnrollmentState::PendingUserApproval
    };
    sqlx::query(
        r#"
        insert into node_enrollments (
            enrollment_id, display_name, daemon_version, capabilities_json,
            pairing_code_hash, status, expires_at, claimed_node_id,
            created_at, updated_at, approved_at
        )
        values (?1, ?2, ?3, ?4, ?5, ?6, ?7, null, ?8, ?8, ?9)
        "#,
    )
    .bind(enrollment_id.as_str())
    .bind(normalized_display_name)
    .bind(daemon_version)
    .bind(serde_json::to_string(&capabilities)?)
    .bind(hash_secret(&pairing_code))
    .bind(format_enrollment_state(&status))
    .bind(expires_at)
    .bind(now)
    .bind(approved_at)
    .execute(&state.pool)
    .await?;
    tracing::info!(
        daemon_version = daemon_version.unwrap_or("client"),
        capabilities = capabilities.len(),
        auto_approved = approved_at.is_some(),
        expires_at = %expires_at,
        "node enrollment created"
    );
    audit_security_event(
        state,
        "node.enrollment.created",
        None,
        None,
        "accepted",
        JsonValue(json!({
            "enrollment_id": enrollment_id,
            "auto_approved": approved_at.is_some(),
        })),
    )
    .await?;

    Ok(NodeEnrollmentRequestedResponse {
        enrollment_id,
        pairing_code,
        status,
        expires_at,
    })
}

pub(crate) async fn claim_enrollment(
    state: &AppState,
    request: &NodeEnrollmentClaimRequest,
) -> Result<NodeEnrollmentClaimResponse, AppError> {
    let row = sqlx::query(
        r#"
        select enrollment_id, display_name, daemon_version, capabilities_json,
               pairing_code_hash, status, expires_at, claimed_node_id,
               approved_at, created_at
        from node_enrollments
        where enrollment_id = ?1
        "#,
    )
    .bind(request.enrollment_id.as_str())
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("node_enrollment.not_found", "Enrollment not found"))?;

    let now = Utc::now();
    let status = parse_enrollment_state(row.try_get::<String, _>("status")?.as_str());
    let expires_at: DateTime<Utc> = row.try_get("expires_at")?;
    if expires_at <= now {
        expire_enrollment(state, &request.enrollment_id, now).await?;
        tracing::warn!("node enrollment claim rejected because enrollment expired");
        return Ok(NodeEnrollmentClaimResponse {
            accepted: false,
            pending: false,
            node_id: None,
            credential: None,
            message: "Enrollment expired".to_owned(),
        });
    }
    let stored_pairing_hash: String = row.try_get("pairing_code_hash")?;
    if !constant_time_eq(
        stored_pairing_hash.as_bytes(),
        hash_secret(&request.pairing_code).as_bytes(),
    ) {
        tracing::warn!("node enrollment claim rejected because pairing code was invalid");
        audit_security_event(
            state,
            "node.enrollment.claim_rejected",
            None,
            None,
            "rejected",
            JsonValue(json!({
                "enrollment_id": request.enrollment_id,
                "reason": "invalid_pairing_code",
            })),
        )
        .await?;
        return Err(AppError::auth(
            "auth_dev.invalid_pairing_code",
            "Pairing code is invalid",
        ));
    }
    let approved_at: Option<DateTime<Utc>> = row.try_get("approved_at")?;
    if status == EnrollmentState::Registered {
        let claimed_node_id: Option<String> = row.try_get("claimed_node_id")?;
        tracing::info!("node enrollment claim replayed after registration");
        return Ok(NodeEnrollmentClaimResponse {
            accepted: true,
            pending: false,
            node_id: claimed_node_id.map(NodeId::from),
            credential: None,
            message: "Enrollment already claimed; existing credential is not returned".to_owned(),
        });
    }
    if matches!(
        status,
        EnrollmentState::Expired | EnrollmentState::Rejected | EnrollmentState::Revoked
    ) {
        tracing::warn!(
            status = ?status,
            "node enrollment claim rejected because enrollment is terminal"
        );
        return Ok(NodeEnrollmentClaimResponse {
            accepted: false,
            pending: false,
            node_id: None,
            credential: None,
            message: format!("Enrollment is {}", format_enrollment_state(&status)),
        });
    }
    let approved = approved_at.is_some()
        || status == EnrollmentState::Approved
        || state.config.auto_approve_enrollments;
    if !approved {
        tracing::debug!("node enrollment claim waiting for user approval");
        return Ok(NodeEnrollmentClaimResponse {
            accepted: false,
            pending: true,
            node_id: None,
            credential: None,
            message: "Enrollment is waiting for approval".to_owned(),
        });
    }

    let node_id = NodeId::new();
    let credential = new_secret("node");
    let credential_hash = hash_secret(&credential);
    let display_name: String = row.try_get("display_name")?;
    let daemon_version: Option<String> = row.try_get("daemon_version")?;
    let capabilities_json: String = row.try_get("capabilities_json")?;
    let capabilities = serde_json::from_str::<Vec<CapabilitySummary>>(&capabilities_json)?;

    let mut claim_transaction = state.pool.begin().await?;
    sqlx::query(
        r#"
        insert into nodes (
            node_id, display_name, presence, sleep_hint, last_heartbeat_at,
            daemon_version, active_runtime_count, capabilities_json, diagnostics,
            credential_hash, created_at, updated_at
        )
        values (?1, ?2, 'offline', 'unknown', null, ?3, 0, ?4, 'enrolled; waiting for heartbeat', ?5, ?6, ?6)
        "#,
    )
    .bind(node_id.as_str())
    .bind(display_name)
    .bind(daemon_version.unwrap_or_else(|| "unknown".to_owned()))
    .bind(capabilities_json)
    .bind(credential_hash)
    .bind(now)
    .execute(&mut *claim_transaction)
    .await?;

    sqlx::query("delete from node_capabilities where node_id = ?1")
        .bind(node_id.as_str())
        .execute(&mut *claim_transaction)
        .await?;
    for capability in &capabilities {
        sqlx::query(
            r#"
            insert into node_capabilities (
                node_id, capability_key, value_json, updated_at
            ) values (?1, ?2, ?3, ?4)
            "#,
        )
        .bind(node_id.as_str())
        .bind(&capability.key)
        .bind(serde_json::to_string(&capability.value)?)
        .bind(now)
        .execute(&mut *claim_transaction)
        .await?;
    }

    let updated = sqlx::query(
        r#"
        update node_enrollments
        set status = 'registered', claimed_node_id = ?1, updated_at = ?2
        where enrollment_id = ?3
          and claimed_node_id is null
          and status in ('pending_user_approval', 'approved')
          and expires_at > ?4
        "#,
    )
    .bind(node_id.as_str())
    .bind(now)
    .bind(request.enrollment_id.as_str())
    .bind(now)
    .execute(&mut *claim_transaction)
    .await?;
    if updated.rows_affected() == 0 {
        claim_transaction.rollback().await?;
        let current: Option<(String, Option<String>, DateTime<Utc>)> = sqlx::query_as(
            "select status, claimed_node_id, expires_at from node_enrollments where enrollment_id = ?1",
        )
        .bind(request.enrollment_id.as_str())
        .fetch_optional(&state.pool)
        .await?
        ;
        let Some((current_status, claimed_node_id, current_expires_at)) = current else {
            return Err(AppError::not_found(
                "node_enrollment.not_found",
                "Enrollment not found",
            ));
        };
        if current_expires_at <= now {
            return Ok(NodeEnrollmentClaimResponse {
                accepted: false,
                pending: false,
                node_id: None,
                credential: None,
                message: "Enrollment expired".to_owned(),
            });
        }
        return Ok(NodeEnrollmentClaimResponse {
            accepted: claimed_node_id.is_some(),
            pending: current_status == "pending_user_approval",
            node_id: claimed_node_id.map(NodeId::from),
            credential: None,
            message: "Enrollment already claimed; existing credential is not returned".to_owned(),
        });
    }
    sqlx::query(
        r#"
        insert into security_audit_events (
            audit_event_id, kind, node_id, origin, outcome, metadata_json, happened_at
        ) values (?1, ?2, ?3, null, ?4, ?5, ?6)
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind("node.enrollment.claimed")
    .bind(node_id.as_str())
    .bind("accepted")
    .bind(serde_json::to_string(&json!({
        "enrollment_id": request.enrollment_id,
    }))?)
    .bind(now)
    .execute(&mut *claim_transaction)
    .await?;
    claim_transaction.commit().await?;
    tracing::info!("node enrollment claimed");
    Ok(NodeEnrollmentClaimResponse {
        accepted: true,
        pending: false,
        node_id: Some(node_id),
        credential: Some(credential),
        message: "Enrollment claimed".to_owned(),
    })
}

pub(crate) async fn expire_enrollment(
    state: &AppState,
    enrollment_id: &EnrollmentId,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        update node_enrollments
        set status = 'expired', updated_at = ?1
        where enrollment_id = ?2 and status in ('pending_user_approval', 'approved')
        "#,
    )
    .bind(now)
    .bind(enrollment_id.as_str())
    .execute(&state.pool)
    .await?;
    Ok(())
}

pub(crate) async fn load_enrollments(
    state: &AppState,
) -> Result<Vec<NodeEnrollmentSummary>, AppError> {
    let rows = sqlx::query(
        r#"
        select enrollment_id, display_name, status, claimed_node_id,
               expires_at, created_at, approved_at
        from node_enrollments
        order by created_at desc
        "#,
    )
    .fetch_all(&state.pool)
    .await?;
    rows.into_iter().map(row_to_enrollment).collect()
}

pub(crate) async fn load_enrollment(
    state: &AppState,
    enrollment_id: &EnrollmentId,
) -> Result<NodeEnrollmentSummary, AppError> {
    let row = sqlx::query(
        r#"
        select enrollment_id, display_name, status, claimed_node_id,
               expires_at, created_at, approved_at
        from node_enrollments
        where enrollment_id = ?1
        "#,
    )
    .bind(enrollment_id.as_str())
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("node_enrollment.not_found", "Enrollment not found"))?;
    row_to_enrollment(row)
}

pub(crate) fn row_to_enrollment(
    row: sqlx::sqlite::SqliteRow,
) -> Result<NodeEnrollmentSummary, AppError> {
    let claimed_node_id: Option<String> = row.try_get("claimed_node_id")?;
    let approved_at: Option<DateTime<Utc>> = row.try_get("approved_at")?;
    let mut status = parse_enrollment_state(row.try_get::<String, _>("status")?.as_str());
    if status == EnrollmentState::PendingUserApproval && approved_at.is_some() {
        status = EnrollmentState::Approved;
    }
    Ok(NodeEnrollmentSummary {
        enrollment_id: EnrollmentId::from(row.try_get::<String, _>("enrollment_id")?),
        display_name: row.try_get("display_name")?,
        status,
        claimed_node_id: claimed_node_id.map(NodeId::from),
        expires_at: row.try_get("expires_at")?,
        created_at: row.try_get("created_at")?,
        approved_at,
    })
}

pub(crate) async fn verify_node_credential(
    state: &AppState,
    node_id: &NodeId,
    credential: Option<&str>,
) -> Result<(), AppError> {
    let credential = credential
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppError::auth(
                "auth_dev.credential_required",
                "Node credential is required",
            )
        })?;
    let auth_key = format!("node:{}", node_id.as_str());
    reject_if_auth_rate_limited(state, &auth_key).await?;
    let row = sqlx::query("select presence, credential_hash from nodes where node_id = ?1")
        .bind(node_id.as_str())
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::auth("auth_dev.node_unknown", "Node is not enrolled"))?;
    if parse_presence(row.try_get::<String, _>("presence")?.as_str()) == NodePresence::Revoked {
        audit_security_event(
            state,
            "node.auth.rejected",
            Some(node_id),
            None,
            "rejected",
            JsonValue(json!({ "reason": "revoked" })),
        )
        .await?;
        record_auth_failure(state, &auth_key).await;
        return Err(AppError::auth(
            "auth_dev.node_revoked",
            "Node has been revoked",
        ));
    }
    let Some(credential_hash) = row.try_get::<Option<String>, _>("credential_hash")? else {
        audit_security_event(
            state,
            "node.auth.rejected",
            Some(node_id),
            None,
            "rejected",
            JsonValue(json!({ "reason": "credential_missing" })),
        )
        .await?;
        record_auth_failure(state, &auth_key).await;
        return Err(AppError::auth(
            "auth_dev.credential_missing",
            "Node credential is missing",
        ));
    };
    if !constant_time_eq(
        credential_hash.as_bytes(),
        hash_secret(credential).as_bytes(),
    ) {
        audit_security_event(
            state,
            "node.auth.rejected",
            Some(node_id),
            None,
            "rejected",
            JsonValue(json!({ "reason": "credential_invalid" })),
        )
        .await?;
        record_auth_failure(state, &auth_key).await;
        return Err(AppError::auth(
            "auth_dev.credential_invalid",
            "Node credential is invalid",
        ));
    }
    clear_auth_failures(state, &auth_key).await;
    Ok(())
}

pub(crate) async fn upsert_heartbeat_workspaces(
    state: &AppState,
    node_id: &NodeId,
    workspaces: Vec<WorkspaceSnapshot>,
) -> Result<(), AppError> {
    for workspace in workspaces {
        if workspace_binding_deleted(state, node_id, &workspace.workspace_path).await? {
            continue;
        }
        let placement_id = stable_placement_id(node_id, &workspace.workspace_path);
        let project_id = stable_project_id(node_id, &workspace.workspace_path);
        upsert_project(
            state,
            &project_id,
            &workspace.display_name,
            workspace.last_validated_at,
        )
        .await?;
        sqlx::query(
            r#"
            insert into project_placements (
                project_placement_id, project_id, node_id, display_name, workspace_path,
                state, resource_badges_json, git_snapshot_json, last_validated_at, created_at, updated_at
            )
            values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?9)
            on conflict(node_id, workspace_path) do update set
                project_id = excluded.project_id,
                display_name = excluded.display_name,
                state = excluded.state,
                resource_badges_json = excluded.resource_badges_json,
                git_snapshot_json = excluded.git_snapshot_json,
                last_validated_at = excluded.last_validated_at,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(placement_id.as_str())
        .bind(project_id.as_str())
        .bind(node_id.as_str())
        .bind(workspace.display_name)
        .bind(workspace.workspace_path)
        .bind(format_placement_state(workspace.state))
        .bind(serde_json::to_string(&workspace.resource_badges)?)
        .bind(
            workspace
                .git_snapshot
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
        )
        .bind(workspace.last_validated_at)
        .execute(&state.pool)
        .await?;
    }
    Ok(())
}

pub(crate) async fn workspace_binding_deleted(
    state: &AppState,
    node_id: &NodeId,
    workspace_path: &str,
) -> Result<bool, AppError> {
    sqlx::query_scalar::<_, i64>(
        r#"
        select 1
        from deleted_workspace_bindings
        where node_id = ?1 and workspace_path = ?2
        "#,
    )
    .bind(node_id.as_str())
    .bind(workspace_path)
    .fetch_optional(&state.pool)
    .await
    .map(|row| row.is_some())
    .map_err(AppError::from)
}

pub(crate) async fn replace_node_capabilities(
    state: &AppState,
    node_id: &NodeId,
    capabilities: &[CapabilitySummary],
    updated_at: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query("delete from node_capabilities where node_id = ?1")
        .bind(node_id.as_str())
        .execute(&state.pool)
        .await?;
    for capability in capabilities {
        sqlx::query(
            r#"
            insert into node_capabilities (
                node_id, capability_key, value_json, updated_at
            )
            values (?1, ?2, ?3, ?4)
            "#,
        )
        .bind(node_id.as_str())
        .bind(&capability.key)
        .bind(serde_json::to_string(&capability.value)?)
        .bind(updated_at)
        .execute(&state.pool)
        .await?;
    }
    Ok(())
}

pub(crate) async fn upsert_project(
    state: &AppState,
    project_id: &ProjectId,
    display_name: &str,
    updated_at: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        insert into projects (project_id, display_name, repo_id, created_at, updated_at)
        values (?1, ?2, null, ?3, ?3)
        on conflict(project_id) do update set
            display_name = excluded.display_name,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(project_id.as_str())
    .bind(display_name)
    .bind(updated_at)
    .execute(&state.pool)
    .await?;
    Ok(())
}

pub(crate) async fn effective_node_presence(
    state: &AppState,
    node_id: &NodeId,
) -> Result<NodePresence, AppError> {
    let row = sqlx::query("select presence, last_heartbeat_at from nodes where node_id = ?1")
        .bind(node_id.as_str())
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("node.not_found", "Node not found"))?;
    let last_heartbeat_at: Option<DateTime<Utc>> = row.try_get("last_heartbeat_at")?;
    let heartbeat_age_seconds = last_heartbeat_at.map(|timestamp| {
        Utc::now()
            .signed_duration_since(timestamp)
            .num_seconds()
            .max(0)
    });
    Ok(derive_presence(
        parse_presence(row.try_get::<String, _>("presence")?.as_str()),
        heartbeat_age_seconds,
        state.config.stale_after_seconds,
        state.config.offline_after_seconds,
    ))
}

pub(crate) async fn ensure_node_commandable(
    state: &AppState,
    node_id: &NodeId,
) -> Result<(), AppError> {
    match effective_node_presence(state, node_id).await? {
        NodePresence::Reachable | NodePresence::Stale => Ok(()),
        NodePresence::Offline => Err(AppError::bad_request(
            "node.offline",
            "Node is offline and cannot accept commands",
        )),
        NodePresence::Revoked => Err(AppError::bad_request(
            "node.revoked",
            "Node has been revoked and cannot accept commands",
        )),
    }
}

pub(crate) async fn ensure_session_commandable(
    state: &AppState,
    detail: &SessionDetail,
    command_kind: CommandKind,
) -> Result<(), AppError> {
    if command_kind == CommandKind::ResumeRuntime {
        let stop_pending: bool = sqlx::query_scalar(
            r#"
            select exists(
                select 1 from commands
                where runtime_session_id = ?1
                  and kind = 'StopRuntime'
                  and state not in ('completed', 'failed', 'expired')
            )
            "#,
        )
        .bind(detail.session.runtime.runtime_session_id.as_str())
        .fetch_one(&state.pool)
        .await?;
        if stop_pending {
            return Err(AppError::conflict(
                "runtime.stop_pending",
                "Runtime stop must finish before it can be resumed",
            ));
        }
    }
    if command_requires_attached_session(command_kind)
        && detail.session.state == SessionThreadState::Detached
    {
        return Err(AppError::bad_request(
            "session.detached",
            "Session is detached; attach before sending interactive commands",
        ));
    }
    ensure_runtime_accepts_command(&detail.session.runtime, command_kind)?;
    ensure_node_commandable(state, &detail.placement.node_id).await?;
    if command_requires_startable_placement(command_kind) {
        ensure_placement_startable(&detail.placement)?;
    }
    if command_requires_provider_capability(command_kind) {
        ensure_node_supports_execution_profile(
            state,
            &detail.placement.node_id,
            &detail.session.runtime.provider,
            detail.session.runtime.execution_profile,
        )
        .await?;
    }
    Ok(())
}

pub(crate) fn command_requires_startable_placement(command_kind: CommandKind) -> bool {
    matches!(
        command_kind,
        CommandKind::StartRuntime
            | CommandKind::SendTurn
            | CommandKind::ResumeRuntime
            | CommandKind::RequestDeduction
    )
}

pub(crate) fn ensure_runtime_accepts_command(
    runtime: &RuntimeSummary,
    command_kind: CommandKind,
) -> Result<(), AppError> {
    let accepts = match command_kind {
        CommandKind::SendTurn | CommandKind::RequestDeduction => matches!(
            runtime.state,
            RuntimeSessionState::Ready | RuntimeSessionState::Running
        ),
        CommandKind::ResolveApproval | CommandKind::SubmitUserInput => {
            runtime.state == RuntimeSessionState::Blocked
        }
        CommandKind::InterruptRuntime => matches!(
            runtime.state,
            RuntimeSessionState::Running | RuntimeSessionState::Blocked
        ),
        CommandKind::StopRuntime => !matches!(
            runtime.state,
            RuntimeSessionState::Stopped | RuntimeSessionState::Expired
        ),
        CommandKind::ResumeRuntime => matches!(
            runtime.state,
            RuntimeSessionState::Stopped
                | RuntimeSessionState::Expired
                | RuntimeSessionState::Stale
                | RuntimeSessionState::Error
                | RuntimeSessionState::Interrupted
        ),
        CommandKind::StartRuntime
        | CommandKind::ValidateWorkspace
        | CommandKind::RefreshResourceSnapshot
        | CommandKind::ListWorkspaceTree
        | CommandKind::ReadWorkspaceFile
        | CommandKind::WriteWorkspaceFile
        | CommandKind::RunWorkspaceCommand
        | CommandKind::ReadWorkspaceDiff
        | CommandKind::OpenWorkspaceTerminal
        | CommandKind::AttachWorkspaceTerminal
        | CommandKind::ResizeWorkspaceTerminal
        | CommandKind::WriteWorkspaceTerminal
        | CommandKind::CloseWorkspaceTerminal
        | CommandKind::CancelDeduction
        | CommandKind::RunTask
        | CommandKind::CancelTaskRun
        | CommandKind::Tooling => true,
        CommandKind::Extension => false,
    };
    if accepts {
        return Ok(());
    }
    Err(AppError::bad_request(
        "runtime.command_not_allowed",
        format!(
            "Runtime state `{}` cannot accept `{command_kind:?}`",
            format_runtime_state(runtime.state)
        ),
    ))
}

pub(crate) fn ensure_pending_approval(
    detail: &SessionDetail,
    approval_id: &ApprovalId,
) -> Result<(), AppError> {
    if pending_approvals(&detail.events)
        .iter()
        .any(|pending| pending == approval_id)
    {
        return Ok(());
    }
    Err(AppError::bad_request(
        "approval.not_pending",
        "Approval is not pending for this session",
    ))
}

pub(crate) fn command_requires_attached_session(command_kind: CommandKind) -> bool {
    matches!(
        command_kind,
        CommandKind::SendTurn | CommandKind::ResolveApproval | CommandKind::SubmitUserInput
    )
}

pub(crate) fn command_requires_provider_capability(command_kind: CommandKind) -> bool {
    matches!(
        command_kind,
        CommandKind::StartRuntime
            | CommandKind::ResumeRuntime
            | CommandKind::SendTurn
            | CommandKind::ResolveApproval
            | CommandKind::SubmitUserInput
            | CommandKind::RequestDeduction
    )
}

pub(crate) fn ensure_placement_startable(
    placement: &ProjectPlacementSummary,
) -> Result<(), AppError> {
    if placement.state != PlacementState::Validated {
        return Err(AppError::bad_request(
            "placement.not_startable",
            "Workspace placement is not startable",
        ));
    }
    if placement_has_hard_block(placement) {
        return Err(AppError::bad_request(
            "placement.hard_blocked",
            "Workspace placement has a hard-blocking resource badge",
        ));
    }
    Ok(())
}

pub(crate) fn placement_has_hard_block(placement: &ProjectPlacementSummary) -> bool {
    placement
        .resource_badges
        .iter()
        .any(|badge| badge.severity == WarningSeverity::HardBlock)
}

pub(crate) async fn ensure_node_supports_provider(
    state: &AppState,
    node_id: &NodeId,
    provider: &str,
) -> Result<(), AppError> {
    if node_supports_provider(state, node_id, provider).await? {
        return Ok(());
    }
    Err(AppError::bad_request(
        "node.capability_missing",
        format!("Node does not advertise provider capability `{provider}`"),
    ))
}

pub(crate) async fn ensure_node_supports_execution_profile(
    state: &AppState,
    node_id: &NodeId,
    provider: &str,
    profile: AgentExecutionProfile,
) -> Result<Vec<ProviderRuntimeCapability>, AppError> {
    if provider != "codex" {
        if profile == AgentExecutionProfile::Managed {
            return Err(AppError::bad_request(
                "runtime.managed_provider_unsupported",
                format!("Managed execution is not implemented for provider `{provider}`"),
            ));
        }
        ensure_node_supports_provider(state, node_id, provider).await?;
        return Ok(Vec::new());
    }

    let required = match profile {
        AgentExecutionProfile::Managed => {
            ProviderRuntimeCapability::required_for_managed_codex().to_vec()
        }
        AgentExecutionProfile::ExecCompatibility => {
            vec![ProviderRuntimeCapability::CodexExec]
        }
    };
    let mut unavailable = Vec::new();
    for capability in &required {
        if !node_supports_capability(state, node_id, capability.as_str()).await? {
            unavailable.push(capability.as_str());
        }
    }
    if unavailable.is_empty() {
        return Ok(required);
    }

    // Rolling compatibility: pre-foundation Nodes only advertised provider.codex.
    if profile == AgentExecutionProfile::ExecCompatibility
        && node_supports_provider(state, node_id, provider).await?
    {
        return Ok(required);
    }

    Err(AppError::bad_request(
        "runtime.profile_capability_unavailable",
        format!(
            "Node cannot admit `{profile:?}` execution; unavailable capabilities: {}",
            unavailable.join(", ")
        ),
    ))
}

pub(crate) async fn provider_version_for_node(
    state: &AppState,
    node_id: &NodeId,
    provider: &str,
) -> Result<Option<String>, AppError> {
    let capability_json: Option<String> = sqlx::query_scalar(
        "select value_json from node_capabilities where node_id = ?1 and capability_key = ?2",
    )
    .bind(node_id.as_str())
    .bind(format!("provider.{provider}.version"))
    .fetch_optional(&state.pool)
    .await?;
    let Some(capability_json) = capability_json else {
        return Ok(None);
    };
    let capability: CapabilityValue = serde_json::from_str(&capability_json)?;
    Ok(match capability {
        CapabilityValue::Extension { name, value } if name == "provider_version" => {
            value.0.as_str().map(str::to_owned)
        }
        _ => None,
    })
}

pub(crate) async fn node_supports_capability(
    state: &AppState,
    node_id: &NodeId,
    capability_key: &str,
) -> Result<bool, AppError> {
    let capability_json: Option<String> = sqlx::query_scalar(
        "select value_json from node_capabilities where node_id = ?1 and capability_key = ?2",
    )
    .bind(node_id.as_str())
    .bind(capability_key)
    .fetch_optional(&state.pool)
    .await?;
    if let Some(capability_json) = capability_json {
        let value = serde_json::from_str::<CapabilityValue>(&capability_json)?;
        return Ok(capability_is_available(&CapabilitySummary {
            key: capability_key.to_owned(),
            value,
        }));
    }

    let capabilities_json: String =
        sqlx::query_scalar("select capabilities_json from nodes where node_id = ?1")
            .bind(node_id.as_str())
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| AppError::not_found("node.not_found", "Node not found"))?;
    let capabilities = serde_json::from_str::<Vec<CapabilitySummary>>(&capabilities_json)?;
    Ok(capabilities
        .iter()
        .any(|capability| capability.key == capability_key && capability_is_available(capability)))
}

pub(crate) async fn node_supports_provider(
    state: &AppState,
    node_id: &NodeId,
    provider: &str,
) -> Result<bool, AppError> {
    let provider_key = format!("provider.{provider}");
    let capability_json: Option<String> = sqlx::query_scalar(
        "select value_json from node_capabilities where node_id = ?1 and capability_key = ?2",
    )
    .bind(node_id.as_str())
    .bind(&provider_key)
    .fetch_optional(&state.pool)
    .await?;
    if let Some(capability_json) = capability_json {
        let value = serde_json::from_str::<CapabilityValue>(&capability_json)?;
        return Ok(capability_is_available(&CapabilitySummary {
            key: provider_key,
            value,
        }));
    }

    let capabilities_json: String =
        sqlx::query_scalar("select capabilities_json from nodes where node_id = ?1")
            .bind(node_id.as_str())
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| AppError::not_found("node.not_found", "Node not found"))?;
    let capabilities = serde_json::from_str::<Vec<CapabilitySummary>>(&capabilities_json)?;
    Ok(capabilities
        .iter()
        .any(|capability| capability.key == provider_key && capability_is_available(capability)))
}

pub(crate) fn capability_is_available(capability: &CapabilitySummary) -> bool {
    match &capability.value {
        CapabilityValue::Provider { available, .. } => *available,
        CapabilityValue::WorkspaceValidation { .. } | CapabilityValue::Extension { .. } => true,
    }
}
