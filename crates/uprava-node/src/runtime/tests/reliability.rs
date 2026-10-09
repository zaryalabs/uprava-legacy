use super::*;

#[test]
fn command_execution_key_prefers_runtime_then_placement() {
    let runtime_command = command_fixture("command-runtime", CommandKind::SendTurn);
    assert_eq!(command_execution_key(&runtime_command), "runtime:runtime-1");

    let mut placement_command =
        command_fixture("command-placement", CommandKind::RunWorkspaceCommand);
    placement_command.target = CommandTarget::Placement {
        node_id: NodeId::from("node-1"),
        project_placement_id: ProjectPlacementId::from("placement-1"),
    };
    assert_eq!(
        command_execution_key(&placement_command),
        "placement:placement-1"
    );

    let mut command = command_fixture("command-standalone", CommandKind::ValidateWorkspace);
    command.target = CommandTarget::Node {
        node_id: NodeId::from("node-1"),
    };
    assert_eq!(
        command_execution_key(&command),
        "command:command-standalone"
    );
}

#[test]
fn interrupt_and_stop_never_wait_on_the_active_runtime_execution_lock() {
    let send = command_fixture("command-send", CommandKind::SendTurn);
    let interrupt = command_fixture("command-interrupt", CommandKind::InterruptRuntime);
    let stop = command_fixture("command-stop", CommandKind::StopRuntime);

    assert_eq!(command_execution_key(&send), "runtime:runtime-1");
    assert_eq!(
        command_execution_key(&interrupt),
        "priority-command:command-interrupt"
    );
    assert_eq!(
        command_execution_key(&stop),
        "priority-command:command-stop"
    );
}

#[test]
fn priority_runtime_events_rebase_after_the_active_command_finishes() {
    let command = command_fixture("command-interrupt", CommandKind::InterruptRuntime);
    let runtime_id = RuntimeSessionId::from("runtime-1");
    let mut local_state = NodeLocalState::default();
    let first = event_for_command(
        "codex",
        &command,
        &mut local_state.runtime_seqs,
        runtime_id.clone(),
        Some(TurnId::from("turn-1")),
        EventKind::TurnInterrupted,
        serde_json::json!({}),
    );
    let second = event_for_command(
        "codex",
        &command,
        &mut local_state.runtime_seqs,
        runtime_id,
        None,
        EventKind::RuntimeReady,
        serde_json::json!({}),
    );
    local_state.event_outbox = vec![first.clone(), second.clone()];
    let mut events = vec![first, second];
    let current = NodeLocalState {
        runtime_seqs: HashMap::from([("runtime-1".to_owned(), 41)]),
        ..NodeLocalState::default()
    };

    rebase_priority_runtime_events(&mut local_state, &mut events, &current, &command);

    assert_eq!(
        events.iter().map(|event| event.seq).collect::<Vec<_>>(),
        [42, 43]
    );
    assert_eq!(
        local_state
            .event_outbox
            .iter()
            .map(|event| event.seq)
            .collect::<Vec<_>>(),
        [42, 43]
    );
    assert_eq!(local_state.runtime_seqs.get("runtime-1"), Some(&43));
}

#[tokio::test]
async fn command_dispatch_rejects_payload_kind_mismatch_before_execution() {
    let config = config_fixture();
    let mut command = command_fixture("command-mismatch", CommandKind::SendTurn);
    command.payload = CommandPayload::StopRuntime {
        runtime_attempt_id: None,
        reason: None,
    };
    let mut local_state = NodeLocalState::default();

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;

    assert_eq!(outcome.status, CommandState::Failed);
    assert_eq!(
        outcome.result_payload.0["error_code"],
        "protocol.command_payload_mismatch"
    );
    assert!(!outcome.state_changed);
}

