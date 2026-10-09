use super::*;

#[tokio::test]
async fn compatible_control_hello_acknowledges_and_dispatches_pending_command() {
    let state = test_state().await;
    let claim = enroll_test_node(&state).await;
    let node_id = claim.node_id.expect("node id returned");
    let command_id = CommandId::from("hello-dispatch-command-1");
    record_and_dispatch_command(&state, command_fixture(command_id.clone(), node_id.clone()))
        .await
        .expect("command records");
    let (tx, mut rx) = mpsc::channel(CONTROL_QUEUE_CAPACITY);
    let context = state.control_connections.context(node_id.clone(), tx);
    assert!(!state.control_connections.contains(&node_id).await);

    handle_node_control_frame(
        &state,
        &context,
        ControlFrame::Hello {
            frame_id: "hello-frame-1".to_owned(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
            node_id: node_id.clone(),
            daemon_version: "0.1.0".to_owned(),
            active_runtime_ids: vec![],
            actual_runtime_attempts: vec![],
        },
    )
    .await
    .expect("compatible hello accepts");
    assert!(state.control_connections.contains(&node_id).await);
    let hello_ack = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("hello ack is sent")
        .expect("channel stays open");
    let dispatch = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("command dispatch is sent")
        .expect("channel stays open");

    assert!(matches!(hello_ack, ControlFrame::HelloAck { .. }));
    assert!(matches!(
        dispatch,
        ControlFrame::CommandDispatch { command, .. }
            if command.command_id == command_id
    ));
}

#[tokio::test]
async fn managed_actual_state_reconciles_only_the_current_attempt() {
    let state = test_state().await;
    let (node_id, compatibility, workspace_path) = create_test_session(&state).await;
    let capabilities = ProviderRuntimeCapability::required_for_managed_codex()
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
        })
        .chain(std::iter::once(CapabilitySummary {
            key: "provider.codex".to_owned(),
            value: CapabilityValue::provider(true),
        }))
        .collect();
    set_node_capabilities(&state, &node_id, capabilities).await;
    let managed = create_session(
        State(state.clone()),
        Json(CreateSessionRequest {
            project_placement_id: compatibility.placement.project_placement_id,
            title: Some("Managed reconciliation".to_owned()),
            provider: "codex".to_owned(),
            execution_profile: Some(AgentExecutionProfile::Managed),
            force: false,
        }),
    )
    .await
    .expect("managed session creates")
    .0;
    let mut started = node_event_fixture(
        &managed,
        node_id.clone(),
        "reconcile-attempt-started",
        1,
        EventKind::RuntimeAttemptStarted,
        json!({
            "runtime_attempt_id": "attempt-current",
            "state": "starting",
            "reason": "session_start",
        }),
    );
    started.turn_id = None;
    accept_node_event(&state, started)
        .await
        .expect("attempt starts");
    let policy_hash = managed
        .session
        .runtime
        .effective_policy_hash
        .clone()
        .expect("managed policy hash exists");
    let (sender, _receiver) = mpsc::channel(CONTROL_QUEUE_CAPACITY);
    let context = state.control_connections.context(node_id.clone(), sender);
    handle_node_control_frame(
        &state,
        &context,
        ControlFrame::Hello {
            frame_id: "managed-reconcile-hello".to_owned(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
            node_id: node_id.clone(),
            daemon_version: "test".to_owned(),
            active_runtime_ids: vec![managed.session.runtime.runtime_session_id.clone()],
            actual_runtime_attempts: vec![RuntimeAttemptActualState {
                runtime_session_id: managed.session.runtime.runtime_session_id.clone(),
                runtime_attempt_id: RuntimeAttemptId::from("attempt-current"),
                state: RuntimeAttemptState::Recovered,
                effective_policy_hash: policy_hash,
                active_turn_id: None,
                started_at: Utc::now(),
            }],
        },
    )
    .await
    .expect("actual state reconciles");
    let reconciled = load_session_detail(&state, &managed.session.session_thread_id)
        .await
        .expect("reconciled session loads");

    assert_eq!(
        (
            reconciled.session.runtime.recovery_status,
            reconciled
                .session
                .runtime
                .current_attempt
                .map(|attempt| (attempt.runtime_attempt_id, attempt.state)),
        ),
        (
            RuntimeRecoveryStatus::Recovered,
            Some((
                RuntimeAttemptId::from("attempt-current"),
                RuntimeAttemptState::Recovered,
            )),
        )
    );
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");
}

