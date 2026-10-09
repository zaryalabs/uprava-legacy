use super::*;

#[tokio::test]
async fn validate_placement_records_node_command_and_pending_state() {
    let state = test_state().await;
    let claim = enroll_test_node(&state).await;
    let node_id = claim.node_id.expect("node id returned");
    heartbeat_test_node(&state, node_id.clone(), claim.credential.clone()).await;
    let workspace_path = std::env::temp_dir().join(format!("uprava-test-{}", Uuid::new_v4()));

    let placement = validate_placement(
        State(state.clone()),
        Json(CreatePlacementRequest {
            node_id: node_id.clone(),
            display_name: "workspace".to_owned(),
            workspace_path: workspace_path.display().to_string(),
        }),
    )
    .await
    .expect("placement validation command records")
    .0;
    let reused = validate_placement(
        State(state.clone()),
        Json(CreatePlacementRequest {
            node_id: node_id.clone(),
            display_name: "renamed workspace".to_owned(),
            workspace_path: workspace_path.display().to_string(),
        }),
    )
    .await
    .expect("existing placement is reused")
    .0;
    assert_eq!(reused.project_placement_id, placement.project_placement_id);
    let (command_kind, command_json): (String, String) = sqlx::query_as(
            "select kind, command_json from commands where target_node_id = ?1 order by created_at desc limit 1",
        )
        .bind(node_id.as_str())
        .fetch_one(&state.pool)
        .await
        .expect("command loads");
    let command =
        serde_json::from_str::<CommandEnvelope>(&command_json).expect("command json decodes");
    let project_id = placement
        .project_id
        .clone()
        .expect("placement has project id");
    let project_display_name: String =
        sqlx::query_scalar("select display_name from projects where project_id = ?1")
            .bind(project_id.as_str())
            .fetch_one(&state.pool)
            .await
            .expect("project row loads");

    assert_eq!(placement.state, PlacementState::Pending);
    assert_eq!(project_display_name, "workspace");
    assert_eq!(command_kind, "ValidateWorkspace");
    assert_eq!(
        command.target.project_placement_id().cloned(),
        Some(placement.project_placement_id.clone())
    );
    assert!(should_open_control_channel(&state, &node_id)
        .await
        .expect("channel request evaluates"));
}

#[tokio::test]
async fn concurrent_validate_placement_reuses_canonical_workspace_identity() {
    let db_path = std::env::temp_dir().join(format!("uprava-test-{}.sqlite", Uuid::new_v4()));
    let pool = sqlite_file_pool_with_connections(&db_path, 4).await;
    let state = AppState::new(test_config(86_400), pool)
        .await
        .expect("state migrates");
    let claim = enroll_test_node(&state).await;
    let node_id = claim.node_id.clone().expect("node id returned");
    heartbeat_test_node(&state, node_id.clone(), claim.credential.clone()).await;
    let workspace_path = std::env::temp_dir().join(format!("uprava-test-{}", Uuid::new_v4()));
    let request = CreatePlacementRequest {
        node_id: node_id.clone(),
        display_name: "workspace".to_owned(),
        workspace_path: workspace_path.display().to_string(),
    };

    let (first, second) = tokio::join!(
        validate_placement(State(state.clone()), Json(request.clone())),
        validate_placement(
            State(state.clone()),
            Json(CreatePlacementRequest {
                display_name: "renamed workspace".to_owned(),
                ..request
            })
        ),
    );
    let first = first.expect("first placement validates").0;
    let second = second.expect("second placement validates").0;
    let placement_count: i64 = sqlx::query_scalar(
        "select count(*) from project_placements where node_id = ?1 and workspace_path = ?2",
    )
    .bind(node_id.as_str())
    .bind(&first.workspace_path)
    .fetch_one(&state.pool)
    .await
    .expect("placement count loads");
    let project_count: i64 = sqlx::query_scalar("select count(*) from projects")
        .fetch_one(&state.pool)
        .await
        .expect("project count loads");
    state.pool.close().await;
    remove_sqlite_file_set(&db_path);

    assert_eq!(first.project_placement_id, second.project_placement_id);
    assert_eq!(placement_count, 1);
    assert_eq!(project_count, 1);
}

#[tokio::test]
async fn command_api_uses_request_correlation_id_header() {
    let state = test_state().await;
    let claim = enroll_test_node(&state).await;
    let node_id = claim.node_id.expect("node id returned");
    heartbeat_test_node(&state, node_id.clone(), claim.credential.clone()).await;
    let workspace_path = std::env::temp_dir().join(format!("uprava-test-{}", Uuid::new_v4()));
    let app = build_router(state.clone());

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/project-placements/validate")
                .header(CONTENT_TYPE, "application/json")
                .header(CORRELATION_ID_HEADER, "corr-http-1")
                .body(Body::from(
                    serde_json::to_vec(&CreatePlacementRequest {
                        node_id: node_id.clone(),
                        display_name: "workspace".to_owned(),
                        workspace_path: workspace_path.display().to_string(),
                    })
                    .expect("request serializes"),
                ))
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    let command_json: String = sqlx::query_scalar(
            "select command_json from commands where target_node_id = ?1 order by created_at desc limit 1",
        )
        .bind(node_id.as_str())
        .fetch_one(&state.pool)
        .await
        .expect("command loads");
    let command =
        serde_json::from_str::<CommandEnvelope>(&command_json).expect("command json decodes");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(command.correlation_id.as_str(), "corr-http-1");
}