#[tokio::test]
async fn dispatch_busy_result_is_retryable_failed_command_result() {
    let (sender, mut receiver) = mpsc::channel(4);
    let command = command_fixture("command-busy", CommandKind::SendTurn);

    send_dispatch_busy_result(&sender, &command)
        .await
        .expect("busy result sends");

    let ack = receiver.recv().await.expect("ack frame");
    let ControlFrame::CommandAck {
        command_id, status, ..
    } = ack
    else {
        panic!("expected command ack");
    };
    assert_eq!(command_id, command.command_id);
    assert_eq!(status, CommandState::Acknowledged);

    let result = receiver.recv().await.expect("result frame");
    let ControlFrame::CommandResult {
        command_id,
        status,
        payload,
        ..
    } = result
    else {
        panic!("expected command result");
    };
    assert_eq!(command_id, command.command_id);
    assert_eq!(status, CommandState::Failed);
    assert_eq!(
        payload
            .0
            .get("error_code")
            .and_then(serde_json::Value::as_str),
        Some("node.dispatch_busy")
    );
    assert_eq!(
        payload
            .0
            .get("retryable")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
}

#[tokio::test]
async fn send_frame_reports_saturated_writer_queue() {
    let (sender, _receiver) = mpsc::channel(1);
    send_frame(
        &sender,
        ControlFrame::Pong {
            frame_id: "frame-1".to_owned(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
        },
    )
    .await
    .expect("first frame fits queue");

    let error = send_frame(
        &sender,
        ControlFrame::Pong {
            frame_id: "frame-2".to_owned(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
        },
    )
    .await
    .expect_err("second frame reports saturation");
    assert!(error.to_string().contains("control frame send failed"));
}

#[test]
fn append_capped_process_line_bounds_output_bytes() {
    let mut output = Vec::new();
    let mut truncated = false;

    append_capped_process_line(&mut output, "abcdef", 5, &mut truncated);

    assert_eq!(output, b"abcde");
    assert!(truncated);

    append_capped_process_line(&mut output, "ignored", 5, &mut truncated);

    assert_eq!(output, b"abcde");
}

#[cfg(unix)]
#[test]
fn codex_process_limit_event_records_output_truncation() {
    use std::os::unix::process::ExitStatusExt;

    let command = command_fixture("command-limits", CommandKind::SendTurn);
    let output = CodexProcessOutput {
        status: ExitStatus::from_raw(0),
        stdout: b"partial".to_vec(),
        stderr: Vec::new(),
        stdout_truncated: true,
        stderr_truncated: false,
        dropped_activity_count: 7,
        approval_requests: vec![],
        provider_resume_ref: None,
        activity_events: vec![],
    };
    let mut runtime_seqs = HashMap::new();
    let mut events = Vec::new();

    append_codex_process_limit_events(
        "codex",
        &command,
        &mut runtime_seqs,
        RuntimeSessionId::from("runtime-1"),
        Some(TurnId::from("turn-1")),
        &output,
        &mut events,
    );

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, EventKind::ProviderActivity);
    assert_eq!(
        events[0]
            .payload
            .0
            .get("provider_event_type")
            .and_then(serde_json::Value::as_str),
        Some("output_truncated")
    );
    assert_eq!(
        events[0]
            .payload
            .0
            .get("dropped_activity_count")
            .and_then(serde_json::Value::as_i64),
        Some(7)
    );
}

#[cfg(unix)]
#[test]
fn codex_resume_ref_uses_incremental_parse_result() {
    use std::os::unix::process::ExitStatusExt;

    let output = CodexProcessOutput {
        status: ExitStatus::from_raw(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
        stdout_truncated: true,
        stderr_truncated: false,
        dropped_activity_count: 0,
        approval_requests: vec![],
        provider_resume_ref: Some(serde_json::json!({
            "provider_session_id": "session-incremental",
        })),
        activity_events: vec![],
    };

    assert_eq!(
        codex_resume_ref_from_output(&output)
            .and_then(|value| value.get("provider_session_id").cloned())
            .and_then(|value| value.as_str().map(str::to_owned)),
        Some("session-incremental".to_owned())
    );
}

#[test]
fn command_state_merge_preserves_registration_and_replaces_runtime_fields() {
    let mut shared = NodeLocalState {
        node_id: Some(NodeId::from("node-owner")),
        credential: Some("credential-owner".to_owned()),
        ..NodeLocalState::default()
    };
    let command_state = NodeLocalState {
        node_id: Some(NodeId::from("stale-command-copy")),
        credential: Some("stale-credential".to_owned()),
        runtime_seqs: HashMap::from([("runtime-1".to_owned(), 4)]),
        ..NodeLocalState::default()
    };

    let baseline = shared.clone();
    shared.merge_command_state_from(&baseline, &command_state);

    assert_eq!(shared.node_id, Some(NodeId::from("node-owner")));
    assert_eq!(shared.credential.as_deref(), Some("credential-owner"));
    assert_eq!(shared.runtime_seqs.get("runtime-1"), Some(&4));
}

#[tokio::test]
async fn state_store_serializes_command_merges_and_preserves_registration() {
    let path = std::env::temp_dir().join(format!("uprava-node-store-{}.sqlite", Uuid::new_v4()));
    let mut initial = NodeLocalState {
        node_id: Some(NodeId::from("node-owner")),
        credential: Some("credential-owner".to_owned()),
        ..NodeLocalState::default()
    };
    let event = runtime_outbox_retention_event(
        &mut initial,
        RuntimeSessionId::from("runtime-store"),
        None,
        None,
    );
    let event_id = event.event_id.clone();
    initial.event_outbox.push(event);
    initial
        .save_async(&path)
        .await
        .expect("initial state persists");
    let store = NodeStateStore::new(initial, path.clone());
    let baseline = store.snapshot().await.expect("state snapshot");
    let first = NodeLocalState {
        node_id: Some(NodeId::from("stale-first-copy")),
        command_status: HashMap::from([("command-1".to_owned(), CommandState::Completed)]),
        event_outbox: baseline.event_outbox.clone(),
        ..NodeLocalState::default()
    };
    let second = NodeLocalState {
        node_id: Some(NodeId::from("stale-second-copy")),
        command_status: HashMap::from([("command-2".to_owned(), CommandState::Failed)]),
        event_outbox: baseline.event_outbox.clone(),
        ..NodeLocalState::default()
    };

    let (first_result, second_result) = tokio::join!(
        store.merge_command_state(&baseline, &first),
        store.merge_command_state(&baseline, &second)
    );
    first_result.expect("first command merge persists");
    second_result.expect("second command merge persists");

    let merged = store.snapshot().await.expect("state snapshot");
    assert_eq!(merged.node_id, Some(NodeId::from("node-owner")));
    assert_eq!(merged.credential.as_deref(), Some("credential-owner"));
    assert!(merged.command_status.contains_key("command-1"));
    assert!(merged.command_status.contains_key("command-2"));
    assert!(merged
        .event_outbox
        .iter()
        .any(|event| event.event_id == event_id));

    let stale = baseline.clone();
    let (ack_result, merge_result) = tokio::join!(
        store.persist_event_ack(std::slice::from_ref(&event_id)),
        store.merge_command_state(&baseline, &stale)
    );
    ack_result.expect("event ACK persists");
    merge_result.expect("stale command merge persists");
    let merged = store.snapshot().await.expect("state snapshot");
    assert!(merged
        .event_outbox
        .iter()
        .all(|event| event.event_id != event_id));
    assert!(merged.command_status.contains_key("command-1"));
    let reopened = NodeLocalState::load_async(&path)
        .await
        .expect("sqlite state reopens");
    assert!(reopened
        .event_outbox
        .iter()
        .all(|event| event.event_id != event_id));
    let pool = open_state_store(&path).await.expect("sqlite store opens");
    let outbox_rows: i64 = sqlx::query_scalar("select count(*) from node_event_outbox")
        .fetch_one(&pool)
        .await
        .expect("outbox rows query");
    pool.close().await;
    assert_eq!(outbox_rows, 0);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn state_store_persists_command_outcome_before_result_delivery() {
    let path = std::env::temp_dir().join(format!(
        "uprava-node-command-outcome-{}.sqlite",
        Uuid::new_v4()
    ));
    let baseline = NodeLocalState {
        node_id: Some(NodeId::from("node-owner")),
        credential: Some("credential-owner".to_owned()),
        ..NodeLocalState::default()
    };
    baseline
        .clone()
        .save_async(&path)
        .await
        .expect("baseline state persists");
    let store = NodeStateStore::new(baseline.clone(), path.clone());
    let mut command_state = baseline.clone();
    let command_id = "command-outcome";
    command_state
        .command_status
        .insert(command_id.to_owned(), CommandState::Completed);
    command_state.command_result_payloads.insert(
        command_id.to_owned(),
        JsonValue(serde_json::json!({"ok": true})),
    );
    let event = runtime_outbox_retention_event(
        &mut command_state,
        RuntimeSessionId::from("runtime-command-outcome"),
        None,
        None,
    );
    let event_id = event.event_id.clone();
    command_state.event_outbox.push(event);

    store
        .persist_command_outcome(&baseline, &command_state)
        .await
        .expect("command outcome persists");

    let reloaded = NodeLocalState::load_async(&path)
        .await
        .expect("persisted outcome reloads");
    assert_eq!(
        reloaded.command_status.get(command_id),
        Some(&CommandState::Completed)
    );
    assert_eq!(
        reloaded.command_result_payloads.get(command_id),
        Some(&JsonValue(serde_json::json!({"ok": true})))
    );
    assert!(reloaded
        .event_outbox
        .iter()
        .any(|event| event.event_id == event_id));
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn state_store_command_merge_propagates_removals_without_clobbering_newer_values() {
    let baseline = NodeLocalState {
        runtime_provider_resume_refs: HashMap::from([(
            "runtime-remove".to_owned(),
            ProviderResumeRef {
                provider_session_id: Some("session-old".to_owned()),
                resume_cursor: Some("cursor-old".to_owned()),
            },
        )]),
        ..NodeLocalState::default()
    };
    let command_state = NodeLocalState {
        // The command snapshot explicitly removed the resume reference.
        ..baseline.clone()
    };
    let mut command_state = command_state;
    command_state.runtime_provider_resume_refs.clear();

    let removal_path = std::env::temp_dir().join(format!(
        "uprava-node-store-removal-{}.sqlite",
        Uuid::new_v4()
    ));
    baseline
        .clone()
        .save_async(&removal_path)
        .await
        .expect("baseline removal state persists");
    let removal_store = NodeStateStore::new(baseline.clone(), removal_path.clone());
    removal_store
        .merge_command_state(&baseline, &command_state)
        .await
        .expect("removal merge persists");
    assert!(!removal_store
        .snapshot()
        .await
        .expect("state snapshot")
        .runtime_provider_resume_refs
        .contains_key("runtime-remove"));
    let removal_reopened = NodeLocalState::load_async(&removal_path)
        .await
        .expect("removal state reopens");
    assert!(!removal_reopened
        .runtime_provider_resume_refs
        .contains_key("runtime-remove"));

    let newer_path = std::env::temp_dir().join(format!(
        "uprava-node-store-newer-ref-{}.sqlite",
        Uuid::new_v4()
    ));
    let newer_ref = ProviderResumeRef {
        provider_session_id: Some("session-new".to_owned()),
        resume_cursor: Some("cursor-new".to_owned()),
    };
    let newer_owner = NodeLocalState {
        runtime_provider_resume_refs: HashMap::from([(
            "runtime-remove".to_owned(),
            newer_ref.clone(),
        )]),
        ..NodeLocalState::default()
    };
    newer_owner
        .clone()
        .save_async(&newer_path)
        .await
        .expect("newer owner state persists");
    let newer_store = NodeStateStore::new(newer_owner, newer_path.clone());
    newer_store
        .merge_command_state(&baseline, &command_state)
        .await
        .expect("stale removal merge persists");
    assert_eq!(
        newer_store
            .snapshot()
            .await
            .expect("state snapshot")
            .runtime_provider_resume_refs
            .get("runtime-remove"),
        Some(&newer_ref)
    );
    let newer_reopened = NodeLocalState::load_async(&newer_path)
        .await
        .expect("newer state reopens");
    assert_eq!(
        newer_reopened
            .runtime_provider_resume_refs
            .get("runtime-remove"),
        Some(&newer_ref)
    );

    let _ = std::fs::remove_file(removal_path);
    let _ = std::fs::remove_file(newer_path);
}

#[test]
fn live_event_sink_only_records_until_durable_dispatch_phase() {
    let mut local_state = NodeLocalState::default();
    let event = runtime_outbox_retention_event(
        &mut local_state,
        RuntimeSessionId::from("runtime-durable-live-event"),
        None,
        None,
    );
    let mut runtime_states = HashMap::new();
    let (sender, mut receiver) = mpsc::channel(1);
    let mut sink = NodeLiveEventSink::new(&mut runtime_states, &sender);

    sink.emit(&event);

    assert_eq!(
        runtime_states.get("runtime-durable-live-event"),
        Some(&RuntimeSessionState::Error)
    );
    let frame = receiver.try_recv().expect("live event queued");
    assert!(matches!(frame, ControlFrame::EventBatch { events, .. } if events == vec![event]));
}
