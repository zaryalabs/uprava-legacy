use super::*;

#[tokio::test]
async fn runtime_stopped_closes_managed_attempt_before_resume_attempt_starts() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    let mut started = node_event_fixture(
        &detail,
        node_id.clone(),
        "attempt-before-stop",
        1,
        EventKind::RuntimeAttemptStarted,
        json!({
            "runtime_attempt_id": "attempt-before-stop",
            "state": "ready",
            "reason": "session_start",
        }),
    );
    started.turn_id = None;
    accept_node_event(&state, started)
        .await
        .expect("first attempt starts");
    let mut stopped = node_event_fixture(
        &detail,
        node_id.clone(),
        "runtime-stop",
        2,
        EventKind::RuntimeStopped,
        json!({"provider": "codex", "mode": "managed", "reason": "explicit_stop"}),
    );
    stopped.turn_id = None;
    accept_node_event(&state, stopped)
        .await
        .expect("runtime stop projects");
    let mut resumed = node_event_fixture(
        &detail,
        node_id,
        "attempt-after-stop",
        3,
        EventKind::RuntimeAttemptStarted,
        json!({
            "runtime_attempt_id": "attempt-after-stop",
            "state": "starting",
            "reason": "explicit_resume",
        }),
    );
    resumed.turn_id = None;
    accept_node_event(&state, resumed)
        .await
        .expect("resume attempt supersedes stopped attempt");

    let stopped_attempt: (String, Option<String>) = sqlx::query_as(
        "select state, stop_reason from runtime_attempts where runtime_attempt_id = 'attempt-before-stop'",
    )
    .fetch_one(&state.pool)
    .await
    .expect("stopped attempt loads");
    let current_attempt: String = sqlx::query_scalar(
        "select current_attempt_id from runtime_sessions where runtime_session_id = ?1",
    )
    .bind(detail.session.runtime.runtime_session_id.as_str())
    .fetch_one(&state.pool)
    .await
    .expect("current attempt loads");

    assert_eq!(
        stopped_attempt,
        ("stopped".to_owned(), Some("explicit_stop".to_owned()))
    );
    assert_eq!(current_attempt, "attempt-after-stop");
    let _ = std::fs::remove_dir_all(workspace_path);
}

#[tokio::test]
async fn managed_attempt_and_interaction_events_round_trip_through_persistence() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    let attempt_id = "attempt-foundation-1";
    let mut started = node_event_fixture(
        &detail,
        node_id.clone(),
        "attempt-foundation-started",
        1,
        EventKind::RuntimeAttemptStarted,
        json!({
            "runtime_attempt_id": attempt_id,
            "state": "starting",
            "reason": "session_start",
        }),
    );
    started.turn_id = None;
    accept_node_event(&state, started)
        .await
        .expect("attempt event accepts");
    let mut requested = node_event_fixture(
        &detail,
        node_id.clone(),
        "interaction-foundation-requested",
        2,
        EventKind::ProviderInteractionRequested,
        json!({
            "provider_interaction_id": "interaction-foundation-1",
            "runtime_attempt_id": attempt_id,
            "interaction_kind": "user_input",
            "prompt": "Choose a target",
            "expires_at": null,
        }),
    );
    requested.turn_id = None;
    accept_node_event(&state, requested)
        .await
        .expect("interaction event accepts");
    let projected = load_session_detail(&state, &detail.session.session_thread_id)
        .await
        .expect("session projection loads");

    assert_eq!(
        (
            projected
                .session
                .runtime
                .current_attempt
                .as_ref()
                .map(|attempt| attempt.runtime_attempt_id.as_str()),
            projected.pending_interactions.first().map(|interaction| (
                interaction.provider_interaction_id.as_str(),
                interaction.kind,
                interaction.prompt.as_str(),
            )),
        ),
        (
            Some(attempt_id),
            Some((
                "interaction-foundation-1",
                ProviderInteractionKind::UserInput,
                "Choose a target",
            )),
        )
    );

    let _ = submit_provider_input_route(
        State(state.clone()),
        HeaderMap::new(),
        Path((
            detail.session.session_thread_id.to_string(),
            "interaction-foundation-1".to_owned(),
        )),
        Json(SubmitProviderInputRequest {
            answers: vec!["workspace".to_owned()],
        }),
    )
    .await
    .expect("Core accepts a typed resolution intent");
    let resolving_state: String = sqlx::query_scalar(
        "select state from provider_interactions where provider_interaction_id = 'interaction-foundation-1'",
    )
    .fetch_one(&state.pool)
    .await
    .expect("resolving interaction loads");
    assert_eq!(resolving_state, "resolving");
    let duplicate = submit_provider_input_route(
        State(state.clone()),
        HeaderMap::new(),
        Path((
            detail.session.session_thread_id.to_string(),
            "interaction-foundation-1".to_owned(),
        )),
        Json(SubmitProviderInputRequest {
            answers: vec!["duplicate".to_owned()],
        }),
    )
    .await;
    assert!(matches!(
        duplicate,
        Err(AppError::Conflict {
            code: "provider_interaction.already_resolving",
            ..
        })
    ));
    let resolution_command_count: i64 = sqlx::query_scalar(
        "select count(*) from commands where kind = 'SubmitUserInput' and runtime_session_id = ?1",
    )
    .bind(detail.session.runtime.runtime_session_id.as_str())
    .fetch_one(&state.pool)
    .await
    .expect("resolution command count loads");
    assert_eq!(resolution_command_count, 1);

    let mut resolved = node_event_fixture(
        &detail,
        node_id,
        "interaction-foundation-resolved",
        3,
        EventKind::ProviderInteractionResolved,
        json!({
            "provider_interaction_id": "interaction-foundation-1",
            "runtime_attempt_id": attempt_id,
            "interaction_kind": "user_input",
            "approved": null,
            "answers": ["workspace"],
        }),
    );
    resolved.turn_id = None;
    accept_node_event(&state, resolved)
        .await
        .expect("resolved interaction event accepts");
    let terminal_state: String = sqlx::query_scalar(
        "select state from provider_interactions where provider_interaction_id = 'interaction-foundation-1'",
    )
            .fetch_one(&state.pool)
            .await
            .expect("terminal interaction loads");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert_eq!(terminal_state, "answered");
}