#[tokio::test]
async fn validate_placement_rejects_offline_node() {
    let state = test_state().await;
    let claim = enroll_test_node(&state).await;
    let node_id = claim.node_id.expect("node id returned");
    let workspace_path = std::env::temp_dir().join(format!("uprava-test-{}", Uuid::new_v4()));

    let result = validate_placement(
        State(state),
        Json(CreatePlacementRequest {
            node_id,
            display_name: "workspace".to_owned(),
            workspace_path: workspace_path.display().to_string(),
        }),
    )
    .await;

    assert!(matches!(
        result,
        Err(AppError::BadRequest {
            code: "node.offline",
            ..
        })
    ));
}

#[tokio::test]
async fn workspace_validated_event_updates_pending_placement() {
    let state = test_state().await;
    let claim = enroll_test_node(&state).await;
    let node_id = claim.node_id.expect("node id returned");
    heartbeat_test_node(&state, node_id.clone(), claim.credential.clone()).await;
    let workspace_path = std::env::temp_dir().join(format!("uprava-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace dir creates");
    let placement = validate_placement(
        State(state.clone()),
        Json(CreatePlacementRequest {
            node_id: node_id.clone(),
            display_name: "workspace".to_owned(),
            workspace_path: workspace_path.display().to_string(),
        }),
    )
    .await
    .expect("placement validation command records")
    .0;

    accept_workspace_validation_event(
        &state,
        &placement,
        node_id,
        PlacementState::Validated,
        vec![ResourceBadge {
            kind: "git_workspace".to_owned(),
            severity: WarningSeverity::Info,
            label: "Git workspace".to_owned(),
        }],
    )
    .await;
    let placement = load_placement(&state, &placement.project_placement_id)
        .await
        .expect("placement reloads");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert_eq!(placement.state, PlacementState::Validated);
    assert_eq!(placement.resource_badges[0].kind, "git_workspace");
}

#[tokio::test]
async fn placement_projection_warns_when_workspace_has_active_session() {
    let state = test_state().await;
    let (_node_id, detail, workspace_path) = create_test_session(&state).await;

    let placement = load_placement(&state, &detail.placement.project_placement_id)
        .await
        .expect("placement reloads");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert!(placement.resource_badges.iter().any(|badge| {
        badge.kind == "same_workspace_active" && badge.severity == WarningSeverity::Warning
    }));
}

#[tokio::test]
async fn placement_projection_warns_when_same_repo_branch_is_active_elsewhere() {
    let state = test_state().await;
    let (node_id, first_detail, first_workspace_path) = create_test_session(&state).await;
    let second_workspace_path =
        std::env::temp_dir().join(format!("uprava-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&second_workspace_path).expect("second workspace creates");
    let second = validate_placement(
        State(state.clone()),
        Json(CreatePlacementRequest {
            node_id: node_id.clone(),
            display_name: "second workspace".to_owned(),
            workspace_path: second_workspace_path.display().to_string(),
        }),
    )
    .await
    .expect("second placement validates")
    .0;
    let snapshot = GitWorkspaceSnapshot {
        state: uprava_protocol::GitRepositoryState::Ready,
        repo_id: Some("sha256:test-repository".to_owned()),
        head_state: Some(uprava_protocol::GitHeadState::Branch),
        branch: Some("feature/review".to_owned()),
        generated_at: Utc::now(),
        ..GitWorkspaceSnapshot::default()
    };
    sqlx::query(
        "update project_placements set git_snapshot_json = ?1 where project_placement_id = ?2",
    )
    .bind(serde_json::to_string(&snapshot).expect("snapshot serializes"))
    .bind(first_detail.placement.project_placement_id.as_str())
    .execute(&state.pool)
    .await
    .expect("first snapshot persists");
    accept_placement_snapshot_event_with_git(
        &state,
        &second,
        node_id,
        EventKind::WorkspaceValidated,
        PlacementState::Validated,
        vec![],
        Some(snapshot),
    )
    .await;

    let second = load_placement(&state, &second.project_placement_id)
        .await
        .expect("second placement reloads");
    std::fs::remove_dir_all(&first_workspace_path).expect("first workspace removes");
    std::fs::remove_dir_all(&second_workspace_path).expect("second workspace removes");

    assert!(second.resource_badges.iter().any(|badge| {
        badge.kind == "same_repo_branch_active" && badge.severity == WarningSeverity::Warning
    }));
    assert_eq!(
        second
            .git_snapshot
            .as_ref()
            .and_then(|git| git.branch.as_deref()),
        Some("feature/review")
    );
}

#[tokio::test]
async fn session_detail_does_not_warn_about_its_own_active_runtime() {
    let state = test_state().await;
    let (_node_id, detail, workspace_path) = create_test_session(&state).await;

    let detail = load_session_detail(&state, &detail.session.session_thread_id)
        .await
        .expect("session reloads");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert!(!detail
        .placement
        .resource_badges
        .iter()
        .any(|badge| badge.kind == "same_workspace_active"));
}

#[tokio::test]
async fn second_session_on_same_workspace_is_allowed_and_warns() {
    let state = test_state().await;
    let (_node_id, first_detail, workspace_path) = create_test_session(&state).await;

    let second_detail = create_session(
        State(state.clone()),
        Json(CreateSessionRequest {
            project_placement_id: first_detail.placement.project_placement_id,
            title: Some("Second session".to_owned()),
            provider: "codex".to_owned(),
            execution_profile: Some(AgentExecutionProfile::ExecCompatibility),
            force: false,
        }),
    )
    .await
    .expect("second session starts with warning")
    .0;
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert!(second_detail
        .placement
        .resource_badges
        .iter()
        .any(|badge| badge.kind == "same_workspace_active"));
}

#[tokio::test]
async fn refresh_resource_snapshot_records_node_command() {
    let state = test_state().await;
    let claim = enroll_test_node(&state).await;
    let node_id = claim.node_id.expect("node id returned");
    heartbeat_test_node(&state, node_id.clone(), claim.credential.clone()).await;
    let workspace_path = std::env::temp_dir().join(format!("uprava-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace dir creates");
    let placement = validate_placement(
        State(state.clone()),
        Json(CreatePlacementRequest {
            node_id: node_id.clone(),
            display_name: "workspace".to_owned(),
            workspace_path: workspace_path.display().to_string(),
        }),
    )
    .await
    .expect("placement validation command records")
    .0;
    accept_workspace_validation_event(
        &state,
        &placement,
        node_id,
        PlacementState::Validated,
        vec![],
    )
    .await;

    let response = refresh_resource_snapshot(
        State(state.clone()),
        Path(placement.project_placement_id.to_string()),
    )
    .await
    .expect("resource snapshot refresh records")
    .0;
    let (command_kind, command_json): (String, String) =
        sqlx::query_as("select kind, command_json from commands where command_id = ?1")
            .bind(response.command_id.as_str())
            .fetch_one(&state.pool)
            .await
            .expect("command loads");
    let command =
        serde_json::from_str::<CommandEnvelope>(&command_json).expect("command json decodes");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert_eq!(command_kind, "RefreshResourceSnapshot");
    assert_eq!(
        command.target.project_placement_id().cloned(),
        Some(placement.project_placement_id)
    );
}

#[tokio::test]
async fn create_session_rejects_missing_provider_capability_without_recording_command() {
    let state = test_state().await;
    let claim = enroll_test_node(&state).await;
    let node_id = claim.node_id.expect("node id returned");
    heartbeat_test_node(&state, node_id.clone(), claim.credential.clone()).await;
    let workspace_path = std::env::temp_dir().join(format!("uprava-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace dir creates");
    let placement = validate_placement(
        State(state.clone()),
        Json(CreatePlacementRequest {
            node_id: node_id.clone(),
            display_name: "workspace".to_owned(),
            workspace_path: workspace_path.display().to_string(),
        }),
    )
    .await
    .expect("placement validation command records")
    .0;
    accept_workspace_validation_event(
        &state,
        &placement,
        node_id,
        PlacementState::Validated,
        vec![],
    )
    .await;
    let command_count_before = command_count(&state).await;

    let result = create_session(
        State(state.clone()),
        Json(CreateSessionRequest {
            project_placement_id: placement.project_placement_id,
            title: Some("Unsupported session".to_owned()),
            provider: "opencode".to_owned(),
            execution_profile: Some(AgentExecutionProfile::ExecCompatibility),
            force: false,
        }),
    )
    .await;
    let command_count_after = command_count(&state).await;
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert!(matches!(
        result,
        Err(AppError::BadRequest {
            code: "node.capability_missing",
            ..
        })
    ));
    assert_eq!(command_count_after, command_count_before);
}

#[tokio::test]
async fn resource_snapshot_event_updates_placement_state() {
    let state = test_state().await;
    let claim = enroll_test_node(&state).await;
    let node_id = claim.node_id.expect("node id returned");
    heartbeat_test_node(&state, node_id.clone(), claim.credential.clone()).await;
    let workspace_path = std::env::temp_dir().join(format!("uprava-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace dir creates");
    let placement = validate_placement(
        State(state.clone()),
        Json(CreatePlacementRequest {
            node_id: node_id.clone(),
            display_name: "workspace".to_owned(),
            workspace_path: workspace_path.display().to_string(),
        }),
    )
    .await
    .expect("placement validation command records")
    .0;
    let warning = ResourceBadge {
        kind: "dirty_workspace".to_owned(),
        severity: WarningSeverity::Warning,
        label: "Dirty workspace".to_owned(),
    };

    accept_placement_snapshot_event(
        &state,
        &placement,
        node_id,
        EventKind::ResourceSnapshotUpdated,
        PlacementState::Validated,
        vec![warning],
    )
    .await;
    let placement = load_placement(&state, &placement.project_placement_id)
        .await
        .expect("placement reloads");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert_eq!(placement.state, PlacementState::Validated);
    assert_eq!(placement.resource_badges[0].kind, "dirty_workspace");
}

#[tokio::test]
async fn node_provider_completed_event_creates_assistant_message() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    let event = node_event_fixture(
        &detail,
        node_id,
        "provider-completed-1",
        1,
        EventKind::ProviderMessageCompleted,
        json!({ "content": "from node" }),
    );

    accept_node_event(&state, event)
        .await
        .expect("node event accepts");
    let messages = load_messages(&state, &detail.session.session_thread_id)
        .await
        .expect("messages load");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert!(messages
        .iter()
        .any(|message| message.role == MessageRole::Assistant && message.content == "from node"));
}

#[tokio::test]
async fn provider_activity_event_persists_without_creating_message() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    let message_count_before = session_message_count(&state, &detail).await;
    let event = node_event_fixture(
        &detail,
        node_id,
        "provider-activity-1",
        1,
        EventKind::ProviderActivity,
        json!({
            "provider": "codex",
            "source": "codex.exec.jsonl",
            "provider_event_type": "item.completed",
            "raw_event": {
                "type": "item.completed",
                "unknown_future_field": true
            }
        }),
    );

    accept_node_event(&state, event)
        .await
        .expect("provider activity event accepts");
    let persisted_events: i64 =
        sqlx::query_scalar("select count(*) from events where event_id = 'provider-activity-1'")
            .fetch_one(&state.pool)
            .await
            .expect("provider activity event count loads");
    let message_count_after = session_message_count(&state, &detail).await;
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert_eq!(persisted_events, 1);
    assert_eq!(message_count_after, message_count_before);
}

#[tokio::test]
async fn pending_command_requests_control_channel_and_dispatches_after_connect() {
    let state = test_state().await;
    let claim = enroll_test_node(&state).await;
    let node_id = claim.node_id.expect("node id returned");
    let command_id = CommandId::from("pending-command-1");

    record_and_dispatch_command(&state, command_fixture(command_id.clone(), node_id.clone()))
        .await
        .expect("command records");

    assert!(should_open_control_channel(&state, &node_id)
        .await
        .expect("channel request evaluates"));

    let (_context, mut rx) = activate_test_connection(&state, node_id.clone()).await;
    dispatch_pending_commands(&state, &node_id)
        .await
        .expect("pending command dispatches");
    let frame = rx.recv().await.expect("dispatch frame is sent");
    let command_state: String =
        sqlx::query_scalar("select state from commands where command_id = ?1")
            .bind(command_id.as_str())
            .fetch_one(&state.pool)
            .await
            .expect("command state loads");

    assert!(matches!(
        frame,
        ControlFrame::CommandDispatch { command, .. }
            if command.command_id == command_id
    ));
    assert_eq!(command_state, "dispatched");
    let attempts: i64 =
        sqlx::query_scalar("select attempts from command_dispatch_outbox where command_id = ?1")
            .bind(command_id.as_str())
            .fetch_one(&state.pool)
            .await
            .expect("outbox attempts load");
    assert_eq!(attempts, 1);
}

#[tokio::test]
async fn command_dispatch_outbox_is_idempotent_and_clears_on_terminal_result() {
    let state = test_state().await;
    let claim = enroll_test_node(&state).await;
    let node_id = claim.node_id.expect("node id returned");
    let command_id = CommandId::from("outbox-idempotent-command-1");
    let command = command_fixture(command_id.clone(), node_id.clone());

    record_command(&state, command.clone())
        .await
        .expect("command records");
    let duplicate = record_command(&state, command).await;
    assert!(duplicate.is_err(), "command id remains database-idempotent");
    let outbox_count: i64 =
        sqlx::query_scalar("select count(*) from command_dispatch_outbox where command_id = ?1")
            .bind(command_id.as_str())
            .fetch_one(&state.pool)
            .await
            .expect("outbox count loads");
    assert_eq!(outbox_count, 1);

    update_command_result(
        &state,
        &command_id,
        CommandState::Completed,
        &JsonValue(json!({"ok": true})),
    )
    .await
    .expect("terminal result stores");
    let remaining: i64 =
        sqlx::query_scalar("select count(*) from command_dispatch_outbox where command_id = ?1")
            .bind(command_id.as_str())
            .fetch_one(&state.pool)
            .await
            .expect("outbox cleanup loads");
    assert_eq!(remaining, 0);
}

#[tokio::test]
async fn acknowledged_command_without_result_dispatches_after_reconnect() {
    let state = test_state().await;
    let claim = enroll_test_node(&state).await;
    let node_id = claim.node_id.expect("node id returned");
    let command_id = CommandId::from("acknowledged-command-1");

    record_command(&state, command_fixture(command_id.clone(), node_id.clone()))
        .await
        .expect("command records");
    update_command_state(&state, &command_id, CommandState::Acknowledged)
        .await
        .expect("command acknowledges");

    assert!(should_open_control_channel(&state, &node_id)
        .await
        .expect("channel request evaluates"));

    let (_context, mut rx) = activate_test_connection(&state, node_id.clone()).await;
    dispatch_pending_commands(&state, &node_id)
        .await
        .expect("acknowledged command redispatches");
    let frame = rx.recv().await.expect("dispatch frame is sent");

    assert!(matches!(
        frame,
        ControlFrame::CommandDispatch { command, .. }
            if command.command_id == command_id
    ));
}

#[tokio::test]
async fn workspace_file_route_dispatches_node_read_and_decodes_payload() {
    let state = test_state().await;
    let claim = enroll_test_node(&state).await;
    let node_id = claim.node_id.clone().expect("node id returned");
    heartbeat_test_node(&state, node_id.clone(), claim.credential.clone()).await;
    let workspace_path = std::env::temp_dir().join(format!("uprava-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace dir creates");
    let placement = validate_placement(
        State(state.clone()),
        Json(CreatePlacementRequest {
            node_id: node_id.clone(),
            display_name: "workspace".to_owned(),
            workspace_path: workspace_path.display().to_string(),
        }),
    )
    .await
    .expect("placement validates")
    .0;
    accept_workspace_validation_event(
        &state,
        &placement,
        node_id.clone(),
        PlacementState::Validated,
        vec![],
    )
    .await;
    sqlx::query("delete from commands")
        .execute(&state.pool)
        .await
        .expect("setup commands clear");
    let (context, mut rx) = activate_test_connection(&state, node_id.clone()).await;
    let state_for_route = state.clone();
    let placement_id = placement.project_placement_id.clone();
    let route_task = tokio::spawn(async move {
        workspace_file_with_correlation(
            &state_for_route,
            placement_id,
            "README.md".to_owned(),
            CorrelationId::from("correlation-workspace-file"),
        )
        .await
    });

    let dispatched = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("workspace command dispatch is sent")
        .expect("channel stays open");
    let ControlFrame::CommandDispatch { command, .. } = dispatched else {
        panic!("expected command dispatch");
    };
    let response_payload = JsonValue(json!({
        "placement_id": placement.project_placement_id.as_str(),
        "path": "README.md",
        "metadata": {
            "name": "README.md",
            "path": "README.md",
            "kind": "file",
            "status": "readable",
            "byte_len": 5,
            "modified_at": null,
            "children": []
        },
        "content": "hello",
        "truncated": false,
        "generated_at": "2026-06-17T00:00:00Z"
    }));
    handle_node_control_frame(
        &state,
        &context,
        ControlFrame::CommandResult {
            frame_id: "workspace-result-frame".to_owned(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
            command_id: command.command_id.clone(),
            status: CommandState::Completed,
            payload: response_payload,
        },
    )
    .await
    .expect("node command result accepts");
    let response = route_task
        .await
        .expect("route task joins")
        .expect("workspace file route succeeds");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert_eq!(command.kind, CommandKind::ReadWorkspaceFile);
    assert!(matches!(
        command.payload,
        CommandPayload::ReadWorkspaceFile { ref path, .. } if path == "README.md"
    ));
    assert_eq!(response.content.as_deref(), Some("hello"));
}

#[tokio::test]
async fn workspace_check_route_persists_typed_result_for_review_history() {
    let state = test_state().await;
    let claim = enroll_test_node(&state).await;
    let node_id = claim.node_id.clone().expect("node id returned");
    heartbeat_test_node(&state, node_id.clone(), claim.credential.clone()).await;
    let workspace_path = std::env::temp_dir().join(format!("uprava-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace dir creates");
    let placement = validate_placement(
        State(state.clone()),
        Json(CreatePlacementRequest {
            node_id: node_id.clone(),
            display_name: "workspace".to_owned(),
            workspace_path: workspace_path.display().to_string(),
        }),
    )
    .await
    .expect("placement validates")
    .0;
    accept_workspace_validation_event(
        &state,
        &placement,
        node_id.clone(),
        PlacementState::Validated,
        vec![],
    )
    .await;
    sqlx::query("delete from commands")
        .execute(&state.pool)
        .await
        .expect("setup commands clear");
    let (context, mut rx) = activate_test_connection(&state, node_id.clone()).await;
    let state_for_route = state.clone();
    let placement_id = placement.project_placement_id.clone();
    let route_task = tokio::spawn(async move {
        workspace_command_run_with_correlation(
            &state_for_route,
            placement_id,
            WorkspaceCommandRunRequest {
                command: "rustc".to_owned(),
                args: vec!["--version".to_owned()],
                intent: uprava_protocol::WorkspaceCommandIntent::Check,
                label: Some("Quick check".to_owned()),
                timeout_seconds: Some(30),
            },
            CorrelationId::from("correlation-workspace-command"),
        )
        .await
    });

    let dispatched = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("workspace command dispatch is sent")
        .expect("channel stays open");
    let ControlFrame::CommandDispatch { command, .. } = dispatched else {
        panic!("expected command dispatch");
    };
    let response_payload = JsonValue(json!({
        "placement_id": placement.project_placement_id.as_str(),
        "terminal_command_id": "terminal-command-test",
        "command": "rustc",
        "args": ["--version"],
        "intent": "check",
        "label": "Quick check",
        "exit_code": 0,
        "success": true,
        "stdout": "rustc 1.0.0\n",
        "stderr": "",
        "stdout_truncated": false,
        "stderr_truncated": false,
        "duration_ms": 10,
        "started_at": "2026-06-17T00:00:00Z",
        "completed_at": "2026-06-17T00:00:01Z"
    }));
    handle_node_control_frame(
        &state,
        &context,
        ControlFrame::CommandResult {
            frame_id: "workspace-command-result-frame".to_owned(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
            command_id: command.command_id.clone(),
            status: CommandState::Completed,
            payload: response_payload,
        },
    )
    .await
    .expect("node command result accepts");
    let response = route_task
        .await
        .expect("route task joins")
        .expect("workspace command route succeeds");
    let history =
        workspace_command_history(&state, placement.project_placement_id.clone(), Some(10))
            .await
            .expect("workspace command history loads");
    let checks = workspace_check_history(&state, placement.project_placement_id.clone(), Some(10))
        .await
        .expect("workspace check history loads");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert_eq!(command.kind, CommandKind::RunWorkspaceCommand);
    assert_eq!(response.stdout, "rustc 1.0.0\n");
    assert_eq!(history.commands.len(), 1);
    assert_eq!(history.commands[0].command_id, command.command_id);
    assert!(history.commands[0].result_payload.is_some());
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].label.as_deref(), Some("Quick check"));
    assert_eq!(checks[0].success, Some(true));
}

#[tokio::test]
async fn workspace_review_route_combines_git_diff_and_check_projection() {
    let state = test_state().await;
    let claim = enroll_test_node(&state).await;
    let node_id = claim.node_id.clone().expect("node id returned");
    heartbeat_test_node(&state, node_id.clone(), claim.credential.clone()).await;
    let workspace_path = std::env::temp_dir().join(format!("uprava-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace dir creates");
    let placement = validate_placement(
        State(state.clone()),
        Json(CreatePlacementRequest {
            node_id: node_id.clone(),
            display_name: "workspace".to_owned(),
            workspace_path: workspace_path.display().to_string(),
        }),
    )
    .await
    .expect("placement validates")
    .0;
    accept_workspace_validation_event(
        &state,
        &placement,
        node_id.clone(),
        PlacementState::Validated,
        vec![],
    )
    .await;
    sqlx::query("delete from commands")
        .execute(&state.pool)
        .await
        .expect("setup commands clear");
    let (context, mut rx) = activate_test_connection(&state, node_id).await;
    let state_for_route = state.clone();
    let placement_id = placement.project_placement_id.clone();
    let route_task = tokio::spawn(async move {
        workspace_review_route(
            State(state_for_route),
            HeaderMap::new(),
            Path(placement_id.to_string()),
            Query(WorkspaceDiffRequest {
                scope: uprava_protocol::WorkspaceDiffScope::Staged,
                path: Some("src/main.rs".to_owned()),
            }),
        )
        .await
    });
    let dispatched = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("workspace review dispatch is sent")
        .expect("channel stays open");
    let ControlFrame::CommandDispatch { command, .. } = dispatched else {
        panic!("expected command dispatch");
    };
    let git_snapshot = GitWorkspaceSnapshot {
        state: uprava_protocol::GitRepositoryState::Ready,
        repo_id: Some("sha256:review".to_owned()),
        head_state: Some(uprava_protocol::GitHeadState::Branch),
        branch: Some("feature/review".to_owned()),
        generated_at: Utc::now(),
        ..GitWorkspaceSnapshot::default()
    };
    let response = WorkspaceDiffResponse {
        placement_id: placement.project_placement_id.clone(),
        diff_id: "workspace-diff-review".to_owned(),
        git_snapshot,
        summary: "1 changed".to_owned(),
        diff: "@@ -1 +1 @@".to_owned(),
        scope: uprava_protocol::WorkspaceDiffScope::Staged,
        path: Some("src/main.rs".to_owned()),
        changed_files: vec![],
        hunks: vec![uprava_protocol::WorkspaceDiffHunk {
            hunk_id: "workspace-diff-review:hunk-1".to_owned(),
            header: "@@ -1 +1 @@".to_owned(),
            patch: "@@ -1 +1 @@\n-before\n+after\n".to_owned(),
        }],
        original: Some("before\n".to_owned()),
        modified: Some("after\n".to_owned()),
        binary: false,
        summary_truncated: false,
        diff_truncated: false,
        generated_at: Utc::now(),
    };
    handle_node_control_frame(
        &state,
        &context,
        ControlFrame::CommandResult {
            frame_id: "workspace-review-result-frame".to_owned(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
            command_id: command.command_id,
            status: CommandState::Completed,
            payload: JsonValue(serde_json::to_value(response).expect("response serializes")),
        },
    )
    .await
    .expect("review result accepts");
    let review = route_task
        .await
        .expect("route task joins")
        .expect("workspace review succeeds")
        .0;
    let hunk_ref = UpravaRef::DiffHunk {
        diff_id: "workspace-diff-review".to_owned(),
        hunk_id: "workspace-diff-review:hunk-1".to_owned(),
    };
    let hunk = resolve_workspace_diff_hunk_reference(
        &state,
        hunk_ref,
        "workspace-diff-review",
        "workspace-diff-review:hunk-1",
    )
    .await
    .expect("diff hunk resolves");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert_eq!(
        review.git_snapshot.branch.as_deref(),
        Some("feature/review")
    );
    assert_eq!(
        review.diff.scope,
        uprava_protocol::WorkspaceDiffScope::Staged
    );
    assert!(review.checks.is_empty());
    assert_eq!(hunk.status, ReferenceResolutionStatus::Resolved);
}

#[tokio::test]
async fn workspace_command_async_resource_reports_progress_and_terminal_result() {
    let state = test_state().await;
    let claim = enroll_test_node(&state).await;
    let node_id = claim.node_id.clone().expect("node id returned");
    heartbeat_test_node(&state, node_id.clone(), claim.credential.clone()).await;
    let workspace_path = std::env::temp_dir().join(format!("uprava-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace dir creates");
    let placement = validate_placement(
        State(state.clone()),
        Json(CreatePlacementRequest {
            node_id: node_id.clone(),
            display_name: "workspace".to_owned(),
            workspace_path: workspace_path.display().to_string(),
        }),
    )
    .await
    .expect("placement validates")
    .0;
    accept_workspace_validation_event(
        &state,
        &placement,
        node_id.clone(),
        PlacementState::Validated,
        vec![],
    )
    .await;
    sqlx::query("delete from commands")
        .execute(&state.pool)
        .await
        .expect("setup commands clear");
    let (context, mut rx) = activate_test_connection(&state, node_id.clone()).await;
    let app = build_router(state.clone());

    let accepted_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!(
                    "/api/v1/placements/{}/workspace/commands/async",
                    placement.project_placement_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&WorkspaceCommandRunRequest {
                        command: "rustc".to_owned(),
                        args: vec!["--version".to_owned()],
                        intent: uprava_protocol::WorkspaceCommandIntent::Command,
                        label: None,
                        timeout_seconds: Some(30),
                    })
                    .expect("request serializes"),
                ))
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(accepted_response.status(), StatusCode::ACCEPTED);
    let accepted_body = to_bytes(accepted_response.into_body(), 64 * 1024)
        .await
        .expect("accepted body loads");
    let accepted = serde_json::from_slice::<CommandAcceptedResponse>(&accepted_body)
        .expect("accepted decodes");

    let dispatched = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("workspace command dispatch is sent")
        .expect("channel stays open");
    let ControlFrame::CommandDispatch { command, .. } = dispatched else {
        panic!("expected command dispatch");
    };
    let progress_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/placements/{}/workspace/commands/async/{}",
                    placement.project_placement_id, accepted.command_id
                ))
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(progress_response.status(), StatusCode::ACCEPTED);

    let response_payload = JsonValue(json!({
        "placement_id": placement.project_placement_id.as_str(),
        "terminal_command_id": "terminal-command-async-test",
        "command": "rustc",
        "args": ["--version"],
        "intent": "command",
        "label": null,
        "exit_code": 0,
        "success": true,
        "stdout": "rustc 1.0.0\n",
        "stderr": "",
        "stdout_truncated": false,
        "stderr_truncated": false,
        "duration_ms": 10,
        "started_at": "2026-06-17T00:00:00Z",
        "completed_at": "2026-06-17T00:00:01Z"
    }));
    handle_node_control_frame(
        &state,
        &context,
        ControlFrame::CommandResult {
            frame_id: "workspace-command-async-result-frame".to_owned(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
            command_id: command.command_id.clone(),
            status: CommandState::Completed,
            payload: response_payload,
        },
    )
    .await
    .expect("node command result accepts");

    let terminal_response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/placements/{}/workspace/commands/async/{}",
                    placement.project_placement_id, accepted.command_id
                ))
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(terminal_response.status(), StatusCode::OK);
    let terminal_body = to_bytes(terminal_response.into_body(), 64 * 1024)
        .await
        .expect("terminal body loads");
    let item = serde_json::from_slice::<WorkspaceCommandHistoryItem>(&terminal_body)
        .expect("resource decodes");
    let result_payload = item.result_payload.expect("result payload persists");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert_eq!(accepted.command_id, command.command_id);
    assert_eq!(item.state, CommandState::Completed);
    assert_eq!(result_payload.0["stdout"], "rustc 1.0.0\n");
}

#[tokio::test]
async fn workspace_command_async_resource_cancels_and_expires_nonterminal_commands() {
    let state = test_state().await;
    let claim = enroll_test_node(&state).await;
    let node_id = claim.node_id.clone().expect("node id returned");
    heartbeat_test_node(&state, node_id.clone(), claim.credential.clone()).await;
    let workspace_path = std::env::temp_dir().join(format!("uprava-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace dir creates");
    let placement = validate_placement(
        State(state.clone()),
        Json(CreatePlacementRequest {
            node_id: node_id.clone(),
            display_name: "workspace".to_owned(),
            workspace_path: workspace_path.display().to_string(),
        }),
    )
    .await
    .expect("placement validates")
    .0;
    accept_workspace_validation_event(
        &state,
        &placement,
        node_id.clone(),
        PlacementState::Validated,
        vec![],
    )
    .await;
    sqlx::query("delete from commands")
        .execute(&state.pool)
        .await
        .expect("setup commands clear");
    let (_context, mut rx) = activate_test_connection(&state, node_id).await;
    let app = build_router(state.clone());

    let cancel_accepted = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!(
                    "/api/v1/placements/{}/workspace/commands/async",
                    placement.project_placement_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&WorkspaceCommandRunRequest {
                        command: "sleep".to_owned(),
                        args: vec!["10".to_owned()],
                        intent: uprava_protocol::WorkspaceCommandIntent::Command,
                        label: None,
                        timeout_seconds: Some(30),
                    })
                    .expect("request serializes"),
                ))
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    let cancel_body = to_bytes(cancel_accepted.into_body(), 64 * 1024)
        .await
        .expect("cancel accepted body loads");
    let cancel_command =
        serde_json::from_slice::<CommandAcceptedResponse>(&cancel_body).expect("accepted decodes");
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("cancel command dispatch is sent")
        .expect("channel stays open");

    let cancel_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(format!(
                    "/api/v1/placements/{}/workspace/commands/async/{}",
                    placement.project_placement_id, cancel_command.command_id
                ))
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    let cancel_body = to_bytes(cancel_response.into_body(), 64 * 1024)
        .await
        .expect("cancel body loads");
    let cancelled = serde_json::from_slice::<WorkspaceCommandHistoryItem>(&cancel_body)
        .expect("cancel decodes");

    let expire_accepted = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!(
                    "/api/v1/placements/{}/workspace/commands/async",
                    placement.project_placement_id
                ))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&WorkspaceCommandRunRequest {
                        command: "sleep".to_owned(),
                        args: vec!["10".to_owned()],
                        intent: uprava_protocol::WorkspaceCommandIntent::Command,
                        label: None,
                        timeout_seconds: Some(1),
                    })
                    .expect("request serializes"),
                ))
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    let expire_body = to_bytes(expire_accepted.into_body(), 64 * 1024)
        .await
        .expect("expire accepted body loads");
    let expire_command =
        serde_json::from_slice::<CommandAcceptedResponse>(&expire_body).expect("accepted decodes");
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("expire command dispatch is sent")
        .expect("channel stays open");
    sqlx::query("update commands set created_at = ?1 where command_id = ?2")
        .bind(Utc::now() - chrono::Duration::seconds(20))
        .bind(expire_command.command_id.as_str())
        .execute(&state.pool)
        .await
        .expect("command age rewinds");

    let expired_response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/placements/{}/workspace/commands/async/{}",
                    placement.project_placement_id, expire_command.command_id
                ))
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(expired_response.status(), StatusCode::OK);
    let expired_body = to_bytes(expired_response.into_body(), 64 * 1024)
        .await
        .expect("expired body loads");
    let expired = serde_json::from_slice::<WorkspaceCommandHistoryItem>(&expired_body)
        .expect("expire decodes");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert_eq!(cancelled.state, CommandState::Expired);
    assert_eq!(
        cancelled
            .result_payload
            .expect("cancel payload")
            .0
            .get("error_code")
            .and_then(serde_json::Value::as_str),
        Some("workspace.command_cancelled")
    );
    assert_eq!(expired.state, CommandState::Expired);
    assert_eq!(
        expired
            .result_payload
            .expect("expiry payload")
            .0
            .get("error_code")
            .and_then(serde_json::Value::as_str),
        Some("workspace.command_expired")
    );
}

