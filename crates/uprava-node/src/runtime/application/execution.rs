//! Command execution, event outbox and local runtime projections.

use super::super::*;

pub(crate) struct NodeLiveEventSink<'a> {
    pub(crate) runtime_states: &'a mut HashMap<String, RuntimeSessionState>,
    pub(crate) sender: ControlFrameSender,
}

impl<'a> NodeLiveEventSink<'a> {
    pub(crate) fn new(
        runtime_states: &'a mut HashMap<String, RuntimeSessionState>,
        sender: &ControlFrameSender,
    ) -> Self {
        Self {
            runtime_states,
            sender: sender.clone(),
        }
    }

    pub(crate) fn emit(&mut self, event: &EventEnvelope) {
        apply_runtime_state_projection_for_event(self.runtime_states, event);
        if let Err(error) = self.sender.try_send(ControlFrame::EventBatch {
            frame_id: Uuid::new_v4().to_string(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
            events: vec![event.clone()],
        }) {
            tracing::warn!(
                error = %error,
                event_kind = ?event.kind,
                correlation_id = ?event.correlation_id,
                "live provider event could not be queued; durable delivery will retry it"
            );
        }
    }
}

#[derive(Default)]
pub(crate) struct CommandExecutionContext<'a> {
    pub(crate) provider_mcp_access: Option<&'a ProviderMcpAccess>,
    pub(crate) live_sender: Option<&'a ControlFrameSender>,
    pub(crate) terminal_supervisor: Option<&'a TerminalSupervisor>,
    pub(crate) managed_supervisor: Option<&'a ManagedRuntimeSupervisor>,
    pub(crate) cancellation: Option<watch::Receiver<bool>>,
    pub(crate) state_store: Option<&'a NodeStateStore>,
}

#[cfg(test)]
pub(crate) async fn prepare_command_dispatch(
    config: &NodeConfig,
    local_state: &mut NodeLocalState,
    command: &CommandEnvelope,
) -> CommandDispatchOutcome {
    prepare_command_dispatch_with_live_socket(
        config,
        local_state,
        command,
        CommandExecutionContext::default(),
    )
    .await
}