#[tokio::test]
async fn expired_provider_interaction_rejects_late_input_without_command() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    let attempt_id = "attempt-expired-interaction";
    let mut started = node_event_fixture(
        &detail,
        node_id.clone(),
        "expired-interaction-attempt",
        1,
        EventKind::RuntimeAttemptStarted,
        json!({
            "runtime_attempt_id": attempt_id,
            "state": "ready",
            "reason": "session_start",
        }),
    );
    started.turn_id = None;
    accept_node_event(&state, started)
        .await
        .expect("attempt event accepts");
    let mut requested = node_event_fixture(
        &detail,
        node_id,
        "expired-interaction-requested",
        2,
        EventKind::ProviderInteractionRequested,
        json!({
            "provider_interaction_id": "interaction-expired",
            "runtime_attempt_id": attempt_id,
            "interaction_kind": "user_input",
            "prompt": "This request is already stale",
            "expires_at": Utc::now() - chrono::Duration::seconds(1),
        }),
    );
    requested.turn_id = None;
    accept_node_event(&state, requested)
        .await
        .expect("interaction event accepts");
    let before = command_count(&state).await;

    let result = submit_provider_input_route(
        State(state.clone()),
        HeaderMap::new(),
        Path((
            detail.session.session_thread_id.to_string(),
            "interaction-expired".to_owned(),
        )),
        Json(SubmitProviderInputRequest {
            answers: vec!["too late".to_owned()],
        }),
    )
    .await;
    let state_value: String = sqlx::query_scalar(
        "select state from provider_interactions where provider_interaction_id = 'interaction-expired'",
    )
    .fetch_one(&state.pool)
    .await
    .expect("expired interaction reloads");

    assert!(matches!(
        result,
        Err(AppError::Conflict {
            code: "provider_interaction.expired",
            ..
        })
    ));
    assert_eq!(state_value, "expired");
    assert_eq!(command_count(&state).await, before);
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");
}

#[tokio::test]
async fn turn_events_update_durable_turn_state_and_blocked_approval() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    set_session_runtime_state(&state, &detail, RuntimeSessionState::Ready).await;
    let response = send_turn(
        State(state.clone()),
        Path(detail.session.session_thread_id.to_string()),
        Json(SendTurnRequest {
            content: "needs approval".to_owned(),
            collaboration_mode: None,
        }),
    )
    .await
    .expect("turn sends")
    .0;
    let turn_id = turn_id_for_command(&state, &response.command_id).await;
    let mut started = node_event_fixture(
        &detail,
        node_id.clone(),
        "turn-state-started-1",
        1,
        EventKind::TurnStarted,
        json!({}),
    );
    started.turn_id = Some(turn_id.clone());
    accept_node_event(&state, started)
        .await
        .expect("started event accepts");
    let mut approval = node_event_fixture(
        &detail,
        node_id,
        "turn-state-approval-2",
        2,
        EventKind::ApprovalRequested,
        json!({
            "approval_id": "approval-turn-state",
            "prompt": "Allow state change"
        }),
    );
    approval.turn_id = Some(turn_id.clone());
    accept_node_event(&state, approval)
        .await
        .expect("approval event accepts");
    let (turn_state, blocked_approval_id): (String, Option<String>) =
        sqlx::query_as("select state, blocked_approval_id from turns where turn_id = ?1")
            .bind(turn_id.as_str())
            .fetch_one(&state.pool)
            .await
            .expect("turn row loads");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert_eq!(turn_state, "blocked_on_approval");
    assert_eq!(blocked_approval_id.as_deref(), Some("approval-turn-state"));
}

