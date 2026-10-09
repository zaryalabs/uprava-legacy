use super::*;

#[tokio::test]
async fn prepare_command_dispatch_rejects_command_without_provider_metadata() {
    let config = config_fixture();
    let command = command_fixture("command-1", CommandKind::SendTurn);
    let mut local_state = NodeLocalState::default();

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;

    assert_eq!(outcome.status, CommandState::Failed);
    assert!(outcome.state_changed);
    assert_eq!(
        event_kinds(&outcome.events_to_send),
        vec![EventKind::RuntimeError]
    );
    assert_eq!(local_state.event_outbox.len(), 1);
    assert_eq!(local_state.runtime_seqs.get("runtime-1").copied(), Some(1));
    assert_eq!(
        local_state.runtime_states.get("runtime-1").copied(),
        Some(RuntimeSessionState::Error)
    );
    assert_eq!(active_runtime_count(&local_state), 0);
    assert_eq!(
        local_state.command_status.get("command-1").copied(),
        Some(CommandState::Failed)
    );
    assert_eq!(
        outcome.events_to_send[0]
            .payload
            .0
            .get("code")
            .and_then(serde_json::Value::as_str),
        Some("provider.missing")
    );
    assert!(outcome.events_to_send.iter().all(|event| event
        .correlation_id
        .as_ref()
        .is_some_and(|correlation_id| correlation_id.as_str() == "correlation-1")));
}

#[tokio::test]
async fn prepare_command_dispatch_replays_outbox_for_duplicate_command() {
    let config = config_fixture();
    let command = command_fixture("command-1", CommandKind::SendTurn);
    let mut local_state = NodeLocalState::default();
    let first = prepare_command_dispatch(&config, &mut local_state, &command).await;
    let first_event_ids = event_ids(&first.events_to_send);

    let second = prepare_command_dispatch(&config, &mut local_state, &command).await;

    assert_eq!(second.status, CommandState::Failed);
    assert!(!second.state_changed);
    assert_eq!(event_ids(&second.events_to_send), first_event_ids);
    assert_eq!(local_state.runtime_seqs.get("runtime-1").copied(), Some(1));
    assert_eq!(local_state.event_outbox.len(), 1);
}

#[tokio::test]
async fn node_local_state_replays_outbox_for_duplicate_command_after_restart() {
    let config = config_fixture();
    let command = command_fixture("command-1", CommandKind::SendTurn);
    let path = std::env::temp_dir().join(format!("uprava-node-{}.json", Uuid::new_v4()));
    let mut local_state = NodeLocalState::default();
    let first = prepare_command_dispatch(&config, &mut local_state, &command).await;
    let first_event_ids = event_ids(&first.events_to_send);
    local_state
        .save(&path)
        .expect("node state with outbox saves");

    let mut reloaded_state = NodeLocalState::load(&path).expect("node state reloads");
    let second = prepare_command_dispatch(&config, &mut reloaded_state, &command).await;
    std::fs::remove_file(path).expect("node state fixture is removed");

    assert_eq!(second.status, CommandState::Failed);
    assert!(!second.state_changed);
    assert_eq!(event_ids(&second.events_to_send), first_event_ids);
    assert_eq!(
        reloaded_state.runtime_seqs.get("runtime-1").copied(),
        Some(1)
    );
    assert_eq!(reloaded_state.event_outbox.len(), 1);
}

#[tokio::test]
async fn remove_acked_events_removes_only_accepted_event_ids() {
    let config = config_fixture();
    let command = command_fixture("command-1", CommandKind::SendTurn);
    let mut local_state = NodeLocalState::default();
    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;
    let accepted_event_id = outcome.events_to_send[0].event_id.clone();

    let removed = remove_acked_events(&mut local_state.event_outbox, &[accepted_event_id]);

    assert_eq!(removed, 1);
    assert_eq!(local_state.event_outbox.len(), 0);
    assert!(event_ids(&local_state.event_outbox).is_empty());
}

