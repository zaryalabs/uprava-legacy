//! Bounded command dispatch, cancellation and durable result delivery.

use super::super::*;

#[derive(Debug)]
pub(crate) struct CommandDispatchJob {
    pub(crate) command: CommandEnvelope,
}

#[derive(Clone, Default)]
pub(crate) struct CommandExecutionLocks {
    pub(crate) locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl CommandExecutionLocks {
    pub(crate) async fn lock_for(&self, key: String) -> Arc<Mutex<()>> {
        let mut locks = self.locks.lock().await;
        locks
            .entry(key)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

#[derive(Clone, Default)]
pub(crate) struct ExecutionCancellationRegistry {
    pub(crate) state: Arc<Mutex<ExecutionCancellationState>>,
}

#[derive(Default)]
pub(crate) struct ExecutionCancellationState {
    pub(crate) senders: HashMap<String, watch::Sender<bool>>,
    pub(crate) pending: HashSet<String>,
}

pub(crate) struct ExecutionCancellationGuard {
    pub(crate) key: String,
    pub(crate) sender: watch::Sender<bool>,
    pub(crate) receiver: watch::Receiver<bool>,
}

impl ExecutionCancellationRegistry {
    pub(crate) async fn begin(&self, key: String) -> ExecutionCancellationGuard {
        let mut state = self.state.lock().await;
        let initially_cancelled = state.pending.remove(&key);
        let (sender, receiver) = watch::channel(initially_cancelled);
        state.senders.insert(key.clone(), sender.clone());
        ExecutionCancellationGuard {
            key,
            sender,
            receiver,
        }
    }

    pub(crate) async fn cancel(&self, key: String, remember_if_pending: bool) -> bool {
        let mut state = self.state.lock().await;
        if let Some(sender) = state.senders.get(&key) {
            return sender.send(true).is_ok();
        }
        if remember_if_pending {
            state.pending.insert(key);
        }
        false
    }

    pub(crate) async fn finish(&self, guard: ExecutionCancellationGuard) {
        let mut state = self.state.lock().await;
        if state
            .senders
            .get(&guard.key)
            .is_some_and(|sender| sender.same_channel(&guard.sender))
        {
            state.senders.remove(&guard.key);
        }
    }
}

impl ExecutionCancellationGuard {
    pub(crate) fn receiver(&self) -> watch::Receiver<bool> {
        self.receiver.clone()
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "dispatcher composition passes independent process supervisors and bounded priority queues"
)]
pub(crate) async fn run_command_dispatcher(
    config: NodeConfig,
    client: reqwest::Client,
    shared_state: NodeStateStore,
    sender: ControlFrameSender,
    terminal_supervisor: TerminalSupervisor,
    managed_supervisor: ManagedRuntimeSupervisor,
    mut priority_receiver: mpsc::Receiver<CommandDispatchJob>,
    mut receiver: mpsc::Receiver<CommandDispatchJob>,
) {
    let shared = CommandDispatcherShared {
        config,
        client,
        shared_state,
        sender,
        terminal_supervisor,
        managed_supervisor,
        locks: CommandExecutionLocks::default(),
        cancellations: ExecutionCancellationRegistry::default(),
        concurrency: Arc::new(Semaphore::new(NODE_COMMAND_DISPATCH_CONCURRENCY)),
    };
    let mut tasks = tokio::task::JoinSet::new();

    loop {
        tokio::select! {
            biased;
            Some(job) = priority_receiver.recv() => {
                spawn_command_dispatch_task(&mut tasks, job, &shared);
            }
            Some(job) = receiver.recv() => {
                spawn_command_dispatch_task(&mut tasks, job, &shared);
            }
            Some(result) = tasks.join_next(), if !tasks.is_empty() => {
                if let Err(error) = result {
                    tracing::warn!(error = %error, "command dispatcher task failed");
                }
            }
            else => break,
        }
    }

    while let Some(result) = tasks.join_next().await {
        if let Err(error) = result {
            tracing::warn!(error = %error, "command dispatcher task failed");
        }
    }
}

#[derive(Clone)]
pub(crate) struct CommandDispatcherShared {
    pub(crate) config: NodeConfig,
    pub(crate) client: reqwest::Client,
    pub(crate) shared_state: NodeStateStore,
    pub(crate) sender: ControlFrameSender,
    pub(crate) terminal_supervisor: TerminalSupervisor,
    pub(crate) managed_supervisor: ManagedRuntimeSupervisor,
    pub(crate) locks: CommandExecutionLocks,
    pub(crate) cancellations: ExecutionCancellationRegistry,
    pub(crate) concurrency: Arc<Semaphore>,
}

pub(crate) fn spawn_command_dispatch_task(
    tasks: &mut tokio::task::JoinSet<()>,
    job: CommandDispatchJob,
    shared: &CommandDispatcherShared,
) {
    let shared = shared.clone();
    tasks.spawn(async move {
        let Some(_permit) = prepare_command_dispatch_task(
            &job.command,
            &shared.cancellations,
            shared.concurrency.clone(),
        )
        .await
        else {
            return;
        };
        let execution_lock = shared
            .locks
            .lock_for(command_execution_key(&job.command))
            .await;
        let _guard = execution_lock.lock().await;
        let post_execution_lock = if matches!(
            job.command.kind,
            CommandKind::InterruptRuntime | CommandKind::StopRuntime
        ) {
            match job.command.target.runtime_session_id() {
                Some(runtime_id) => Some(
                    shared
                        .locks
                        .lock_for(runtime_cancellation_key(runtime_id))
                        .await,
                ),
                None => None,
            }
        } else {
            None
        };
        let cancellation_guard = match execution_cancellation_key(&job.command) {
            Some(key) => Some(shared.cancellations.begin(key).await),
            None => None,
        };
        let cancellation_receiver = cancellation_guard
            .as_ref()
            .map(ExecutionCancellationGuard::receiver);
        let baseline = match shared.shared_state.snapshot().await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "command dispatcher could not load state snapshot"
                );
                let _ = send_dispatch_internal_error_result(
                    &shared.sender,
                    &job.command,
                    "node.dispatch_state_unavailable",
                    "Node could not load durable state before command dispatch",
                    false,
                )
                .await;
                return;
            }
        };
        let mut local_state = baseline.clone();
        let context = CommandDispatchContext {
            config: &shared.config,
            client: &shared.client,
            sender: &shared.sender,
            terminal_supervisor: &shared.terminal_supervisor,
            managed_supervisor: &shared.managed_supervisor,
            shared_state: &shared.shared_state,
            cancellation: cancellation_receiver,
            post_execution_lock,
        };
        if let Err(error) =
            handle_command_dispatch(context, job.command, &mut local_state, &baseline).await
        {
            tracing::warn!(error = %error, "command dispatch failed");
        }
        if let Some(guard) = cancellation_guard {
            shared.cancellations.finish(guard).await;
        }
    });
}