#[tokio::test]
async fn send_turn_rejects_offline_node_without_recording_command() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    set_session_runtime_state(&state, &detail, RuntimeSessionState::Ready).await;
    mark_node_offline(&state, &node_id).await;
    let command_count_before = command_count(&state).await;
    let message_count_before = session_message_count(&state, &detail).await;

    let result = send_turn(
        State(state.clone()),
        Path(detail.session.session_thread_id.to_string()),
        Json(SendTurnRequest {
            content: "hello".to_owned(),
            collaboration_mode: None,
        }),
    )
    .await;
    let command_count_after = command_count(&state).await;
    let message_count_after = session_message_count(&state, &detail).await;
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert!(matches!(
        result,
        Err(AppError::BadRequest {
            code: "node.offline",
            ..
        })
    ));
    assert_eq!(command_count_after, command_count_before);
    assert_eq!(message_count_after, message_count_before);
}

#[tokio::test]
async fn send_turn_rejects_detached_session_without_recording_command() {
    let state = test_state().await;
    let (_node_id, detail, workspace_path) = create_test_session(&state).await;
    let _ = detach_session(
        State(state.clone()),
        Path(detail.session.session_thread_id.to_string()),
    )
    .await
    .expect("session detaches");
    let command_count_before = command_count(&state).await;
    let message_count_before = session_message_count(&state, &detail).await;

    let result = send_turn(
        State(state.clone()),
        Path(detail.session.session_thread_id.to_string()),
        Json(SendTurnRequest {
            content: "hello".to_owned(),
            collaboration_mode: None,
        }),
    )
    .await;
    let command_count_after = command_count(&state).await;
    let message_count_after = session_message_count(&state, &detail).await;
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert!(matches!(
        result,
        Err(AppError::BadRequest {
            code: "session.detached",
            ..
        })
    ));
    assert_eq!(command_count_after, command_count_before);
    assert_eq!(message_count_after, message_count_before);
}

#[tokio::test]
async fn send_turn_rejects_runtime_state_without_recording_command() {
    let state = test_state().await;
    let (_node_id, detail, workspace_path) = create_test_session(&state).await;
    set_session_runtime_state(&state, &detail, RuntimeSessionState::Stopped).await;
    let command_count_before = command_count(&state).await;
    let message_count_before = session_message_count(&state, &detail).await;

    let result = send_turn(
        State(state.clone()),
        Path(detail.session.session_thread_id.to_string()),
        Json(SendTurnRequest {
            content: "hello".to_owned(),
            collaboration_mode: None,
        }),
    )
    .await;
    let command_count_after = command_count(&state).await;
    let message_count_after = session_message_count(&state, &detail).await;
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert!(matches!(
        result,
        Err(AppError::BadRequest {
            code: "runtime.command_not_allowed",
            ..
        })
    ));
    assert_eq!(command_count_after, command_count_before);
    assert_eq!(message_count_after, message_count_before);
}

#[tokio::test]
async fn interrupt_runtime_rejects_ready_runtime_without_recording_command() {
    let state = test_state().await;
    let (_node_id, detail, workspace_path) = create_test_session(&state).await;
    set_session_runtime_state(&state, &detail, RuntimeSessionState::Ready).await;
    let command_count_before = command_count(&state).await;

    let result = interrupt_runtime(
        State(state.clone()),
        Path(detail.session.runtime.runtime_session_id.to_string()),
    )
    .await;
    let command_count_after = command_count(&state).await;
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert!(matches!(
        result,
        Err(AppError::BadRequest {
            code: "runtime.command_not_allowed",
            ..
        })
    ));
    assert_eq!(command_count_after, command_count_before);
}

#[tokio::test]
async fn resume_runtime_rejects_ready_runtime_without_recording_command() {
    let state = test_state().await;
    let (_node_id, detail, workspace_path) = create_test_session(&state).await;
    set_session_runtime_state(&state, &detail, RuntimeSessionState::Ready).await;
    let command_count_before = command_count(&state).await;

    let result = resume_runtime(
        State(state.clone()),
        Path(detail.session.runtime.runtime_session_id.to_string()),
    )
    .await;
    let command_count_after = command_count(&state).await;
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert!(matches!(
        result,
        Err(AppError::BadRequest {
            code: "runtime.command_not_allowed",
            ..
        })
    ));
    assert_eq!(command_count_after, command_count_before);
}