#[tokio::test]
async fn overlapping_control_connections_keep_newest_generation_active() {
    let state = test_state().await;
    let node_id = NodeId::from("overlapping-control-node");
    let (old_sender, _old_receiver) = mpsc::channel(CONTROL_QUEUE_CAPACITY);
    let old_context = state
        .control_connections
        .context(node_id.clone(), old_sender);
    state.control_connections.activate(&old_context).await;
    let (new_sender, _new_receiver) = mpsc::channel(CONTROL_QUEUE_CAPACITY);
    let new_context = state
        .control_connections
        .context(node_id.clone(), new_sender);
    state.control_connections.activate(&new_context).await;

    let old_removed = state
        .control_connections
        .remove_if_active(&old_context)
        .await;
    let stale_result = handle_node_control_frame(
        &state,
        &old_context,
        ControlFrame::CommandAck {
            frame_id: "stale-ack".to_owned(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
            command_id: CommandId::from("stale-command"),
            status: CommandState::Acknowledged,
        },
    )
    .await;
    let stale_hello = handle_node_control_frame(
        &state,
        &old_context,
        ControlFrame::Hello {
            frame_id: "stale-hello".to_owned(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
            node_id: old_context.node_id.clone(),
            daemon_version: "test".to_owned(),
            active_runtime_ids: vec![],
            actual_runtime_attempts: vec![],
        },
    )
    .await;

    assert!(!old_removed);
    assert!(state.control_connections.is_active(&new_context).await);
    assert!(matches!(
        stale_result,
        Err(AppError::Auth {
            code: "control.stale_generation",
            ..
        })
    ));
    assert!(matches!(
        stale_hello,
        Err(AppError::Auth {
            code: "control.stale_generation",
            ..
        })
    ));
}

#[tokio::test]
async fn saturated_control_queue_rejects_frame_and_increments_metric() {
    let state = test_state().await;
    let node_id = NodeId::from("saturated-control-node");
    let (_context, _receiver) = activate_test_connection(&state, node_id.clone()).await;
    for index in 0..CONTROL_QUEUE_CAPACITY {
        assert!(
            send_control_frame(
                &state,
                &node_id,
                ControlFrame::Ping {
                    frame_id: format!("fill-{index}"),
                    protocol_version: API_VERSION.to_owned(),
                    sent_at: Utc::now(),
                },
            )
            .await
        );
    }

    let overflow = try_send_control_frame(
        &state,
        &node_id,
        ControlFrame::Ping {
            frame_id: "overflow".to_owned(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
        },
    )
    .await;

    assert_eq!(overflow, Err(ControlSendError::Saturated));
    assert_eq!(
        state
            .core_metrics
            .control_queue_rejections
            .load(Ordering::Relaxed),
        1
    );
}

#[tokio::test]
async fn cross_node_command_ack_is_rejected_without_state_change() {
    let state = test_state().await;
    let owner = enroll_test_node(&state)
        .await
        .node_id
        .expect("owner node id returned");
    let attacker = enroll_test_node(&state)
        .await
        .node_id
        .expect("attacker node id returned");
    let command_id = CommandId::from("cross-node-command");
    record_command(&state, command_fixture(command_id.clone(), owner))
        .await
        .expect("owned command records");
    update_command_state(&state, &command_id, CommandState::Dispatched)
        .await
        .expect("owned command dispatches");
    let (context, _receiver) = activate_test_connection(&state, attacker).await;

    let result = handle_node_control_frame(
        &state,
        &context,
        ControlFrame::CommandAck {
            frame_id: "cross-node-ack".to_owned(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
            command_id: command_id.clone(),
            status: CommandState::Acknowledged,
        },
    )
    .await;
    let stored_state: String =
        sqlx::query_scalar("select state from commands where command_id = ?1")
            .bind(command_id.as_str())
            .fetch_one(&state.pool)
            .await
            .expect("command state loads");

    assert!(matches!(
        result,
        Err(AppError::Auth {
            code: "control.command_owner_mismatch",
            ..
        })
    ));
    assert_eq!(stored_state, "dispatched");
}

#[tokio::test]
async fn conflicting_duplicate_command_result_is_rejected() {
    let state = test_state().await;
    let node_id = enroll_test_node(&state)
        .await
        .node_id
        .expect("node id returned");
    let command_id = CommandId::from("conflicting-result-command");
    record_command(&state, command_fixture(command_id.clone(), node_id.clone()))
        .await
        .expect("command records");
    update_command_state(&state, &command_id, CommandState::Dispatched)
        .await
        .expect("command dispatches");
    let (context, _receiver) = activate_test_connection(&state, node_id).await;
    let result_frame = |payload| ControlFrame::CommandResult {
        frame_id: Uuid::new_v4().to_string(),
        protocol_version: API_VERSION.to_owned(),
        sent_at: Utc::now(),
        command_id: command_id.clone(),
        status: CommandState::Completed,
        payload: JsonValue(payload),
    };
    handle_node_control_frame(&state, &context, result_frame(json!({"value": 1})))
        .await
        .expect("first terminal result accepts");

    let duplicate =
        handle_node_control_frame(&state, &context, result_frame(json!({"value": 2}))).await;

    assert!(matches!(
        duplicate,
        Err(AppError::BadRequest {
            code: "control.command_result_conflict",
            ..
        })
    ));
}

#[tokio::test]
async fn workspace_result_with_wrong_placement_echo_is_rejected() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    let command_id = CommandId::from("wrong-placement-result");
    let mut command = command_fixture(command_id.clone(), node_id.clone());
    command.kind = CommandKind::ReadWorkspaceFile;
    command.target = CommandTarget::Placement {
        node_id: detail.placement.node_id.clone(),
        project_placement_id: detail.placement.project_placement_id.clone(),
    };
    command.payload = CommandPayload::ReadWorkspaceFile {
        workspace_path: detail.placement.workspace_path.clone(),
        path: "README.md".to_owned(),
    };
    record_command(&state, command)
        .await
        .expect("workspace command records");
    update_command_state(&state, &command_id, CommandState::Dispatched)
        .await
        .expect("workspace command dispatches");
    let (context, _receiver) = activate_test_connection(&state, node_id).await;

    let result = handle_node_control_frame(
        &state,
        &context,
        ControlFrame::CommandResult {
            frame_id: "wrong-placement-result-frame".to_owned(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
            command_id,
            status: CommandState::Completed,
            payload: JsonValue(json!({
                "placement_id": "another-placement",
                "path": "README.md",
                "metadata": {
                    "name": "README.md",
                    "path": "README.md",
                    "kind": "file",
                    "status": "readable",
                    "byte_len": 1,
                    "modified_at": null,
                    "children": []
                },
                "content": "x",
                "truncated": false,
                "generated_at": "2026-07-10T00:00:00Z"
            })),
        },
    )
    .await;
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert!(matches!(
        result,
        Err(AppError::BadRequest {
            code: "control.command_result_target_mismatch",
            ..
        })
    ));
}

#[tokio::test]
async fn oversized_event_batch_is_rejected_before_event_validation() {
    let state = test_state().await;
    let node_id = NodeId::from("oversized-batch-node");
    let (context, _receiver) = activate_test_connection(&state, node_id.clone()).await;
    let event = EventEnvelope {
        event_id: EventId::from("oversized-event"),
        command_id: None,
        correlation_id: None,
        actor_ref: ActorRef::Node {
            node_id: node_id.clone(),
        },
        scope_ref: ScopeRef::Node { node_id },
        node_id: Some(context.node_id.clone()),
        runtime_session_id: None,
        session_thread_id: None,
        turn_id: None,
        seq: 1,
        session_projection_seq: None,
        kind: EventKind::ProviderActivity,
        happened_at: Utc::now(),
        source_refs: vec![],
        evidence_refs: vec![],
        cause_refs: vec![],
        result_refs: vec![],
        payload: EventPayload::from_json(EventKind::ProviderActivity, json!({})),
    };

    let result = handle_node_control_frame(
        &state,
        &context,
        ControlFrame::EventBatch {
            frame_id: "oversized-event-batch".to_owned(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
            events: vec![event; MAX_EVENT_BATCH_ITEMS + 1],
        },
    )
    .await;

    assert!(matches!(
        result,
        Err(AppError::BadRequest {
            code: "control.event_batch_too_large",
            ..
        })
    ));
}

#[tokio::test]
async fn deeply_nested_control_payload_is_rejected_before_command_lookup() {
    let state = test_state().await;
    let node_id = NodeId::from("deep-control-node");
    let (context, _receiver) = activate_test_connection(&state, node_id).await;
    let mut nested = json!(null);
    for _ in 0..=MAX_CONTROL_JSON_DEPTH {
        nested = json!({ "nested": nested });
    }

    let result = handle_node_control_frame(
        &state,
        &context,
        ControlFrame::CommandResult {
            frame_id: "deep-control-frame".to_owned(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
            command_id: CommandId::from("not-looked-up"),
            status: CommandState::Completed,
            payload: JsonValue(nested),
        },
    )
    .await;

    assert!(matches!(
        result,
        Err(AppError::BadRequest {
            code: "control.frame_too_deep",
            ..
        })
    ));
}

#[tokio::test]
async fn cross_node_runtime_event_is_rejected_before_persistence() {
    let state = test_state().await;
    let (owner, detail, workspace_path) = create_test_session(&state).await;
    let attacker = enroll_test_node(&state)
        .await
        .node_id
        .expect("attacker node id returned");
    let (context, _receiver) = activate_test_connection(&state, attacker.clone()).await;
    let mut forged = node_event_fixture(
        &detail,
        attacker,
        "cross-node-runtime-event",
        1,
        EventKind::ProviderActivity,
        json!({}),
    );
    forged.actor_ref = ActorRef::Provider {
        provider: "codex".to_owned(),
    };

    let result = handle_node_control_frame(
        &state,
        &context,
        ControlFrame::EventBatch {
            frame_id: "cross-node-event-batch".to_owned(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
            events: vec![forged.clone()],
        },
    )
    .await;
    let event_count: i64 = sqlx::query_scalar("select count(*) from events where event_id = ?1")
        .bind(forged.event_id.as_str())
        .fetch_one(&state.pool)
        .await
        .expect("forged event count loads");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert_ne!(owner, context.node_id);
    assert!(matches!(
        result,
        Err(AppError::Auth {
            code: "control.event_runtime_mismatch",
            ..
        })
    ));
    assert_eq!(event_count, 0);
}

#[tokio::test]
async fn cross_node_terminal_output_is_rejected_before_broadcast() {
    let state = test_state().await;
    let (_owner, detail, workspace_path) = create_test_session(&state).await;
    let attacker = enroll_test_node(&state)
        .await
        .node_id
        .expect("attacker node id returned");
    let terminal_id = TerminalId::from("cross-node-terminal");
    state.workspace_terminals.write().await.insert(
        terminal_id.to_string(),
        WorkspaceTerminalSummary {
            placement_id: detail.placement.project_placement_id.clone(),
            terminal_id: terminal_id.clone(),
            title: "test".to_owned(),
            cwd: "/tmp".to_owned(),
            shell: "sh".to_owned(),
            cols: 80,
            rows: 24,
            state: WorkspaceTerminalState::Running,
            exit_code: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        },
    );
    let (context, _receiver) = activate_test_connection(&state, attacker).await;
    let mut terminal_rx = state.terminal_hub.subscribe(&terminal_id).await;

    let result = handle_node_control_frame(
        &state,
        &context,
        ControlFrame::WorkspaceTerminalOutput {
            frame_id: "cross-node-terminal-output".to_owned(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
            terminal_id,
            seq: 1,
            data: "forged".to_owned(),
        },
    )
    .await;
    let broadcast = terminal_rx.try_recv();
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert!(matches!(
        result,
        Err(AppError::Auth {
            code: "control.terminal_owner_mismatch",
            ..
        })
    ));
    assert!(matches!(
        broadcast,
        Err(broadcast::error::TryRecvError::Empty)
    ));
}

#[tokio::test]
async fn incompatible_control_hello_sends_error_and_leaves_command_pending() {
    let state = test_state().await;
    let claim = enroll_test_node(&state).await;
    let node_id = claim.node_id.expect("node id returned");
    let command_id = CommandId::from("bad-hello-command-1");
    record_and_dispatch_command(&state, command_fixture(command_id.clone(), node_id.clone()))
        .await
        .expect("command records");
    let (tx, mut rx) = mpsc::channel(CONTROL_QUEUE_CAPACITY);
    let context = state.control_connections.context(node_id.clone(), tx);

    let error = handle_node_control_frame(
        &state,
        &context,
        ControlFrame::Hello {
            frame_id: "bad-hello-frame-1".to_owned(),
            protocol_version: "v0".to_owned(),
            sent_at: Utc::now(),
            node_id: node_id.clone(),
            daemon_version: "0.1.0".to_owned(),
            active_runtime_ids: vec![],
            actual_runtime_attempts: vec![],
        },
    )
    .await
    .expect_err("incompatible hello rejects");
    let frame = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("control error is sent")
        .expect("channel stays open");
    let command_state: String =
        sqlx::query_scalar("select state from commands where command_id = ?1")
            .bind(command_id.as_str())
            .fetch_one(&state.pool)
            .await
            .expect("command state loads");

    assert!(matches!(
        error,
        AppError::BadRequest {
            code: "control.protocol_incompatible",
            ..
        }
    ));
    assert!(matches!(
        frame,
        ControlFrame::ControlError { error, .. }
            if error.error_code == "control.protocol_incompatible" && !error.retryable
    ));
    assert_eq!(command_state, "pending_dispatch");
}

#[tokio::test]
async fn duplicate_node_event_does_not_duplicate_assistant_message() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    let event = node_event_fixture(
        &detail,
        node_id,
        "provider-completed-duplicate",
        1,
        EventKind::ProviderMessageCompleted,
        json!({ "content": "deduped" }),
    );
    let event_id = event.event_id.clone();

    accept_node_event(&state, event.clone())
        .await
        .expect("first event accepts");
    sqlx::query(
        "update events set projection_state = 'pending', projected_at = null where event_id = ?1",
    )
    .bind(event_id.as_str())
    .execute(&state.pool)
    .await
    .expect("projection is reset for replay");
    let mut conflicting_replay = event;
    conflicting_replay.payload = EventPayload::from_json(
        EventKind::ProviderMessageCompleted,
        json!({ "content": "must not replace original" }),
    );
    accept_node_event(&state, conflicting_replay)
        .await
        .expect("pending duplicate replays persisted event");
    let projection_state: (String, i64) = sqlx::query_as(
        "select projection_state, projection_attempts from events where event_id = ?1",
    )
    .bind(event_id.as_str())
    .fetch_one(&state.pool)
    .await
    .expect("projection state loads");
    let messages = load_messages(&state, &detail.session.session_thread_id)
        .await
        .expect("messages load");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    let duplicate_count = messages
        .iter()
        .filter(|message| message.role == MessageRole::Assistant && message.content == "deduped")
        .count();
    assert_eq!(duplicate_count, 1);
    assert_eq!(projection_state, ("projected".to_owned(), 1));
}

#[tokio::test]
async fn projection_completion_couples_state_and_publication_enqueue() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    let event = node_event_fixture(
        &detail,
        node_id,
        "projection-boundary-provider-completed",
        1,
        EventKind::ProviderMessageCompleted,
        json!({ "content": "boundary" }),
    );
    let scope_key = scope_key(&event.scope_ref);
    insert_event_record(&state, &scope_key, &event)
        .await
        .expect("event record inserts");

    let pending: (String, i64) = sqlx::query_as(
            "select projection_state, (select count(*) from event_publication_outbox where event_id = events.event_id) from events where event_id = ?1",
        )
        .bind(event.event_id.as_str())
        .fetch_one(&state.pool)
        .await
        .expect("pending boundary loads");
    // Node ingest must not expose an event for publication while its
    // projection is still pending.  The completion boundary below is the
    // first operation allowed to enqueue it.
    assert_eq!(pending, ("pending".to_owned(), 0));

    complete_event_projection(&state, &event)
        .await
        .expect("projection completion commits");
    let projected: (String, i64) = sqlx::query_as(
            "select projection_state, (select count(*) from event_publication_outbox where event_id = events.event_id) from events where event_id = ?1",
        )
        .bind(event.event_id.as_str())
        .fetch_one(&state.pool)
        .await
        .expect("projected boundary loads");
    assert_eq!(projected, ("projected".to_owned(), 1));

    // Repeating the completion is idempotent and does not duplicate the
    // publication row.
    complete_event_projection(&state, &event)
        .await
        .expect("repeated projection completion commits");
    let outbox_count: i64 =
        sqlx::query_scalar("select count(*) from event_publication_outbox where event_id = ?1")
            .bind(event.event_id.as_str())
            .fetch_one(&state.pool)
            .await
            .expect("outbox count loads");
    assert_eq!(outbox_count, 1);
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");
}

#[tokio::test]
async fn accepted_node_event_is_published_to_session_stream_bus() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    let event = node_event_fixture(
        &detail,
        node_id,
        "published-provider-completed-1",
        1,
        EventKind::ProviderMessageCompleted,
        json!({ "content": "published" }),
    );
    let expected_event_id = event.event_id.clone();
    let mut event_rx = state.event_tx.subscribe();
    drain_event_publication_outbox(&state)
        .await
        .expect("pre-existing outbox rows drain");
    while event_rx.try_recv().is_ok() {}

    accept_node_event(&state, event)
        .await
        .expect("node event accepts");
    let published = tokio::time::timeout(std::time::Duration::from_secs(1), event_rx.recv())
        .await
        .expect("event is published")
        .expect("event bus stays open");
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert_eq!(published.event_id, expected_event_id);
    assert!(event_matches_session_after_seq(
        &published,
        &detail.session.session_thread_id,
        0
    ));
}

#[tokio::test]
async fn event_publication_outbox_retries_without_duplicate_delivery() {
    let state = test_state().await;
    let (node_id, detail, workspace_path) = create_test_session(&state).await;
    let mut event_rx = state.event_tx.subscribe();
    drain_event_publication_outbox(&state)
        .await
        .expect("pre-existing outbox rows drain");
    while event_rx.try_recv().is_ok() {}
    drop(event_rx);
    let event = node_event_fixture(
        &detail,
        node_id,
        "outbox-retry-provider-completed",
        1,
        EventKind::ProviderMessageCompleted,
        json!({ "content": "outbox" }),
    );
    let event_id = event.event_id.clone();

    // With no subscribers the durable row remains pending and records the
    // failed publication attempt.
    accept_node_event(&state, event.clone())
        .await
        .expect("event accepts without subscribers");
    let pending: (i64, Option<String>) = sqlx::query_as(
        "select attempts, published_at from event_publication_outbox where event_id = ?1",
    )
    .bind(event_id.as_str())
    .fetch_one(&state.pool)
    .await
    .expect("pending outbox row loads");
    assert_eq!(pending.0, 1);
    assert!(pending.1.is_none());

    let mut event_rx = state.event_tx.subscribe();
    drain_event_publication_outbox(&state)
        .await
        .expect("outbox drains after subscriber joins");
    let published = tokio::time::timeout(std::time::Duration::from_secs(1), event_rx.recv())
        .await
        .expect("retry publishes")
        .expect("event bus stays open");
    assert_eq!(published.event_id, event_id);

    let published_at: Option<String> =
        sqlx::query_scalar("select published_at from event_publication_outbox where event_id = ?1")
            .bind(event_id.as_str())
            .fetch_one(&state.pool)
            .await
            .expect("published outbox row loads");
    assert!(published_at.is_some());

    // A projected duplicate is idempotent: its existing published row is
    // not re-enqueued and no second broadcast is emitted.
    accept_node_event(&state, event)
        .await
        .expect("duplicate event is idempotent");
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), event_rx.recv())
            .await
            .is_err()
    );
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");
}