pub(crate) async fn prepare_command_dispatch_task(
    command: &CommandEnvelope,
    cancellations: &ExecutionCancellationRegistry,
    concurrency: Arc<Semaphore>,
) -> Option<tokio::sync::OwnedSemaphorePermit> {
    if let Some((key, remember_if_pending)) = cancellation_signal(command) {
        let cancelled = cancellations.cancel(key.clone(), remember_if_pending).await;
        tracing::debug!(cancelled, key, "cancellation command signalled provider");
    }
    match concurrency.acquire_owned().await {
        Ok(permit) => Some(permit),
        Err(_) => {
            tracing::warn!("command dispatcher semaphore closed");
            None
        }
    }
}

pub(crate) fn runtime_cancellation_key(runtime_session_id: &RuntimeSessionId) -> String {
    format!("runtime:{}", runtime_session_id.as_str())
}

pub(crate) fn is_priority_cancellation_command(command: &CommandEnvelope) -> bool {
    matches!(
        command.kind,
        CommandKind::InterruptRuntime
            | CommandKind::StopRuntime
            | CommandKind::CancelDeduction
            | CommandKind::CancelTaskRun
    ) || matches!(
        &command.payload,
        CommandPayload::Tooling { command }
            if matches!(command.payload, ToolingCommandPayloadV1::CancelToolCall { .. })
    )
}

pub(crate) fn deduction_cancellation_key(deduction_id: &uprava_protocol::DeductionId) -> String {
    format!("deduction:{}", deduction_id.as_str())
}