pub(crate) async fn prepare_command_dispatch_with_live_socket(
    config: &NodeConfig,
    local_state: &mut NodeLocalState,
    command: &CommandEnvelope,
    execution: CommandExecutionContext<'_>,
) -> CommandDispatchOutcome {
    let CommandExecutionContext {
        provider_mcp_access,
        live_sender,
        terminal_supervisor,
        managed_supervisor,
        cancellation,
        state_store,
    } = execution;
    if !command.payload.matches_kind(command.kind) {
        return CommandDispatchOutcome {
            status: CommandState::Failed,
            events_to_send: vec![],
            result_payload: JsonValue(serde_json::json!({
                "error_code": "protocol.command_payload_mismatch",
                "message": "Command payload does not match its command kind",
                "retryable": false,
            })),
            state_changed: false,
        };
    }
    if let Some(status) = local_state
        .command_status
        .get(command.command_id.as_str())
        .copied()
    {
        return CommandDispatchOutcome {
            status,
            events_to_send: outbox_events_for_command(
                &local_state.event_outbox,
                &command.command_id,
            ),
            result_payload: local_state
                .command_result_payloads
                .get(command.command_id.as_str())
                .cloned()
                .unwrap_or_else(|| JsonValue(serde_json::json!({}))),
            state_changed: false,
        };
    }

    let result_payload = JsonValue(serde_json::json!({}));
    let events = match command.kind {
        CommandKind::ValidateWorkspace => {
            workspace_validation_events(config, command, &mut local_state.placement_seqs)
        }
        CommandKind::RefreshResourceSnapshot => {
            resource_snapshot_events(config, command, &mut local_state.placement_seqs)
        }
        CommandKind::ListWorkspaceTree => {
            let (status, payload) = workspace_tree_command_result(config, command);
            record_command_result_payload(local_state, command, status, &payload);
            return CommandDispatchOutcome {
                status,
                events_to_send: vec![],
                result_payload: payload,
                state_changed: true,
            };
        }
        CommandKind::ReadWorkspaceFile => {
            let (status, payload) = workspace_file_command_result(config, command);
            record_command_result_payload(local_state, command, status, &payload);
            return CommandDispatchOutcome {
                status,
                events_to_send: vec![],
                result_payload: payload,
                state_changed: true,
            };
        }
        CommandKind::WriteWorkspaceFile => {
            let (status, payload) = workspace_file_write_command_result(config, command);
            let events =
                causal_workspace_events(command, status, &payload, &mut local_state.placement_seqs);
            return workspace_command_outcome(local_state, command, status, payload, events);
        }
        CommandKind::RunWorkspaceCommand => {
            let (status, payload) = workspace_command_run_command_result(config, command).await;
            let events =
                causal_workspace_events(command, status, &payload, &mut local_state.placement_seqs);
            return workspace_command_outcome(local_state, command, status, payload, events);
        }
        CommandKind::ReadWorkspaceDiff => {
            let (status, payload) = workspace_diff_command_result(config, command).await;
            let events =
                causal_workspace_events(command, status, &payload, &mut local_state.placement_seqs);
            return workspace_command_outcome(local_state, command, status, payload, events);
        }
        CommandKind::OpenWorkspaceTerminal => {
            let (status, payload) = workspace_terminal_open_command_result(
                config,
                command,
                live_sender,
                terminal_supervisor,
            )
            .await;
            record_command_result_payload(local_state, command, status, &payload);
            return CommandDispatchOutcome {
                status,
                events_to_send: vec![],
                result_payload: payload,
                state_changed: true,
            };
        }
        CommandKind::RequestDeduction => {
            let deduction_id = match &command.payload {
                CommandPayload::RequestDeduction { package } => package.deduction_id.to_string(),
                _ => String::new(),
            };
            if local_state.cancelled_deductions.remove(&deduction_id) {
                let (status, payload) = deduction_error_payload(
                    command,
                    "cancelled",
                    "deduction.cancelled",
                    "Deduction was cancelled before provider execution",
                );
                record_command_result_payload(local_state, command, status, &payload);
                return CommandDispatchOutcome {
                    status,
                    events_to_send: vec![],
                    result_payload: payload,
                    state_changed: true,
                };
            }
            if cancellation
                .as_ref()
                .is_some_and(|receiver| *receiver.borrow())
            {
                let (status, payload) = deduction_error_payload(
                    command,
                    "cancelled",
                    "deduction.cancelled",
                    "Deduction was cancelled before provider execution",
                );
                record_command_result_payload(local_state, command, status, &payload);
                return CommandDispatchOutcome {
                    status,
                    events_to_send: vec![],
                    result_payload: payload,
                    state_changed: true,
                };
            }
            let provider_key =
                provider_for_command(local_state, command).unwrap_or_else(|| "unknown".to_owned());
            let workspace_path = workspace_path_for_command(local_state, command);
            let (status, payload) = RuntimeManager::for_provider(&provider_key, config)
                .execute_deduction(command, workspace_path.as_deref(), cancellation)
                .await;
            record_command_result_payload(local_state, command, status, &payload);
            return CommandDispatchOutcome {
                status,
                events_to_send: vec![],
                result_payload: payload,
                state_changed: true,
            };
        }
        CommandKind::CancelDeduction => {
            let CommandPayload::CancelDeduction { deduction_id } = &command.payload else {
                unreachable!("command payload kind was validated before dispatch")
            };
            remember_cancelled_deduction(
                &mut local_state.cancelled_deductions,
                deduction_id.to_string(),
            );
            let payload = JsonValue(serde_json::json!({
                "deduction_id": deduction_id.as_str(),
                "cancelled": true,
            }));
            record_command_result_payload(local_state, command, CommandState::Completed, &payload);
            return CommandDispatchOutcome {
                status: CommandState::Completed,
                events_to_send: vec![],
                result_payload: payload,
                state_changed: true,
            };
        }
        CommandKind::RunTask => {
            let (status, payload, events) =
                task_command_result(config, command, cancellation, live_sender, state_store).await;
            record_command_result_payload(local_state, command, status, &payload);
            local_state.event_outbox.extend(events.iter().cloned());
            let retention_notices =
                enforce_event_outbox_retention(local_state, MAX_EVENT_OUTBOX_EVENTS);
            let mut events_to_send = events;
            events_to_send.extend(retention_notices);
            return CommandDispatchOutcome {
                status,
                events_to_send,
                result_payload: payload,
                state_changed: true,
            };
        }
        CommandKind::CancelTaskRun => {
            let CommandPayload::CancelTaskRun { task_run_id } = &command.payload else {
                unreachable!("command payload kind was validated before dispatch")
            };
            let payload = JsonValue(serde_json::json!({
                "task_run_id": task_run_id,
                "cancellation_requested": true,
            }));
            record_command_result_payload(local_state, command, CommandState::Completed, &payload);
            return CommandDispatchOutcome {
                status: CommandState::Completed,
                events_to_send: vec![],
                result_payload: payload,
                state_changed: true,
            };
        }
        CommandKind::Tooling => {
            let CommandPayload::Tooling { command: tooling } = &command.payload else {
                unreachable!("command payload kind was validated before dispatch")
            };
            let (status, payload) =
                execute_tooling_command(config, local_state, tooling, cancellation).await;
            let durable_payload = durable_tooling_result_payload(&payload);
            record_command_result_payload(local_state, command, status, &durable_payload);
            return CommandDispatchOutcome {
                status,
                events_to_send: vec![],
                result_payload: payload,
                state_changed: true,
            };
        }
        _ => {
            let provider_key =
                provider_for_command(local_state, command).unwrap_or_else(|| "unknown".to_owned());
            if let Err(error) = remember_runtime_metadata(config, local_state, command) {
                runtime_workspace_error_events(
                    &provider_key,
                    command,
                    &mut local_state.runtime_seqs,
                    error,
                )
            } else {
                let provider_key = provider_for_command(local_state, command);
                let workspace_path = workspace_path_for_command(local_state, command);
                let execution_profile = execution_profile_for_command(local_state, command);
                let mut live_event_sink = live_sender
                    .map(|sender| NodeLiveEventSink::new(&mut local_state.runtime_states, sender));
                let events = if let Some(provider_key) = provider_key {
                    AgentRuntimeDriver::for_command(
                        &provider_key,
                        execution_profile,
                        config,
                        managed_supervisor,
                    )
                    .execute_command(
                        command,
                        &mut local_state.runtime_seqs,
                        workspace_path.as_deref(),
                        &mut local_state.runtime_transcripts,
                        &mut local_state.runtime_provider_resume_refs,
                        provider_mcp_access,
                        live_event_sink.as_mut(),
                        cancellation,
                        &mut local_state.managed_attempts,
                    )
                    .await
                } else {
                    missing_provider_events_for_command(command, &mut local_state.runtime_seqs)
                };
                events
            }
        }
    };
    let unsent_events = events.clone();
    apply_runtime_state_projection(local_state, &unsent_events);
    local_state
        .event_outbox
        .extend(unsent_events.iter().cloned());
    let retention_notices = enforce_event_outbox_retention(local_state, MAX_EVENT_OUTBOX_EVENTS);
    let status = command_status_for_events(&events);
    local_state
        .command_status
        .insert(command.command_id.to_string(), status);
    let mut events_to_send = events;
    events_to_send.extend(retention_notices);

    CommandDispatchOutcome {
        status,
        events_to_send,
        result_payload,
        state_changed: true,
    }
}

