//! Outbound protocol-v2 control-channel transport.

use super::super::*;

pub(crate) async fn control_channel_loop(
    config: NodeConfig,
    client: reqwest::Client,
    store: NodeStateStore,
    terminal_supervisor: TerminalSupervisor,
    managed_supervisor: ManagedRuntimeSupervisor,
) {
    loop {
        if let Err(error) = store.persist_reconnect_attempt().await {
            tracing::warn!(error = %error, "failed to persist reconnect metric");
        }
        match run_control_channel(
            &config,
            &client,
            &store,
            &terminal_supervisor,
            &managed_supervisor,
        )
        .await
        {
            Ok(()) => tracing::warn!("control channel closed"),
            Err(error) => tracing::warn!(error = %error, "control channel failed"),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

pub(crate) async fn run_control_channel(
    config: &NodeConfig,
    client: &reqwest::Client,
    store: &NodeStateStore,
    terminal_supervisor: &TerminalSupervisor,
    managed_supervisor: &ManagedRuntimeSupervisor,
) -> anyhow::Result<()> {
    let local_state = store.snapshot().await?;
    let node_id = local_state
        .node_id
        .clone()
        .context("node id is missing for control channel")?;
    let credential = local_state
        .credential
        .clone()
        .context("credential is missing for control channel")?;
    let active_runtime_ids = active_runtime_ids(&local_state);
    let mut actual_runtime_attempts = local_state
        .managed_attempts
        .iter()
        .map(|(runtime_session_id, attempt)| RuntimeAttemptActualState {
            runtime_session_id: RuntimeSessionId::from(runtime_session_id.as_str()),
            runtime_attempt_id: attempt.runtime_attempt_id.clone(),
            state: attempt.state,
            effective_policy_hash: attempt.policy_hash.clone(),
            active_turn_id: attempt.active_turn_id.clone(),
            started_at: attempt.started_at,
        })
        .collect::<Vec<_>>();
    actual_runtime_attempts.sort_by(|left, right| {
        left.runtime_session_id
            .as_str()
            .cmp(right.runtime_session_id.as_str())
    });
    let event_outbox = local_state.event_outbox.clone();
    let url = control_url(&config.core_url)?;
    let mut request = url
        .as_str()
        .into_client_request()
        .context("control channel request should build")?;
    request.headers_mut().insert(
        "x-uprava-node-id",
        HeaderValue::from_str(node_id.as_str()).context("node id header should be valid")?,
    );
    request.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {credential}"))
            .context("authorization header should be valid")?,
    );

    let (socket, _) = connect_async(request)
        .await
        .context("control channel connection failed")?;
    let (mut socket_sender, mut socket_receiver) = socket.split();
    let (outbound_tx, mut outbound_rx) =
        mpsc::channel::<ControlFrame>(CONTROL_WRITER_QUEUE_CAPACITY);
    let send_task = tokio::spawn(async move {
        while let Some(frame) = outbound_rx.recv().await {
            let Ok(text) = serde_json::to_string(&frame) else {
                continue;
            };
            if socket_sender
                .send(WsMessage::Text(text.into()))
                .await
                .is_err()
            {
                break;
            }
        }
    });
    let (dispatch_tx, dispatch_rx) =
        mpsc::channel::<CommandDispatchJob>(NODE_COMMAND_DISPATCH_QUEUE_CAPACITY);
    let (priority_dispatch_tx, priority_dispatch_rx) =
        mpsc::channel::<CommandDispatchJob>(NODE_PRIORITY_COMMAND_DISPATCH_QUEUE_CAPACITY);
    let dispatcher_task = tokio::spawn(run_command_dispatcher(
        config.clone(),
        client.clone(),
        store.clone(),
        outbound_tx.clone(),
        terminal_supervisor.clone(),
        managed_supervisor.clone(),
        priority_dispatch_rx,
        dispatch_rx,
    ));
    tracing::info!("control channel connected");
    terminal_supervisor.rebind_sender(&outbound_tx).await;
    send_frame(
        &outbound_tx,
        ControlFrame::Hello {
            frame_id: Uuid::new_v4().to_string(),
            protocol_version: API_VERSION.to_owned(),
            sent_at: Utc::now(),
            node_id: node_id.clone(),
            daemon_version: daemon_version(),
            active_runtime_ids,
            actual_runtime_attempts,
        },
    )
    .await?;

    replay_event_outbox(&outbound_tx, &event_outbox).await?;

    while let Some(message) = socket_receiver.next().await {
        let message = message.context("control channel read failed")?;
        let WsMessage::Text(text) = message else {
            continue;
        };
        let frame = serde_json::from_str::<ControlFrame>(&text)
            .context("control frame was not valid JSON")?;
        if let Some(error_frame) = control_frame_protocol_error(&frame) {
            send_frame(&outbound_tx, error_frame).await?;
            continue;
        }
        match frame {
            ControlFrame::CommandDispatch { command, .. } => {
                let command = *command;
                let dispatch_result = if is_priority_cancellation_command(&command) {
                    priority_dispatch_tx.try_send(CommandDispatchJob { command })
                } else {
                    dispatch_tx.try_send(CommandDispatchJob { command })
                };
                match dispatch_result {
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Full(job)) => {
                        send_dispatch_busy_result(&outbound_tx, &job.command).await?;
                    }
                    Err(mpsc::error::TrySendError::Closed(job)) => {
                        send_dispatch_closed_result(&outbound_tx, &job.command).await?;
                    }
                }
            }
            ControlFrame::Ping { frame_id, .. } => {
                send_frame(
                    &outbound_tx,
                    ControlFrame::Pong {
                        frame_id,
                        protocol_version: API_VERSION.to_owned(),
                        sent_at: Utc::now(),
                    },
                )
                .await?;
            }
            ControlFrame::EventBatchAck {
                accepted_event_ids, ..
            } => {
                let removed = store.persist_event_ack(&accepted_event_ids).await?;
                if removed > 0 {
                    let local_state = store.snapshot().await?;
                    tracing::info!(
                        removed,
                        remaining = local_state.event_outbox.len(),
                        "control event outbox acked"
                    );
                }
            }
            ControlFrame::HelloAck { .. } => {}
            ControlFrame::WorkspaceTerminalAttach { terminal_id, .. } => {
                terminal_supervisor.attach(&outbound_tx, &terminal_id).await;
            }
            ControlFrame::WorkspaceTerminalInput {
                terminal_id, data, ..
            } => {
                terminal_supervisor.input(&terminal_id, data).await;
            }
            ControlFrame::WorkspaceTerminalResize {
                terminal_id,
                cols,
                rows,
                ..
            } => {
                terminal_supervisor.resize(&terminal_id, cols, rows).await;
            }
            ControlFrame::WorkspaceTerminalClose { terminal_id, .. } => {
                terminal_supervisor.close(&terminal_id).await;
            }
            _ => {}
        }
    }
    terminal_supervisor.detach_sender().await;
    dispatcher_task.abort();
    let _ = dispatcher_task.await;
    send_task.abort();
    let _ = send_task.await;
    Ok(())
}