#[tokio::test]
async fn resume_runtime_accepts_stopped_runtime_with_blank_resume_ref() {
    let state = test_state().await;
    let (_node_id, detail, workspace_path) = create_test_session(&state).await;
    sqlx::query(
        r#"
            update runtime_sessions
            set state = ?1, provider_resume_ref_json = ''
            where runtime_session_id = ?2
            "#,
    )
    .bind(format_runtime_state(RuntimeSessionState::Stopped))
    .bind(detail.session.runtime.runtime_session_id.as_str())
    .execute(&state.pool)
    .await
    .expect("runtime stores blank resume ref");
    sqlx::query("update session_threads set state = ?1 where session_thread_id = ?2")
        .bind(format_session_state(SessionThreadState::Stopped))
        .bind(detail.session.session_thread_id.as_str())
        .execute(&state.pool)
        .await
        .expect("session stops");

    let response = resume_runtime(
        State(state.clone()),
        Path(detail.session.runtime.runtime_session_id.to_string()),
    )
    .await
    .expect("stopped runtime resumes without provider ref")
    .0;
    let command_json: String =
        sqlx::query_scalar("select command_json from commands where command_id = ?1")
            .bind(response.command_id.as_str())
            .fetch_one(&state.pool)
            .await
            .expect("command json loads");
    let command: CommandEnvelope = serde_json::from_str(&command_json).expect("command decodes");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert_eq!(command.kind, CommandKind::ResumeRuntime);
    assert!(matches!(
        command.payload,
        CommandPayload::ResumeRuntime {
            ref workspace_path,
            provider_resume_ref: None,
            ..
        } if workspace_path == &detail.placement.workspace_path
    ));
}

#[tokio::test]
async fn send_turn_rejects_placement_hard_block_without_recording_command() {
    let state = test_state().await;
    let (_node_id, detail, workspace_path) = create_test_session(&state).await;
    set_session_runtime_state(&state, &detail, RuntimeSessionState::Ready).await;
    let hard_block = ResourceBadge {
        kind: "read_only_workspace".to_owned(),
        severity: WarningSeverity::HardBlock,
        label: "Read-only workspace".to_owned(),
    };
    sqlx::query(
        "update project_placements set resource_badges_json = ?1 where project_placement_id = ?2",
    )
    .bind(serde_json::to_string(&vec![hard_block]).expect("badge serializes"))
    .bind(detail.placement.project_placement_id.as_str())
    .execute(&state.pool)
    .await
    .expect("placement hard block stores");
    let command_count_before = command_count(&state).await;
    let message_count_before = session_message_count(&state, &detail).await;

    let result = send_turn(
        State(state.clone()),
        Path(detail.session.session_thread_id.to_string()),
        Json(SendTurnRequest {
            content: "blocked".to_owned(),
            collaboration_mode: None,
        }),
    )
    .await;
    let command_count_after = command_count(&state).await;
    let message_count_after = session_message_count(&state, &detail).await;
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert!(matches!(
        result,
        Err(AppError::BadRequest {
            code: "placement.hard_blocked",
            ..
        })
    ));
    assert_eq!(command_count_after, command_count_before);
    assert_eq!(message_count_after, message_count_before);
}

#[tokio::test]
async fn send_turn_rejects_missing_provider_capability_without_recording_command() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    set_session_runtime_state(&state, &detail, RuntimeSessionState::Ready).await;
    set_node_capabilities(&state, &node_id, vec![]).await;
    let command_count_before = command_count(&state).await;
    let message_count_before = session_message_count(&state, &detail).await;

    let result = send_turn(
        State(state.clone()),
        Path(detail.session.session_thread_id.to_string()),
        Json(SendTurnRequest {
            content: "hello".to_owned(),
            collaboration_mode: None,
        }),
    )
    .await;
    let command_count_after = command_count(&state).await;
    let message_count_after = session_message_count(&state, &detail).await;
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert!(matches!(
        result,
        Err(AppError::BadRequest {
            code: "runtime.profile_capability_unavailable",
            ..
        })
    ));
    assert_eq!(command_count_after, command_count_before);
    assert_eq!(message_count_after, message_count_before);
}