pub(crate) fn provider_mcp_access_failure_outcome(
    local_state: &mut NodeLocalState,
    command: &CommandEnvelope,
) -> CommandDispatchOutcome {
    let provider_key =
        provider_for_command(local_state, command).unwrap_or_else(|| "codex".to_owned());
    let events = command
        .target
        .runtime_session_id()
        .cloned()
        .map(|runtime_session_id| {
            vec![runtime_error_event(
                &provider_key,
                command,
                &mut local_state.runtime_seqs,
                runtime_session_id,
                match &command.payload {
                    CommandPayload::SendTurn { turn_id, .. } => Some(turn_id.clone()),
                    _ => None,
                },
                "provider.mcp_access_unavailable",
                "Uprava MCP access could not be issued for this turn",
            )]
        })
        .unwrap_or_default();
    apply_runtime_state_projection(local_state, &events);
    local_state.event_outbox.extend(events.iter().cloned());
    local_state
        .command_status
        .insert(command.command_id.to_string(), CommandState::Failed);
    CommandDispatchOutcome {
        status: CommandState::Failed,
        events_to_send: events,
        result_payload: JsonValue(serde_json::json!({
            "error_code": "provider.mcp_access_unavailable",
            "message": "Uprava MCP access could not be issued for this turn",
            "retryable": true,
        })),
        state_changed: true,
    }
}