#[tokio::test]
async fn event_outbox_retention_emits_runtime_error_when_runtime_events_are_dropped() {
    let command = command_fixture("command-retention", CommandKind::SendTurn);
    let mut local_state = NodeLocalState {
        node_id: Some(NodeId::from("node-1")),
        ..NodeLocalState::default()
    };
    let runtime_session_id = RuntimeSessionId::from("runtime-1");
    for _ in 0..6 {
        let event = event_for_command(
            "codex",
            &command,
            &mut local_state.runtime_seqs,
            runtime_session_id.clone(),
            None,
            EventKind::RuntimeRunning,
            serde_json::json!({ "provider": "codex" }),
        );
        local_state.event_outbox.push(event);
    }

    let notices = enforce_event_outbox_retention(&mut local_state, 5);

    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].kind, EventKind::RuntimeError);
    assert_eq!(notices[0].seq, 7);
    assert_eq!(
        notices[0]
            .payload
            .0
            .get("code")
            .and_then(serde_json::Value::as_str),
        Some("node.event_outbox_retention_exceeded")
    );
    assert_eq!(local_state.event_outbox.len(), 5);
    assert_eq!(local_state.dropped_event_count, 1);
    assert_eq!(local_state.runtime_seqs.get("runtime-1").copied(), Some(7));
    assert_eq!(
        local_state.runtime_states.get("runtime-1").copied(),
        Some(RuntimeSessionState::Error)
    );
}

#[tokio::test]
async fn event_outbox_retention_drops_old_runtime_events_by_age() {
    let command = command_fixture("command-retention-age", CommandKind::SendTurn);
    let mut local_state = NodeLocalState {
        node_id: Some(NodeId::from("node-1")),
        ..NodeLocalState::default()
    };
    let runtime_session_id = RuntimeSessionId::from("runtime-age");
    let mut old_event = event_for_command(
        "codex",
        &command,
        &mut local_state.runtime_seqs,
        runtime_session_id.clone(),
        None,
        EventKind::RuntimeRunning,
        serde_json::json!({ "provider": "codex" }),
    );
    old_event.happened_at = Utc::now() - chrono::Duration::seconds(10);
    let recent_event = event_for_command(
        "codex",
        &command,
        &mut local_state.runtime_seqs,
        runtime_session_id,
        None,
        EventKind::RuntimeRunning,
        serde_json::json!({ "provider": "codex" }),
    );
    local_state.event_outbox.push(old_event);
    local_state.event_outbox.push(recent_event);

    let notices = enforce_event_outbox_retention_with_limits(
        &mut local_state,
        10,
        Duration::from_secs(1),
        usize::MAX,
    );

    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].kind, EventKind::RuntimeError);
    assert_eq!(local_state.dropped_event_count, 1);
    assert!(local_state
        .event_outbox
        .iter()
        .all(|event| event.happened_at >= Utc::now() - chrono::Duration::seconds(11)));
}

#[tokio::test]
async fn event_outbox_retention_drops_oldest_events_by_serialized_bytes() {
    let command = command_fixture("command-retention-bytes", CommandKind::SendTurn);
    let mut local_state = NodeLocalState {
        node_id: Some(NodeId::from("node-1")),
        ..NodeLocalState::default()
    };
    for index in 0..3 {
        let mut event = event_for_command(
            "codex",
            &command,
            &mut local_state.runtime_seqs,
            RuntimeSessionId::from("runtime-bytes"),
            None,
            EventKind::ProviderActivity,
            serde_json::json!({ "data": "x".repeat(1024), "index": index }),
        );
        event.runtime_session_id = None;
        event.scope_ref = ScopeRef::Node {
            node_id: NodeId::from("node-1"),
        };
        local_state.event_outbox.push(event);
    }
    let max_bytes = serialized_event_len(&local_state.event_outbox[2]) + 1;

    let notices =
        enforce_event_outbox_retention_with_limits(&mut local_state, 10, Duration::ZERO, max_bytes);
    let retained_bytes = local_state
        .event_outbox
        .iter()
        .map(serialized_event_len)
        .sum::<usize>();

    assert!(notices.is_empty());
    assert!(retained_bytes <= max_bytes);
    assert!(local_state.event_outbox.len() <= 1);
    assert!(local_state.dropped_event_count >= 2);
}