pub(crate) fn execution_cancellation_key(command: &CommandEnvelope) -> Option<String> {
    match (&command.kind, &command.payload) {
        (CommandKind::SendTurn, _) => command
            .target
            .runtime_session_id()
            .map(runtime_cancellation_key),
        (CommandKind::RequestDeduction, CommandPayload::RequestDeduction { package }) => {
            Some(deduction_cancellation_key(&package.deduction_id))
        }
        (CommandKind::RunTask, CommandPayload::RunTask { spec, .. }) => {
            Some(task_cancellation_key(&spec.task_run_id))
        }
        (CommandKind::Tooling, CommandPayload::Tooling { command }) => match &command.payload {
            ToolingCommandPayloadV1::ExecuteExternalTool { tool_call_id, .. } => {
                Some(tool_call_cancellation_key(tool_call_id))
            }
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn cancellation_signal(command: &CommandEnvelope) -> Option<(String, bool)> {
    match (&command.kind, &command.payload) {
        (CommandKind::InterruptRuntime | CommandKind::StopRuntime, _) => command
            .target
            .runtime_session_id()
            .map(runtime_cancellation_key)
            .map(|key| (key, false)),
        (CommandKind::CancelDeduction, CommandPayload::CancelDeduction { deduction_id }) => {
            Some((deduction_cancellation_key(deduction_id), true))
        }
        (CommandKind::CancelTaskRun, CommandPayload::CancelTaskRun { task_run_id }) => {
            Some((task_cancellation_key(task_run_id), true))
        }
        (CommandKind::Tooling, CommandPayload::Tooling { command }) => match &command.payload {
            ToolingCommandPayloadV1::CancelToolCall { tool_call_id, .. } => {
                Some((tool_call_cancellation_key(tool_call_id), true))
            }
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn command_execution_key(command: &CommandEnvelope) -> String {
    if matches!(
        command.kind,
        CommandKind::InterruptRuntime | CommandKind::StopRuntime
    ) {
        return format!("priority-command:{}", command.command_id.as_str());
    }
    if let Some(task_run_id) = command.target.task_run_id() {
        return task_cancellation_key(task_run_id);
    }
    if let CommandPayload::Tooling { command } = &command.payload {
        return match &command.payload {
            ToolingCommandPayloadV1::BeginIntegrationAuthorization {
                dependency_instance_id,
                ..
            }
            | ToolingCommandPayloadV1::UpdateDependencyDesiredState {
                dependency_instance_id,
                ..
            }
            | ToolingCommandPayloadV1::ExecuteExternalTool {
                dependency_instance_id,
                ..
            } => format!("tool-dependency:{}", dependency_instance_id.as_str()),
            ToolingCommandPayloadV1::CancelToolCall { tool_call_id, .. } => {
                tool_call_cancellation_key(tool_call_id)
            }
        };
    }
    command
        .target
        .runtime_session_id()
        .map(|runtime_id| format!("runtime:{}", runtime_id.as_str()))
        .or_else(|| {
            command
                .target
                .project_placement_id()
                .map(|placement_id| format!("placement:{}", placement_id.as_str()))
        })
        .unwrap_or_else(|| format!("command:{}", command.command_id.as_str()))
}

pub(crate) fn task_cancellation_key(task_run_id: &uprava_protocol::TaskRunId) -> String {
    format!("task:{}", task_run_id.as_str())
}

pub(crate) struct CommandDispatchContext<'a> {
    pub(crate) config: &'a NodeConfig,
    pub(crate) client: &'a reqwest::Client,
    pub(crate) sender: &'a ControlFrameSender,
    pub(crate) terminal_supervisor: &'a TerminalSupervisor,
    pub(crate) managed_supervisor: &'a ManagedRuntimeSupervisor,
    pub(crate) shared_state: &'a NodeStateStore,
    pub(crate) cancellation: Option<watch::Receiver<bool>>,
    pub(crate) post_execution_lock: Option<Arc<Mutex<()>>>,
}

pub(crate) async fn handle_command_dispatch(
    context: CommandDispatchContext<'_>,
    command: CommandEnvelope,
    local_state: &mut NodeLocalState,
    baseline: &NodeLocalState,
) -> anyhow::Result<()> {
    send_frame(
        context.sender,
        ControlFrame::CommandAck {
            frame_id: Uuid::new_v4().to_string(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
            command_id: command.command_id.clone(),
            status: CommandState::Acknowledged,
        },
    )
    .await?;

    let managed_process_start = matches!(
        &command.payload,
        CommandPayload::StartRuntime {
            execution_profile: AgentExecutionProfile::Managed,
            ..
        } | CommandPayload::ResumeRuntime {
            execution_profile: AgentExecutionProfile::Managed,
            ..
        }
    );
    let provider_mcp_access = if (command.kind == CommandKind::SendTurn || managed_process_start)
        && !local_state
            .command_status
            .contains_key(command.command_id.as_str())
    {
        match request_provider_mcp_access(
            context.client,
            context.config,
            local_state,
            &command.command_id,
        )
        .await
        {
            Ok(access) => Some(access),
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    command_kind = ?command.kind,
                    correlation_id = %command.correlation_id,
                    "provider MCP access could not be issued"
                );
                let outcome = provider_mcp_access_failure_outcome(local_state, &command);
                if outcome.state_changed {
                    context
                        .shared_state
                        .persist_command_outcome(baseline, local_state)
                        .await?;
                }
                send_event_batches(context.sender, outcome.events_to_send).await?;
                return send_command_result(
                    context.sender,
                    &command.command_id,
                    outcome.status,
                    outcome.result_payload,
                )
                .await;
            }
        }
    } else {
        None
    };

    let priority_runtime_command = context.post_execution_lock.is_some();
    let mut outcome = prepare_command_dispatch_with_live_socket(
        context.config,
        local_state,
        &command,
        CommandExecutionContext {
            provider_mcp_access: provider_mcp_access.as_ref(),
            live_sender: (!priority_runtime_command).then_some(context.sender),
            terminal_supervisor: Some(context.terminal_supervisor),
            managed_supervisor: Some(context.managed_supervisor),
            cancellation: context.cancellation,
            state_store: Some(context.shared_state),
        },
    )
    .await;
    let _post_execution_guard = match context.post_execution_lock {
        Some(lock) => Some(lock.lock_owned().await),
        None => None,
    };
    if priority_runtime_command {
        let current = context.shared_state.snapshot().await?;
        rebase_priority_runtime_events(
            local_state,
            &mut outcome.events_to_send,
            &current,
            &command,
        );
    }
    if outcome.state_changed {
        context
            .shared_state
            .persist_command_outcome(baseline, local_state)
            .await?;
    }

    send_event_batches(context.sender, outcome.events_to_send).await?;
    send_command_result(
        context.sender,
        &command.command_id,
        outcome.status,
        outcome.result_payload,
    )
    .await
}

pub(crate) fn rebase_priority_runtime_events(
    local_state: &mut NodeLocalState,
    events_to_send: &mut [EventEnvelope],
    current: &NodeLocalState,
    command: &CommandEnvelope,
) {
    let Some(runtime_id) = command.target.runtime_session_id() else {
        return;
    };
    let mut seq = current
        .runtime_seqs
        .get(runtime_id.as_str())
        .copied()
        .unwrap_or_default();
    for event in events_to_send
        .iter_mut()
        .filter(|event| event.runtime_session_id.as_ref() == Some(runtime_id))
    {
        seq = seq.saturating_add(1);
        event.seq = seq;
        if let Some(durable) = local_state
            .event_outbox
            .iter_mut()
            .find(|durable| durable.event_id == event.event_id)
        {
            durable.seq = seq;
        }
    }
    local_state.runtime_seqs.insert(runtime_id.to_string(), seq);
}

pub(crate) async fn send_command_result(
    sender: &ControlFrameSender,
    command_id: &CommandId,
    status: CommandState,
    payload: JsonValue,
) -> anyhow::Result<()> {
    send_frame(
        sender,
        ControlFrame::CommandResult {
            frame_id: Uuid::new_v4().to_string(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
            command_id: command_id.clone(),
            status,
            payload,
        },
    )
    .await
}

pub(crate) async fn replay_event_outbox(
    sender: &ControlFrameSender,
    events: &[EventEnvelope],
) -> anyhow::Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    tracing::info!(events = events.len(), "replaying control event outbox");
    send_event_batches(sender, events.to_vec()).await
}

const EVENT_DELIVERY_BATCH_SIZE: usize = 32;

pub(crate) async fn send_event_batches(
    sender: &ControlFrameSender,
    events: Vec<EventEnvelope>,
) -> anyhow::Result<()> {
    for batch in events.chunks(EVENT_DELIVERY_BATCH_SIZE) {
        send_event_batch(sender, batch.to_vec()).await?;
    }
    Ok(())
}

pub(crate) async fn send_event_batch(
    sender: &ControlFrameSender,
    events: Vec<EventEnvelope>,
) -> anyhow::Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    sender
        .send(ControlFrame::EventBatch {
            frame_id: Uuid::new_v4().to_string(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
            events,
        })
        .await
        .context("control event batch send failed")
}

pub(crate) async fn send_dispatch_busy_result(
    sender: &ControlFrameSender,
    command: &CommandEnvelope,
) -> anyhow::Result<()> {
    send_dispatch_internal_error_result(
        sender,
        command,
        "node.dispatch_busy",
        "Node command dispatcher is saturated; retry the command later",
        true,
    )
    .await
}

pub(crate) async fn send_dispatch_closed_result(
    sender: &ControlFrameSender,
    command: &CommandEnvelope,
) -> anyhow::Result<()> {
    send_dispatch_internal_error_result(
        sender,
        command,
        "node.dispatch_closed",
        "Node command dispatcher is unavailable on this control connection",
        true,
    )
    .await
}

pub(crate) async fn send_dispatch_internal_error_result(
    sender: &ControlFrameSender,
    command: &CommandEnvelope,
    error_code: &'static str,
    message: &'static str,
    retryable: bool,
) -> anyhow::Result<()> {
    send_frame(
        sender,
        ControlFrame::CommandAck {
            frame_id: Uuid::new_v4().to_string(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
            command_id: command.command_id.clone(),
            status: CommandState::Acknowledged,
        },
    )
    .await?;
    send_command_result(
        sender,
        &command.command_id,
        CommandState::Failed,
        JsonValue(serde_json::json!({
            "error_code": error_code,
            "message": message,
            "retryable": retryable,
        })),
    )
    .await
}

pub(crate) async fn send_frame(
    sender: &ControlFrameSender,
    frame: ControlFrame,
) -> anyhow::Result<()> {
    sender.try_send(frame).context("control frame send failed")
}

pub(crate) fn control_frame_protocol_error(frame: &ControlFrame) -> Option<ControlFrame> {
    let received_protocol_version = control_frame_protocol_version(frame);
    if is_supported_protocol_version(received_protocol_version) {
        return None;
    }

    Some(ControlFrame::ControlError {
        frame_id: Uuid::new_v4().to_string(),
        protocol_version: API_VERSION.to_owned(),
        sent_at: Utc::now(),
        error: ApiError {
            error_code: "control.protocol_incompatible".to_owned(),
            message: "Control protocol version is incompatible".to_owned(),
            details: JsonValue(serde_json::json!({
                "expected_protocol_version": API_VERSION,
                "received_protocol_version": received_protocol_version,
            })),
            retryable: false,
            correlation_id: CorrelationId::from(Uuid::new_v4().to_string()),
        },
    })
}

pub(crate) fn control_frame_protocol_version(frame: &ControlFrame) -> &str {
    match frame {
        ControlFrame::Hello {
            protocol_version, ..
        }
        | ControlFrame::HelloAck {
            protocol_version, ..
        }
        | ControlFrame::CommandDispatch {
            protocol_version, ..
        }
        | ControlFrame::CommandAck {
            protocol_version, ..
        }
        | ControlFrame::CommandResult {
            protocol_version, ..
        }
        | ControlFrame::EventBatch {
            protocol_version, ..
        }
        | ControlFrame::EventBatchAck {
            protocol_version, ..
        }
        | ControlFrame::WorkspaceTerminalAttach {
            protocol_version, ..
        }
        | ControlFrame::WorkspaceTerminalInput {
            protocol_version, ..
        }
        | ControlFrame::WorkspaceTerminalResize {
            protocol_version, ..
        }
        | ControlFrame::WorkspaceTerminalClose {
            protocol_version, ..
        }
        | ControlFrame::WorkspaceTerminalOutput {
            protocol_version, ..
        }
        | ControlFrame::WorkspaceTerminalStatus {
            protocol_version, ..
        }
        | ControlFrame::Ping {
            protocol_version, ..
        }
        | ControlFrame::Pong {
            protocol_version, ..
        }
        | ControlFrame::ControlError {
            protocol_version, ..
        } => protocol_version,
    }
}

#[derive(Debug)]
pub(crate) struct CommandDispatchOutcome {
    pub(crate) status: CommandState,
    pub(crate) events_to_send: Vec<EventEnvelope>,
    pub(crate) result_payload: JsonValue,
    pub(crate) state_changed: bool,
}