#[tokio::test]
async fn agent_projection_warns_for_offline_node_and_suppresses_node_commands() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    accept_node_event(
        &state,
        node_event_fixture(
            &detail,
            node_id.clone(),
            "projection-ready-before-offline",
            1,
            EventKind::RuntimeReady,
            json!({ "provider": "codex" }),
        ),
    )
    .await
    .expect("ready event accepts");
    mark_node_offline(&state, &node_id).await;

    let projection = build_agent_projection(&state, &detail.session.session_thread_id)
        .await
        .expect("projection builds");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert!(projection.active_warnings.iter().any(|warning| {
        warning.kind == "node_offline" && warning.severity == WarningSeverity::HardBlock
    }));
    assert!(!projection
        .available_commands
        .contains(&ActionCapability::SessionSendTurn));
    assert!(!projection
        .available_commands
        .contains(&ActionCapability::RuntimeStop));
}

#[tokio::test]
async fn agent_projection_warns_for_missing_provider_and_suppresses_provider_commands() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    accept_node_event(
        &state,
        node_event_fixture(
            &detail,
            node_id.clone(),
            "projection-ready-before-provider-missing",
            1,
            EventKind::RuntimeReady,
            json!({ "provider": "codex" }),
        ),
    )
    .await
    .expect("ready event accepts");
    set_node_capabilities(&state, &node_id, vec![]).await;

    let projection = build_agent_projection(&state, &detail.session.session_thread_id)
        .await
        .expect("projection builds");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert!(projection.active_warnings.iter().any(|warning| {
        warning.kind == "provider_unavailable" && warning.severity == WarningSeverity::HardBlock
    }));
    assert!(!projection
        .available_commands
        .contains(&ActionCapability::SessionSendTurn));
    assert!(projection
        .available_commands
        .contains(&ActionCapability::RuntimeStop));
}

#[tokio::test]
async fn agent_projection_switches_between_detach_and_attach_commands() {
    let state = test_state().await;
    let (_node_id, detail, workspace_path) = create_test_session(&state).await;
    let before = build_agent_projection(&state, &detail.session.session_thread_id)
        .await
        .expect("projection builds before detach");

    let _ = detach_session(
        State(state.clone()),
        Path(detail.session.session_thread_id.to_string()),
    )
    .await
    .expect("session detaches");
    let after = build_agent_projection(&state, &detail.session.session_thread_id)
        .await
        .expect("projection builds after detach");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert!(before
        .available_commands
        .contains(&ActionCapability::SessionDetach));
    assert!(!before
        .available_commands
        .contains(&ActionCapability::SessionAttach));
    assert!(after
        .available_commands
        .contains(&ActionCapability::SessionAttach));
    assert!(!after
        .available_commands
        .contains(&ActionCapability::SessionDetach));
    assert!(!after
        .available_commands
        .contains(&ActionCapability::SessionSendTurn));
}

#[tokio::test]
async fn agent_projection_tracks_pending_approval_and_current_turn() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    accept_node_event(
        &state,
        node_event_fixture(
            &detail,
            node_id.clone(),
            "projection-running-1",
            1,
            EventKind::RuntimeRunning,
            json!({ "provider": "codex" }),
        ),
    )
    .await
    .expect("running event accepts");
    accept_node_event(
        &state,
        node_event_fixture(
            &detail,
            node_id.clone(),
            "projection-turn-started-1",
            2,
            EventKind::TurnStarted,
            json!({}),
        ),
    )
    .await
    .expect("turn event accepts");
    accept_node_event(
        &state,
        node_event_fixture(
            &detail,
            node_id.clone(),
            "projection-approval-1",
            3,
            EventKind::ApprovalRequested,
            json!({
                "approval_id": "approval-1",
                "prompt": "Allow projection test"
            }),
        ),
    )
    .await
    .expect("approval event accepts");
    accept_node_event(
        &state,
        node_event_fixture(
            &detail,
            node_id,
            "projection-blocked-1",
            4,
            EventKind::RuntimeBlocked,
            json!({ "provider": "codex" }),
        ),
    )
    .await
    .expect("blocked event accepts");

    let projection = build_agent_projection(&state, &detail.session.session_thread_id)
        .await
        .expect("projection builds");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert_eq!(projection.current_turn, Some(TurnId::from("turn-1")));
    assert_eq!(
        projection.pending_approvals,
        vec![ApprovalId::from("approval-1")]
    );
    assert!(projection
        .available_commands
        .contains(&ActionCapability::ApprovalResolve));
}