#[tokio::test]
async fn workspace_command_timeout_cleans_waiter_registry() {
    let state = test_state().await;
    let claim = enroll_test_node(&state).await;
    let node_id = claim.node_id.clone().expect("node id returned");
    heartbeat_test_node(&state, node_id.clone(), claim.credential.clone()).await;
    let workspace_path = std::env::temp_dir().join(format!("uprava-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace dir creates");
    let placement = validate_placement(
        State(state.clone()),
        Json(CreatePlacementRequest {
            node_id: node_id.clone(),
            display_name: "workspace".to_owned(),
            workspace_path: workspace_path.display().to_string(),
        }),
    )
    .await
    .expect("placement validates")
    .0;
    accept_workspace_validation_event(
        &state,
        &placement,
        node_id.clone(),
        PlacementState::Validated,
        vec![],
    )
    .await;
    let placement = load_placement(&state, &placement.project_placement_id)
        .await
        .expect("placement reloads");
    let (_context, _rx) = activate_test_connection(&state, node_id).await;

    let result = dispatch_workspace_command::<WorkspaceCommandRunResponse>(
        &state,
        &placement,
        CommandKind::RunWorkspaceCommand,
        serde_json::to_value(WorkspaceCommandRunRequest {
            command: "sleep".to_owned(),
            args: vec!["10".to_owned()],
            intent: uprava_protocol::WorkspaceCommandIntent::Command,
            label: None,
            timeout_seconds: Some(1),
        })
        .expect("request serializes"),
        vec![UpravaRef::Workspace {
            placement_id: placement.project_placement_id.clone(),
        }],
        CorrelationId::from("correlation-timeout-cleanup"),
        std::time::Duration::from_millis(1),
    )
    .await;
    let waiters_empty = lock_command_waiters(&state)
        .expect("waiters lock")
        .is_empty();
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert!(
        matches!(
            result,
            Err(AppError::BadRequest {
                code: "workspace.command_timeout",
                ..
            }) | Err(AppError::BadRequest {
                code: "workspace.command_result_unavailable",
                ..
            })
        ),
        "unexpected workspace command result: {result:?}"
    );
    assert!(waiters_empty);
}