pub(crate) fn record_command_result_payload(
    local_state: &mut NodeLocalState,
    command: &CommandEnvelope,
    status: CommandState,
    payload: &JsonValue,
) {
    local_state
        .command_status
        .insert(command.command_id.to_string(), status);
    local_state
        .command_result_payloads
        .insert(command.command_id.to_string(), payload.clone());
}

pub(crate) fn workspace_command_outcome(
    local_state: &mut NodeLocalState,
    command: &CommandEnvelope,
    status: CommandState,
    payload: JsonValue,
    events: Vec<EventEnvelope>,
) -> CommandDispatchOutcome {
    record_command_result_payload(local_state, command, status, &payload);
    local_state.event_outbox.extend(events.iter().cloned());
    let retention_notices = enforce_event_outbox_retention(local_state, MAX_EVENT_OUTBOX_EVENTS);
    let mut events_to_send = events;
    events_to_send.extend(retention_notices);
    CommandDispatchOutcome {
        status,
        events_to_send,
        result_payload: payload,
        state_changed: true,
    }
}

pub(crate) fn causal_workspace_events(
    command: &CommandEnvelope,
    status: CommandState,
    payload: &JsonValue,
    placement_seqs: &mut HashMap<String, i64>,
) -> Vec<EventEnvelope> {
    if status != CommandState::Completed {
        return vec![];
    }
    let mut event = match command.kind {
        CommandKind::WriteWorkspaceFile => {
            let Ok(result) =
                serde_json::from_value::<WorkspaceFileWriteResponse>(payload.0.clone())
            else {
                return vec![];
            };
            let mut event = placement_event_for_command(
                command,
                placement_seqs,
                result.placement_id.clone(),
                EventKind::WorkspaceFileWritten,
                payload.0.clone(),
            );
            event.result_refs = vec![
                UpravaRef::WorkspaceEdit {
                    edit_id: result.edit_id,
                    placement_id: Some(result.placement_id.clone()),
                    path: Some(result.path.clone()),
                },
                UpravaRef::File {
                    placement_id: result.placement_id,
                    path: result.path,
                    version: result.metadata.modified_at.map(|value| value.to_rfc3339()),
                },
            ];
            event
        }
        CommandKind::RunWorkspaceCommand => {
            let Ok(result) =
                serde_json::from_value::<WorkspaceCommandRunResponse>(payload.0.clone())
            else {
                return vec![];
            };
            let event_kind = if result.intent == WorkspaceCommandIntent::Check {
                EventKind::WorkspaceCheckCompleted
            } else {
                EventKind::WorkspaceCommandCompleted
            };
            let mut event = placement_event_for_command(
                command,
                placement_seqs,
                result.placement_id,
                event_kind,
                payload.0.clone(),
            );
            let terminal_command_ref = UpravaRef::TerminalCommand {
                terminal_command_id: result.terminal_command_id.clone(),
                terminal_id: None,
            };
            event.evidence_refs = vec![UpravaRef::TerminalOutputRange {
                terminal_command_id: result.terminal_command_id.clone(),
                range: TextRange {
                    start_line: Some(1),
                    end_line: None,
                    start_offset: Some(0),
                    end_offset: Some(
                        result
                            .stdout
                            .chars()
                            .count()
                            .saturating_add(result.stderr.chars().count())
                            as i64,
                    ),
                },
            }];
            event.result_refs = vec![terminal_command_ref];
            if result.intent == WorkspaceCommandIntent::Check {
                event.result_refs.push(UpravaRef::CheckResult {
                    check_run_id: result.terminal_command_id,
                    failure_id: (!result.success).then(|| "command_failed".to_owned()),
                });
            }
            event
        }
        CommandKind::ReadWorkspaceDiff => {
            let Ok(result) = serde_json::from_value::<WorkspaceDiffResponse>(payload.0.clone())
            else {
                return vec![];
            };
            let mut event = placement_event_for_command(
                command,
                placement_seqs,
                result.placement_id.clone(),
                EventKind::WorkspaceDiffObserved,
                payload.0.clone(),
            );
            event.result_refs = vec![UpravaRef::WorkspaceDiff {
                diff_id: result.diff_id.clone(),
                placement_id: result.placement_id,
            }];
            event
                .result_refs
                .extend(result.hunks.into_iter().map(|hunk| UpravaRef::DiffHunk {
                    diff_id: result.diff_id.clone(),
                    hunk_id: hunk.hunk_id,
                }));
            event
        }
        _ => return vec![],
    };
    event.cause_refs.push(UpravaRef::Command {
        command_id: command.command_id.clone(),
    });
    vec![event]
}