#[tokio::test]
async fn agent_projection_requires_blocked_runtime_for_approval_resolution() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    accept_node_event(
        &state,
        node_event_fixture(
            &detail,
            node_id.clone(),
            "projection-approval-ready-1",
            1,
            EventKind::ApprovalRequested,
            json!({
                "approval_id": "approval-ready",
                "prompt": "Allow projection test"
            }),
        ),
    )
    .await
    .expect("approval event accepts");
    accept_node_event(
        &state,
        node_event_fixture(
            &detail,
            node_id,
            "projection-runtime-ready-after-approval",
            2,
            EventKind::RuntimeReady,
            json!({ "provider": "codex" }),
        ),
    )
    .await
    .expect("ready event accepts");

    let projection = build_agent_projection(&state, &detail.session.session_thread_id)
        .await
        .expect("projection builds");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert_eq!(
        projection.pending_approvals,
        vec![ApprovalId::from("approval-ready")]
    );
    assert!(!projection
        .available_commands
        .contains(&ActionCapability::ApprovalResolve));
}

#[tokio::test]
async fn agent_projection_clears_pending_approval_after_resolution() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    accept_node_event(
        &state,
        node_event_fixture(
            &detail,
            node_id.clone(),
            "projection-approval-requested-2",
            1,
            EventKind::ApprovalRequested,
            json!({
                "approval_id": "approval-2",
                "prompt": "Allow projection test"
            }),
        ),
    )
    .await
    .expect("approval event accepts");
    accept_node_event(
        &state,
        node_event_fixture(
            &detail,
            node_id.clone(),
            "projection-approval-resolved-2",
            2,
            EventKind::ApprovalResolved,
            json!({
                "approval_id": "approval-2",
                "approved": true,
                "message": "approved"
            }),
        ),
    )
    .await
    .expect("approval resolution event accepts");
    accept_node_event(
        &state,
        node_event_fixture(
            &detail,
            node_id,
            "projection-ready-2",
            3,
            EventKind::RuntimeReady,
            json!({ "provider": "codex" }),
        ),
    )
    .await
    .expect("ready event accepts");

    let projection = build_agent_projection(&state, &detail.session.session_thread_id)
        .await
        .expect("projection builds");
    let (approval_state, response_payload_json): (String, Option<String>) =
        sqlx::query_as("select state, response_payload_json from approvals where approval_id = ?1")
            .bind("approval-2")
            .fetch_one(&state.pool)
            .await
            .expect("approval row loads");
    let response_payload: serde_json::Value =
        serde_json::from_str(&response_payload_json.expect("response payload stores"))
            .expect("response payload decodes");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert!(projection.pending_approvals.is_empty());
    assert_eq!(approval_state, "resolved");
    assert_eq!(
        response_payload
            .get("message")
            .and_then(serde_json::Value::as_str),
        Some("approved")
    );
    assert!(!projection
        .available_commands
        .contains(&ActionCapability::ApprovalResolve));
    assert!(projection
        .available_commands
        .contains(&ActionCapability::SessionSendTurn));
}

#[tokio::test]
async fn acknowledge_warning_persists_event_and_suppresses_projection_warning() {
    let state = test_state().await;
    let (_node_id, detail, workspace_path) = create_test_session(&state).await;
    let warning = ResourceBadge {
        kind: "dirty_workspace".to_owned(),
        severity: WarningSeverity::Warning,
        label: "Dirty workspace".to_owned(),
    };
    sqlx::query(
        "update project_placements set resource_badges_json = ?1 where project_placement_id = ?2",
    )
    .bind(serde_json::to_string(&vec![warning]).expect("warning serializes"))
    .bind(detail.placement.project_placement_id.as_str())
    .execute(&state.pool)
    .await
    .expect("placement warning stores");
    let before = build_agent_projection(&state, &detail.session.session_thread_id)
        .await
        .expect("projection builds before ack");

    let response = acknowledge_warning(
        State(state.clone()),
        Path((
            detail.session.session_thread_id.to_string(),
            "dirty_workspace".to_owned(),
        )),
        Json(AcknowledgeWarningRequest {
            message: Some("reviewed".to_owned()),
        }),
    )
    .await
    .expect("warning acknowledges")
    .0;
    let row_count: i64 =
        sqlx::query_scalar("select count(*) from warning_acknowledgements where warning_kind = ?1")
            .bind("dirty_workspace")
            .fetch_one(&state.pool)
            .await
            .expect("warning acknowledgement count loads");
    let event_kind: String = sqlx::query_scalar("select kind from events where event_id = ?1")
        .bind(response.event_id.as_str())
        .fetch_one(&state.pool)
        .await
        .expect("ack event loads");
    let after = build_agent_projection(&state, &detail.session.session_thread_id)
        .await
        .expect("projection builds after ack");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert_eq!(before.active_warnings.len(), 1);
    assert!(before
        .available_commands
        .contains(&ActionCapability::WarningAcknowledge));
    assert_eq!(row_count, 1);
    assert_eq!(event_kind, "CoordinationWarningAcknowledged");
    assert!(after.active_warnings.is_empty());
    assert!(!after
        .available_commands
        .contains(&ActionCapability::WarningAcknowledge));
}