#[tokio::test]
async fn failed_command_dispatch_replays_failed_status_and_outbox_for_duplicate_command() {
    let config = config_fixture();
    let command = command_fixture("command-error", CommandKind::SendTurn);
    let mut local_state = NodeLocalState::default();
    let first = prepare_command_dispatch(&config, &mut local_state, &command).await;
    let first_event_ids = event_ids(&first.events_to_send);

    let second = prepare_command_dispatch(&config, &mut local_state, &command).await;

    assert_eq!(second.status, CommandState::Failed);
    assert!(!second.state_changed);
    assert_eq!(event_ids(&second.events_to_send), first_event_ids);
    assert_eq!(local_state.runtime_seqs.get("runtime-1").copied(), Some(1));
}

#[tokio::test]
async fn codex_resolve_approval_returns_runtime_to_ready() {
    let config = config_fixture();
    let mut command = command_fixture("command-resolve", CommandKind::ResolveApproval);
    command.payload = CommandPayload::ResolveApproval {
        approval_id: ApprovalId::from("approval-1"),
        provider_interaction_id: None,
        approved: true,
        message: Some("approved".to_owned()),
    };
    let mut local_state = NodeLocalState::default();
    local_state.runtime_transcripts.insert(
        "runtime-1".to_owned(),
        vec![ProviderTranscriptMessage {
            role: "user".to_owned(),
            content: "stale context".to_owned(),
        }],
    );
    local_state
        .runtime_providers
        .insert("runtime-1".to_owned(), "codex".to_owned());

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;

    assert_eq!(
        event_kinds(&outcome.events_to_send),
        vec![EventKind::ApprovalResolved, EventKind::RuntimeReady]
    );
    assert_eq!(local_state.runtime_seqs.get("runtime-1").copied(), Some(2));
}

#[tokio::test]
async fn stop_runtime_marks_runtime_inactive() {
    let config = config_fixture();
    let command = command_fixture("command-stop", CommandKind::StopRuntime);
    let mut local_state = NodeLocalState::default();
    local_state
        .runtime_states
        .insert("runtime-1".to_owned(), RuntimeSessionState::Ready);
    local_state
        .runtime_providers
        .insert("runtime-1".to_owned(), "codex".to_owned());

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;

    assert_eq!(
        event_kinds(&outcome.events_to_send),
        vec![EventKind::RuntimeStopped]
    );
    assert_eq!(
        local_state.runtime_states.get("runtime-1").copied(),
        Some(RuntimeSessionState::Stopped)
    );
    assert_eq!(active_runtime_count(&local_state), 0);
    assert!(active_runtime_ids(&local_state).is_empty());
}

#[test]
fn active_runtime_ids_are_sorted_and_include_only_live_states() {
    let mut local_state = NodeLocalState::default();
    local_state
        .runtime_states
        .insert("runtime-b".to_owned(), RuntimeSessionState::Blocked);
    local_state
        .runtime_states
        .insert("runtime-stopped".to_owned(), RuntimeSessionState::Stopped);
    local_state
        .runtime_states
        .insert("runtime-a".to_owned(), RuntimeSessionState::Running);
    local_state
        .runtime_states
        .insert("runtime-error".to_owned(), RuntimeSessionState::Error);

    let ids = active_runtime_ids(&local_state);

    assert_eq!(
        ids.iter().map(RuntimeSessionId::as_str).collect::<Vec<_>>(),
        vec!["runtime-a", "runtime-b"]
    );
}