pub(crate) fn command_status_for_events(events: &[EventEnvelope]) -> CommandState {
    if events.is_empty() {
        return CommandState::Failed;
    }
    if events.iter().any(|event| {
        matches!(
            event.kind,
            EventKind::RuntimeError | EventKind::TurnInterrupted
        )
    }) {
        return CommandState::Failed;
    }
    CommandState::Completed
}

pub(crate) fn outbox_events_for_command(
    outbox: &[EventEnvelope],
    command_id: &CommandId,
) -> Vec<EventEnvelope> {
    outbox
        .iter()
        .filter(|event| {
            event
                .command_id
                .as_ref()
                .is_some_and(|event_command_id| event_command_id == command_id)
        })
        .cloned()
        .collect()
}

pub(crate) fn remove_acked_events(
    outbox: &mut Vec<EventEnvelope>,
    accepted_event_ids: &[EventId],
) -> usize {
    if outbox.is_empty() || accepted_event_ids.is_empty() {
        return 0;
    }
    let accepted_event_ids = accepted_event_ids
        .iter()
        .map(EventId::as_str)
        .collect::<HashSet<_>>();
    let original_len = outbox.len();
    outbox.retain(|event| !accepted_event_ids.contains(event.event_id.as_str()));
    original_len - outbox.len()
}

pub(crate) fn enforce_event_outbox_retention(
    local_state: &mut NodeLocalState,
    max_events: usize,
) -> Vec<EventEnvelope> {
    enforce_event_outbox_retention_with_limits(
        local_state,
        max_events,
        MAX_EVENT_OUTBOX_AGE,
        MAX_EVENT_OUTBOX_BYTES,
    )
}

pub(crate) fn enforce_event_outbox_retention_with_limits(
    local_state: &mut NodeLocalState,
    max_events: usize,
    max_age: Duration,
    max_bytes: usize,
) -> Vec<EventEnvelope> {
    let drop_count = event_outbox_retention_drop_count(
        &local_state.event_outbox,
        max_events,
        max_age,
        max_bytes,
    );
    if drop_count == 0 {
        return vec![];
    }

    local_state.dropped_event_count = local_state
        .dropped_event_count
        .saturating_add(drop_count as u64);
    let dropped = local_state
        .event_outbox
        .drain(0..drop_count)
        .collect::<Vec<_>>();
    let mut affected_runtimes =
        BTreeMap::<String, (RuntimeSessionId, Option<SessionThreadId>, Option<NodeId>)>::new();
    for event in dropped {
        let Some(runtime_session_id) = event.runtime_session_id.clone() else {
            continue;
        };
        affected_runtimes
            .entry(runtime_session_id.to_string())
            .or_insert_with(|| {
                (
                    runtime_session_id,
                    event.session_thread_id.clone(),
                    event.node_id.clone(),
                )
            });
    }

    let notices = affected_runtimes
        .into_values()
        .map(|(runtime_session_id, session_thread_id, event_node_id)| {
            runtime_outbox_retention_event(
                local_state,
                runtime_session_id,
                session_thread_id,
                event_node_id,
            )
        })
        .collect::<Vec<_>>();
    if notices.is_empty() {
        trim_event_outbox_to_limits(&mut local_state.event_outbox, max_events, max_bytes);
        return notices;
    }

    apply_runtime_state_projection(local_state, &notices);
    local_state.event_outbox.extend(notices.iter().cloned());
    trim_event_outbox_to_limits(&mut local_state.event_outbox, max_events, max_bytes);
    notices
}