#[tokio::test]
async fn evidence_projection_uses_approval_ref_for_approval_event() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    accept_node_event(
        &state,
        node_event_fixture(
            &detail,
            node_id,
            "artifact-approval-1",
            1,
            EventKind::ApprovalRequested,
            json!({
                "approval_id": "approval-artifact-1",
                "prompt": "Allow artifact test"
            }),
        ),
    )
    .await
    .expect("approval event accepts");

    let evidence_projection =
        build_session_evidence_projection(&state, &detail.session.session_thread_id)
            .await
            .expect("evidence projection builds");
    let rebuilt_projection =
        build_session_evidence_projection(&state, &detail.session.session_thread_id)
            .await
            .expect("evidence projection rebuilds");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert_eq!(
        evidence_projection.root.evidence_id,
        rebuilt_projection.root.evidence_id
    );
    assert_eq!(
        evidence_projection
            .root
            .children
            .iter()
            .map(|node| node.evidence_id.clone())
            .collect::<Vec<_>>(),
        rebuilt_projection
            .root
            .children
            .iter()
            .map(|node| node.evidence_id.clone())
            .collect::<Vec<_>>()
    );
    assert!(evidence_projection
        .root
        .children
        .iter()
        .any(|node| matches!(
            &node.primary_ref,
            UpravaRef::Approval { approval_id } if approval_id.as_str() == "approval-artifact-1"
        )));
}

#[tokio::test]
async fn runtime_scoped_provider_event_updates_last_runtime_step() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    let event = node_event_fixture(
        &detail,
        node_id,
        "runtime-step-provider-completed-1",
        1,
        EventKind::ProviderMessageCompleted,
        json!({ "content": "step" }),
    );
    let happened_at = event.happened_at;

    accept_node_event(&state, event)
        .await
        .expect("provider event accepts");
    let detail = load_session_detail(&state, &detail.session.session_thread_id)
        .await
        .expect("session reloads");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert_eq!(
        detail.session.runtime.last_runtime_step_at,
        Some(happened_at)
    );
}

#[tokio::test]
async fn load_session_detail_expires_idle_runtime_with_durable_event() {
    let state = test_state_with_runtime_expiry(1).await;
    let (_node_id, detail, workspace_path) = create_test_session(&state).await;
    set_session_runtime_state(&state, &detail, RuntimeSessionState::Ready).await;
    set_session_runtime_last_step(&state, &detail, Utc::now() - chrono::Duration::seconds(2)).await;
    let (managed_token, _) = issue_managed_mcp_access_lease(
        &state,
        &detail.session.session_thread_id,
        ActorRef::Provider {
            provider: "codex".to_owned(),
        },
    )
    .await
    .expect("Managed lease issues before expiry");

    let detail = load_session_detail(&state, &detail.session.session_thread_id)
        .await
        .expect("session reloads");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert_eq!(detail.session.runtime.state, RuntimeSessionState::Expired);
    assert_eq!(detail.session.state, SessionThreadState::Degraded);
    assert!(detail.events.iter().any(|event| {
        event.kind == EventKind::RuntimeExpired
            && event
                .payload
                .0
                .get("code")
                .and_then(serde_json::Value::as_str)
                == Some("runtime.idle_expired")
    }));
    let stop_command: String = sqlx::query_scalar(
        "select command_json from commands where runtime_session_id = ?1 and kind = 'StopRuntime'",
    )
    .bind(detail.session.runtime.runtime_session_id.as_str())
    .fetch_one(&state.pool)
    .await
    .expect("idle expiry records a Node stop command");
    let stop_command: CommandEnvelope =
        serde_json::from_str(&stop_command).expect("stop command decodes");
    assert!(matches!(
        stop_command.payload,
        CommandPayload::StopRuntime {
            reason: Some(ref reason),
            ..
        } if reason == "idle_expired"
    ));
    let revoked = validate_mcp_access_lease(&state, &managed_token)
        .await
        .expect_err("idle expiry revokes the Managed provider lease");
    assert_eq!(
        revoked.code,
        uprava_protocol::ToolExecutionErrorCode::LeaseRevoked
    );
}