pub(crate) fn event_outbox_retention_drop_count(
    outbox: &[EventEnvelope],
    max_events: usize,
    max_age: Duration,
    max_bytes: usize,
) -> usize {
    if outbox.is_empty() {
        return 0;
    }
    let cutoff = if max_age.is_zero() {
        None
    } else {
        chrono::Duration::from_std(max_age)
            .ok()
            .map(|age| Utc::now() - age)
    };
    let event_sizes = outbox.iter().map(serialized_event_len).collect::<Vec<_>>();
    let mut retained_bytes = event_sizes.iter().sum::<usize>();
    let mut dropped = 0usize;
    while dropped < outbox.len() {
        let retained_count = outbox.len() - dropped;
        let count_exceeded = max_events > 0 && retained_count > max_events;
        let age_exceeded = cutoff
            .as_ref()
            .is_some_and(|cutoff| outbox[dropped].happened_at < *cutoff);
        let bytes_exceeded = max_bytes > 0 && retained_bytes > max_bytes;
        if !(count_exceeded || age_exceeded || bytes_exceeded) {
            break;
        }
        retained_bytes = retained_bytes.saturating_sub(event_sizes[dropped]);
        dropped += 1;
    }
    dropped
}

pub(crate) fn trim_event_outbox_to_limits(
    outbox: &mut Vec<EventEnvelope>,
    max_events: usize,
    max_bytes: usize,
) {
    loop {
        let count_exceeded = max_events > 0 && outbox.len() > max_events;
        let bytes_exceeded =
            max_bytes > 0 && outbox.iter().map(serialized_event_len).sum::<usize>() > max_bytes;
        if !(count_exceeded || bytes_exceeded) {
            break;
        }
        if outbox.is_empty() {
            break;
        }
        outbox.remove(0);
    }
}

pub(crate) fn serialized_event_len(event: &EventEnvelope) -> usize {
    serde_json::to_vec(event)
        .map(|value| value.len())
        .unwrap_or(usize::MAX)
}

pub(crate) fn runtime_outbox_retention_event(
    local_state: &mut NodeLocalState,
    runtime_session_id: RuntimeSessionId,
    session_thread_id: Option<SessionThreadId>,
    event_node_id: Option<NodeId>,
) -> EventEnvelope {
    let seq = next_runtime_seq(&mut local_state.runtime_seqs, &runtime_session_id);
    let node_id = local_state.node_id.clone().or(event_node_id);
    EventEnvelope {
        event_id: EventId::new(),
        command_id: None,
        correlation_id: None,
        actor_ref: node_id
            .clone()
            .map(|node_id| ActorRef::Node { node_id })
            .unwrap_or(ActorRef::Unknown),
        scope_ref: ScopeRef::Runtime {
            runtime_session_id: runtime_session_id.clone(),
        },
        node_id,
        runtime_session_id: Some(runtime_session_id),
        session_thread_id,
        turn_id: None,
        seq,
        session_projection_seq: None,
        kind: EventKind::RuntimeError,
        happened_at: Utc::now(),
        source_refs: vec![],
        evidence_refs: vec![],
        cause_refs: vec![],
        result_refs: vec![],
        payload: EventPayload::from_json(
            EventKind::RuntimeError,
            serde_json::json!({
                "code": "node.event_outbox_retention_exceeded",
                "message": "Node dropped unacknowledged runtime events because local outbox retention was exceeded",
            }),
        ),
    }
}

pub(crate) fn remember_runtime_metadata(
    config: &NodeConfig,
    local_state: &mut NodeLocalState,
    command: &CommandEnvelope,
) -> Result<bool, WorkspaceInspectError> {
    let Some(runtime_session_id) = command.target.runtime_session_id() else {
        return Ok(false);
    };
    let runtime_key = runtime_session_id.to_string();
    let mut changed = false;
    let canonical_workspace_path = if matches!(
        command.kind,
        CommandKind::StartRuntime | CommandKind::ResumeRuntime
    ) {
        match command_payload_str(command, "workspace_path") {
            Some(workspace_path) => Some(canonical_workspace_root(config, workspace_path)?),
            None => None,
        }
    } else {
        None
    };

    if matches!(command.kind, CommandKind::StartRuntime) {
        if let Some(provider) = command_payload_str(command, "provider") {
            changed |= insert_if_changed(
                &mut local_state.runtime_providers,
                runtime_key.clone(),
                provider.to_owned(),
            );
        }
    } else if matches!(command.kind, CommandKind::ResumeRuntime) {
        if let Some(provider) = command_payload_str(command, "provider") {
            changed |= insert_if_changed(
                &mut local_state.runtime_providers,
                runtime_key.clone(),
                provider.to_owned(),
            );
        }
        if let Some(provider_resume_ref) = command_provider_resume_ref(command) {
            changed |= insert_if_changed(
                &mut local_state.runtime_provider_resume_refs,
                runtime_key.clone(),
                provider_resume_ref,
            );
        }
    }

    if matches!(
        command.kind,
        CommandKind::StartRuntime | CommandKind::ResumeRuntime
    ) {
        let execution_profile = match &command.payload {
            CommandPayload::StartRuntime {
                execution_profile, ..
            }
            | CommandPayload::ResumeRuntime {
                execution_profile, ..
            } => *execution_profile,
            _ => AgentExecutionProfile::ExecCompatibility,
        };
        changed |= insert_if_changed(
            &mut local_state.runtime_execution_profiles,
            runtime_key.clone(),
            execution_profile,
        );
        if let Some(workspace_path) = canonical_workspace_path {
            changed |= insert_if_changed(
                &mut local_state.runtime_workspace_paths,
                runtime_key,
                workspace_path.display().to_string(),
            );
        }
    }

    Ok(changed)
}

pub(crate) fn execution_profile_for_command(
    local_state: &NodeLocalState,
    command: &CommandEnvelope,
) -> AgentExecutionProfile {
    match &command.payload {
        CommandPayload::StartRuntime {
            execution_profile, ..
        }
        | CommandPayload::ResumeRuntime {
            execution_profile, ..
        } => *execution_profile,
        _ => command
            .target
            .runtime_session_id()
            .and_then(|runtime_id| {
                local_state
                    .runtime_execution_profiles
                    .get(runtime_id.as_str())
                    .copied()
            })
            .unwrap_or(AgentExecutionProfile::ExecCompatibility),
    }
}

pub(crate) fn provider_for_command(
    local_state: &NodeLocalState,
    command: &CommandEnvelope,
) -> Option<String> {
    command_payload_str(command, "provider")
        .map(str::to_owned)
        .or_else(|| {
            command
                .target
                .runtime_session_id()
                .and_then(|runtime_session_id| {
                    local_state
                        .runtime_providers
                        .get(runtime_session_id.as_str())
                        .cloned()
                })
        })
}

pub(crate) fn workspace_path_for_command(
    local_state: &NodeLocalState,
    command: &CommandEnvelope,
) -> Option<String> {
    command_payload_str(command, "workspace_path")
        .map(str::to_owned)
        .or_else(|| {
            command
                .target
                .runtime_session_id()
                .and_then(|runtime_session_id| {
                    local_state
                        .runtime_workspace_paths
                        .get(runtime_session_id.as_str())
                        .cloned()
                })
        })
}

pub(crate) fn command_payload_str<'a>(command: &'a CommandEnvelope, key: &str) -> Option<&'a str> {
    match key {
        "provider" => command.payload.provider(),
        "workspace_path" => command.payload.workspace_path(),
        "display_name" => command.payload.display_name(),
        "path" => command.payload.path(),
        _ => None,
    }
    .map(str::trim)
    .filter(|value| !value.is_empty())
}