#[tokio::test]
async fn send_turn_rejects_idle_expired_runtime_without_recording_command() {
    let state = test_state_with_runtime_expiry(1).await;
    let (_node_id, detail, workspace_path) = create_test_session(&state).await;
    set_session_runtime_state(&state, &detail, RuntimeSessionState::Ready).await;
    set_session_runtime_last_step(&state, &detail, Utc::now() - chrono::Duration::seconds(2)).await;
    let command_count_before = command_count(&state).await;
    let message_count_before = session_message_count(&state, &detail).await;

    let result = send_turn(
        State(state.clone()),
        Path(detail.session.session_thread_id.to_string()),
        Json(SendTurnRequest {
            content: "after expiry".to_owned(),
            collaboration_mode: None,
        }),
    )
    .await;
    let command_count_after = command_count(&state).await;
    let message_count_after = session_message_count(&state, &detail).await;
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert!(matches!(
        result,
        Err(AppError::BadRequest {
            code: "runtime.command_not_allowed",
            ..
        })
    ));
    assert_eq!(command_count_after, command_count_before + 1);
    assert_eq!(message_count_after, message_count_before);
}

#[tokio::test]
async fn resume_runtime_waits_for_idle_expiry_stop_to_finish() {
    let state = test_state_with_runtime_expiry(1).await;
    let (_node_id, detail, workspace_path) = create_test_session(&state).await;
    set_session_runtime_state(&state, &detail, RuntimeSessionState::Ready).await;
    set_session_runtime_last_step(&state, &detail, Utc::now() - chrono::Duration::seconds(2)).await;
    sqlx::query(
        "update runtime_sessions set provider_resume_ref_json = ?1 where runtime_session_id = ?2",
    )
    .bind(
        json!({
            "provider_session_id": "codex-session-1",
            "resume_cursor": "cursor-1",
        })
        .to_string(),
    )
    .bind(detail.session.runtime.runtime_session_id.as_str())
    .execute(&state.pool)
    .await
    .expect("provider resume ref stores");

    let pending = resume_runtime(
        State(state.clone()),
        Path(detail.session.runtime.runtime_session_id.to_string()),
    )
    .await;
    assert!(matches!(
        pending,
        Err(AppError::Conflict {
            code: "runtime.stop_pending",
            ..
        })
    ));
    sqlx::query(
        "update commands set state = 'completed', completed_at = ?1 where runtime_session_id = ?2 and kind = 'StopRuntime'",
    )
    .bind(Utc::now())
    .bind(detail.session.runtime.runtime_session_id.as_str())
    .execute(&state.pool)
    .await
    .expect("idle stop confirmation stores");
    let response = resume_runtime(
        State(state.clone()),
        Path(detail.session.runtime.runtime_session_id.to_string()),
    )
    .await
    .expect("expired runtime resumes after stop confirmation")
    .0;
    let command_kind: String =
        sqlx::query_scalar("select kind from commands where command_id = ?1")
            .bind(response.command_id.as_str())
            .fetch_one(&state.pool)
            .await
            .expect("command kind loads");
    let command_json: String =
        sqlx::query_scalar("select command_json from commands where command_id = ?1")
            .bind(response.command_id.as_str())
            .fetch_one(&state.pool)
            .await
            .expect("command json loads");
    let command: CommandEnvelope = serde_json::from_str(&command_json).expect("command decodes");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert_eq!(command_kind, "ResumeRuntime");
    let CommandPayload::ResumeRuntime {
        workspace_path,
        provider_resume_ref: Some(provider_resume_ref),
        ..
    } = command.payload
    else {
        panic!("expected typed resume payload");
    };
    assert_eq!(
        provider_resume_ref
            .0
            .get("provider_session_id")
            .and_then(serde_json::Value::as_str),
        Some("codex-session-1")
    );
    assert_eq!(workspace_path, detail.placement.workspace_path);
}

#[tokio::test]
async fn ready_event_clears_degraded_reason_after_sequence_gap() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    accept_node_event(
        &state,
        node_event_fixture(
            &detail,
            node_id.clone(),
            "runtime-gap-before-ready",
            2,
            EventKind::ProviderOutputDelta,
            json!({ "delta": "late" }),
        ),
    )
    .await
    .expect("gap event accepts");
    accept_node_event(
        &state,
        node_event_fixture(
            &detail,
            node_id,
            "runtime-ready-after-gap",
            3,
            EventKind::RuntimeReady,
            json!({ "provider": "codex" }),
        ),
    )
    .await
    .expect("ready event accepts");

    let detail = load_session_detail(&state, &detail.session.session_thread_id)
        .await
        .expect("session reloads");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert_eq!(detail.session.runtime.state, RuntimeSessionState::Ready);
    assert_eq!(detail.session.runtime.degraded_reason, None);
    assert_eq!(detail.session.state, SessionThreadState::Active);
}