pub(crate) fn insert_if_changed<T: PartialEq>(
    map: &mut HashMap<String, T>,
    key: String,
    value: T,
) -> bool {
    if map.get(&key) == Some(&value) {
        return false;
    }
    map.insert(key, value);
    true
}

pub(crate) fn command_provider_resume_ref(command: &CommandEnvelope) -> Option<ProviderResumeRef> {
    command
        .payload
        .provider_resume_ref()
        .map(|value| &value.0)
        .and_then(provider_resume_ref_from_json)
}

pub(crate) fn provider_resume_ref_from_json(
    value: &serde_json::Value,
) -> Option<ProviderResumeRef> {
    let provider_session_id = value
        .get("provider_session_id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| bounded_text(value, 512));
    let resume_cursor = value
        .get("resume_cursor")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| bounded_text(value, 512));
    if provider_session_id.is_none() && resume_cursor.is_none() {
        None
    } else {
        Some(ProviderResumeRef {
            provider_session_id,
            resume_cursor,
        })
    }
}

pub(crate) fn provider_resume_ref_json(resume_ref: &ProviderResumeRef) -> serde_json::Value {
    let mut value = serde_json::Map::new();
    if let Some(provider_session_id) = &resume_ref.provider_session_id {
        value.insert(
            "provider_session_id".to_owned(),
            serde_json::Value::String(provider_session_id.clone()),
        );
    }
    if let Some(resume_cursor) = &resume_ref.resume_cursor {
        value.insert(
            "resume_cursor".to_owned(),
            serde_json::Value::String(resume_cursor.clone()),
        );
    }
    serde_json::Value::Object(value)
}

pub(crate) fn apply_runtime_state_projection(
    local_state: &mut NodeLocalState,
    events: &[EventEnvelope],
) {
    for event in events {
        apply_runtime_state_projection_for_event(&mut local_state.runtime_states, event);
    }
}

pub(crate) fn apply_runtime_state_projection_for_event(
    runtime_states: &mut HashMap<String, RuntimeSessionState>,
    event: &EventEnvelope,
) {
    let Some(runtime_session_id) = &event.runtime_session_id else {
        return;
    };
    let Some(state) = runtime_state_for_event(event.kind) else {
        return;
    };
    runtime_states.insert(runtime_session_id.to_string(), state);
}

pub(crate) fn runtime_state_for_event(kind: EventKind) -> Option<RuntimeSessionState> {
    match kind {
        EventKind::RuntimeStarting => Some(RuntimeSessionState::Starting),
        EventKind::RuntimeReady => Some(RuntimeSessionState::Ready),
        EventKind::RuntimeRunning => Some(RuntimeSessionState::Running),
        EventKind::RuntimeBlocked => Some(RuntimeSessionState::Blocked),
        EventKind::RuntimeExpired => Some(RuntimeSessionState::Expired),
        EventKind::RuntimeResuming => Some(RuntimeSessionState::Resuming),
        EventKind::RuntimeStopped => Some(RuntimeSessionState::Stopped),
        EventKind::RuntimeError => Some(RuntimeSessionState::Error),
        EventKind::TurnInterrupted => Some(RuntimeSessionState::Interrupted),
        _ => None,
    }
}

pub(crate) fn active_runtime_count(local_state: &NodeLocalState) -> i64 {
    active_runtime_ids(local_state).len() as i64
}

pub(crate) fn active_runtime_ids(local_state: &NodeLocalState) -> Vec<RuntimeSessionId> {
    let mut ids = local_state
        .runtime_states
        .iter()
        .filter(|(_, state)| is_active_runtime_state(**state))
        .map(|(runtime_session_id, _)| RuntimeSessionId::from(runtime_session_id.as_str()))
        .collect::<Vec<_>>();
    ids.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    ids
}

pub(crate) fn is_active_runtime_state(state: RuntimeSessionState) -> bool {
    matches!(
        state,
        RuntimeSessionState::Starting
            | RuntimeSessionState::Ready
            | RuntimeSessionState::Running
            | RuntimeSessionState::Blocked
            | RuntimeSessionState::Stopping
            | RuntimeSessionState::Interrupted
            | RuntimeSessionState::Resuming
    )
}
