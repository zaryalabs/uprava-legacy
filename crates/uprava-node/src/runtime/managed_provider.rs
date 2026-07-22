//! Process-per-attempt Codex app-server supervision.
//!
//! Provider sockets and child handles deliberately live only in this module.
//! Durable Node state receives the secret-free [`ManagedAttemptDescriptor`].

use super::*;
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

const MANAGED_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MANAGED_COMMAND_QUEUE_CAPACITY: usize = 64;
const MAX_MANAGED_PROTOCOL_BYTES: usize = 1024 * 1024;
const MAX_MANAGED_PROMPT_CHARS: usize = 1200;
const MAX_MANAGED_ANSWERS: usize = 16;

type ManagedSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Secret-free local recovery descriptor for one managed process incarnation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ManagedAttemptDescriptor {
    pub(crate) runtime_attempt_id: RuntimeAttemptId,
    pub(crate) provider_thread_id: Option<String>,
    pub(crate) state: RuntimeAttemptState,
    pub(crate) policy_hash: RuntimePolicyHash,
    pub(crate) active_turn_id: Option<TurnId>,
    pub(crate) started_at: DateTime<Utc>,
    pub(crate) stopped_at: Option<DateTime<Utc>>,
    pub(crate) terminal_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) enum ManagedRuntimeUpdate {
    TurnStarted,
    OutputDelta(String),
    MessageCompleted(String),
    Activity {
        method: String,
        payload: serde_json::Value,
        unknown: bool,
    },
    InteractionRequested {
        interaction_id: ProviderInteractionId,
        kind: ProviderInteractionKind,
        prompt: String,
    },
    InteractionResolved {
        interaction_id: ProviderInteractionId,
        kind: ProviderInteractionKind,
        approved: Option<bool>,
        answers: Vec<String>,
    },
    TurnCompleted,
    TurnInterrupted,
    Failed {
        code: &'static str,
        message: String,
    },
}

#[derive(Debug)]
pub(crate) struct ManagedOperation {
    pub(crate) updates: mpsc::Receiver<ManagedRuntimeUpdate>,
}

#[derive(Debug, Clone)]
pub(crate) struct ManagedRuntimeError {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

impl ManagedRuntimeError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct ManagedRuntimeSupervisor {
    sessions: Arc<Mutex<HashMap<String, Arc<Mutex<ManagedSession>>>>>,
}

struct ManagedSession {
    attempt_id: RuntimeAttemptId,
    provider_thread_id: String,
    active_turn_id: Option<String>,
    pending: HashMap<String, PendingInteraction>,
    next_request_id: u64,
    socket: ManagedSocket,
    child: tokio::process::Child,
    stderr_task: tokio::task::JoinHandle<()>,
    step_timeout: Duration,
}

#[derive(Debug)]
struct PendingInteraction {
    provider_request_id: serde_json::Value,
    kind: ProviderInteractionKind,
    question_ids: Vec<String>,
}

impl ManagedRuntimeSupervisor {
    #[expect(
        clippy::too_many_arguments,
        reason = "attempt start binds runtime identity, immutable policy, recovery reference, and ephemeral MCP access"
    )]
    pub(crate) async fn start(
        &self,
        config: &NodeConfig,
        runtime_id: &RuntimeSessionId,
        workspace: &str,
        policy: &uprava_protocol::EffectiveRuntimePolicy,
        policy_hash: &RuntimePolicyHash,
        resume_thread_id: Option<&str>,
        mcp_access: Option<&ProviderMcpAccess>,
    ) -> Result<ManagedAttemptDescriptor, ManagedRuntimeError> {
        if let Some(reason) = &config.codex_managed_unavailable_reason {
            return Err(ManagedRuntimeError::new(
                "provider.managed_capability_unavailable",
                format!("Codex managed runtime capability is unavailable: {reason}"),
            ));
        }
        if let Some(expected) = policy.provider_version.as_deref() {
            let installed = config
                .codex_version
                .as_deref()
                .and_then(provider_version_number);
            let expected = provider_version_number(expected);
            if installed.is_none() || expected.is_none() || installed != expected {
                return Err(ManagedRuntimeError::new(
                    "provider.managed_version_mismatch",
                    "Installed Codex version does not match the immutable effective policy",
                ));
            }
        }
        if self.inspect(runtime_id).await.is_some() {
            return Err(ManagedRuntimeError::new(
                "provider.managed_already_running",
                "A managed Codex attempt is already active for this runtime",
            ));
        }
        let mut session = ManagedSession::spawn(config, workspace, mcp_access).await?;
        let provider_thread_id = if let Some(thread_id) = resume_thread_id {
            let response = session
                .request(
                    "thread/resume",
                    serde_json::json!({ "threadId": thread_id }),
                )
                .await?;
            required_provider_string(&response, &["thread", "id"])?
        } else {
            let response = session
                .request(
                    "thread/start",
                    serde_json::json!({
                        "cwd": workspace,
                        "runtimeWorkspaceRoots": [workspace],
                        "sandbox": sandbox_literal(policy.sandbox_mode),
                        "approvalPolicy": approval_literal(policy.approval_mode),
                        "approvalsReviewer": "user",
                        "ephemeral": false,
                    }),
                )
                .await?;
            required_provider_string(&response, &["thread", "id"])?
        };
        session.provider_thread_id.clone_from(&provider_thread_id);
        let attempt_id = session.attempt_id.clone();
        self.sessions
            .lock()
            .await
            .insert(runtime_id.to_string(), Arc::new(Mutex::new(session)));
        Ok(ManagedAttemptDescriptor {
            runtime_attempt_id: attempt_id,
            provider_thread_id: Some(provider_thread_id),
            state: RuntimeAttemptState::Ready,
            policy_hash: policy_hash.clone(),
            active_turn_id: None,
            started_at: Utc::now(),
            stopped_at: None,
            terminal_reason: None,
        })
    }

    pub(crate) async fn send_turn(
        &self,
        runtime_id: &RuntimeSessionId,
        content: String,
        cancellation: Option<watch::Receiver<bool>>,
    ) -> Result<ManagedOperation, ManagedRuntimeError> {
        let session = self.session(runtime_id).await?;
        let (updates_tx, updates) = mpsc::channel(MANAGED_COMMAND_QUEUE_CAPACITY);
        tokio::spawn(async move {
            let mut session = session.lock().await;
            if session.active_turn_id.is_some() {
                send_managed_failure(
                    &updates_tx,
                    "provider.turn_already_active",
                    "The managed runtime already has an active turn",
                )
                .await;
                return;
            }
            match session.start_turn(&content).await {
                Ok(()) => {
                    let _ = updates_tx.send(ManagedRuntimeUpdate::TurnStarted).await;
                    session.drive_active_turn(updates_tx, cancellation).await;
                }
                Err(error) => send_managed_error(&updates_tx, error).await,
            }
        });
        Ok(ManagedOperation { updates })
    }

    pub(crate) async fn resolve_approval(
        &self,
        runtime_id: &RuntimeSessionId,
        interaction_id: &ProviderInteractionId,
        approved: bool,
    ) -> Result<ManagedOperation, ManagedRuntimeError> {
        let session = self.session(runtime_id).await?;
        let interaction_id = interaction_id.clone();
        let (updates_tx, updates) = mpsc::channel(MANAGED_COMMAND_QUEUE_CAPACITY);
        tokio::spawn(async move {
            let mut session = session.lock().await;
            let Some(pending) = session.pending.remove(interaction_id.as_str()) else {
                send_managed_failure(
                    &updates_tx,
                    "provider.interaction_terminal_conflict",
                    "The provider interaction is no longer pending",
                )
                .await;
                return;
            };
            if pending.kind != ProviderInteractionKind::Approval {
                send_managed_failure(
                    &updates_tx,
                    "provider.interaction_kind_mismatch",
                    "The provider interaction is not an approval",
                )
                .await;
                return;
            }
            let result = serde_json::json!({
                "decision": if approved { "accept" } else { "decline" }
            });
            if let Err(error) = session.respond(pending.provider_request_id, result).await {
                send_managed_error(&updates_tx, error).await;
                return;
            }
            let _ = updates_tx
                .send(ManagedRuntimeUpdate::InteractionResolved {
                    interaction_id,
                    kind: ProviderInteractionKind::Approval,
                    approved: Some(approved),
                    answers: vec![],
                })
                .await;
            session.drive_active_turn(updates_tx, None).await;
        });
        Ok(ManagedOperation { updates })
    }

    pub(crate) async fn submit_input(
        &self,
        runtime_id: &RuntimeSessionId,
        interaction_id: &ProviderInteractionId,
        answers: &[String],
    ) -> Result<ManagedOperation, ManagedRuntimeError> {
        if answers.is_empty() || answers.len() > MAX_MANAGED_ANSWERS {
            return Err(ManagedRuntimeError::new(
                "provider.input_invalid",
                "Managed provider input must contain between 1 and 16 answers",
            ));
        }
        let session = self.session(runtime_id).await?;
        let interaction_id = interaction_id.clone();
        let answers = answers
            .iter()
            .map(|answer| bounded_text(answer, MAX_MANAGED_PROMPT_CHARS))
            .collect::<Vec<_>>();
        let (updates_tx, updates) = mpsc::channel(MANAGED_COMMAND_QUEUE_CAPACITY);
        tokio::spawn(async move {
            let mut session = session.lock().await;
            let Some(pending) = session.pending.remove(interaction_id.as_str()) else {
                send_managed_failure(
                    &updates_tx,
                    "provider.interaction_terminal_conflict",
                    "The provider interaction is no longer pending",
                )
                .await;
                return;
            };
            if pending.kind != ProviderInteractionKind::UserInput {
                send_managed_failure(
                    &updates_tx,
                    "provider.interaction_kind_mismatch",
                    "The provider interaction is not a user-input request",
                )
                .await;
                return;
            }
            let mut provider_answers = serde_json::Map::new();
            for (index, question_id) in pending.question_ids.iter().enumerate() {
                let answer = answers
                    .get(index)
                    .or_else(|| answers.last())
                    .cloned()
                    .unwrap_or_default();
                provider_answers.insert(
                    question_id.clone(),
                    serde_json::json!({ "answers": [answer] }),
                );
            }
            if let Err(error) = session
                .respond(
                    pending.provider_request_id,
                    serde_json::json!({ "answers": provider_answers }),
                )
                .await
            {
                send_managed_error(&updates_tx, error).await;
                return;
            }
            let _ = updates_tx
                .send(ManagedRuntimeUpdate::InteractionResolved {
                    interaction_id,
                    kind: ProviderInteractionKind::UserInput,
                    approved: None,
                    answers,
                })
                .await;
            session.drive_active_turn(updates_tx, None).await;
        });
        Ok(ManagedOperation { updates })
    }

    pub(crate) async fn interrupt(
        &self,
        runtime_id: &RuntimeSessionId,
    ) -> Result<ManagedOperation, ManagedRuntimeError> {
        let session = self.session(runtime_id).await?;
        let (updates_tx, updates) = mpsc::channel(MANAGED_COMMAND_QUEUE_CAPACITY);
        tokio::spawn(async move {
            let mut session = session.lock().await;
            if let Err(error) = session.interrupt_active_turn().await {
                if error.code == "provider.no_active_turn" {
                    return;
                }
                send_managed_error(&updates_tx, error).await;
                return;
            }
            session.drive_active_turn(updates_tx, None).await;
        });
        Ok(ManagedOperation { updates })
    }

    pub(crate) async fn stop(
        &self,
        runtime_id: &RuntimeSessionId,
    ) -> Result<ManagedAttemptDescriptor, ManagedRuntimeError> {
        let Some(session) = self.sessions.lock().await.remove(runtime_id.as_str()) else {
            return Err(ManagedRuntimeError::new(
                "provider.managed_not_running",
                "No managed Codex attempt is active for this runtime",
            ));
        };
        let mut session = session.lock().await;
        let attempt_id = session.attempt_id.clone();
        let thread_id = session.provider_thread_id.clone();
        terminate_provider_process(&mut session.child).await;
        session.stderr_task.abort();
        Ok(ManagedAttemptDescriptor {
            runtime_attempt_id: attempt_id,
            provider_thread_id: Some(thread_id),
            state: RuntimeAttemptState::Stopped,
            policy_hash: RuntimePolicyHash("stopped:retained-by-node-state".to_owned()),
            active_turn_id: None,
            started_at: Utc::now(),
            stopped_at: Some(Utc::now()),
            terminal_reason: Some("explicit_stop".to_owned()),
        })
    }

    pub(crate) async fn inspect(
        &self,
        runtime_id: &RuntimeSessionId,
    ) -> Option<(RuntimeAttemptId, String)> {
        let session = self
            .sessions
            .lock()
            .await
            .get(runtime_id.as_str())
            .cloned()?;
        let session = session.lock().await;
        Some((
            session.attempt_id.clone(),
            session.provider_thread_id.clone(),
        ))
    }

    pub(crate) async fn shutdown(&self) {
        let sessions = {
            let mut sessions = self.sessions.lock().await;
            sessions
                .drain()
                .map(|(_, session)| session)
                .collect::<Vec<_>>()
        };
        for session in sessions {
            let mut session = session.lock().await;
            terminate_provider_process(&mut session.child).await;
            session.stderr_task.abort();
        }
    }

    async fn session(
        &self,
        runtime_id: &RuntimeSessionId,
    ) -> Result<Arc<Mutex<ManagedSession>>, ManagedRuntimeError> {
        self.sessions
            .lock()
            .await
            .get(runtime_id.as_str())
            .cloned()
            .ok_or_else(|| {
                ManagedRuntimeError::new(
                    "provider.managed_not_running",
                    "No managed Codex attempt is active for this runtime",
                )
            })
    }
}

impl ManagedSession {
    async fn spawn(
        config: &NodeConfig,
        workspace: &str,
        mcp_access: Option<&ProviderMcpAccess>,
    ) -> Result<Self, ManagedRuntimeError> {
        let endpoint = available_managed_endpoint()?;
        let mut command = TokioCommand::new(&config.codex_binary);
        configure_provider_environment(&mut command);
        command
            .arg("app-server")
            .arg("--listen")
            .arg(&endpoint)
            .current_dir(workspace)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        configure_uprava_mcp(&mut command, mcp_access)
            .map_err(|error| ManagedRuntimeError::new(error.code, error.message))?;
        configure_provider_process(&mut command);
        let mut child = command.spawn().map_err(|error| {
            ManagedRuntimeError::new(
                "provider.managed_start_failed",
                format!("Could not start Codex app-server: {error}"),
            )
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            ManagedRuntimeError::new(
                "provider.managed_start_failed",
                "Codex app-server stderr was unavailable",
            )
        })?;
        let stderr_task = tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            let mut observed = 0usize;
            while observed < 64 {
                match lines.next_line().await {
                    Ok(Some(_)) => observed += 1,
                    Ok(None) | Err(_) => break,
                }
            }
            if observed > 0 {
                tracing::debug!(
                    observed,
                    "Codex app-server emitted bounded stderr diagnostics"
                );
            }
        });
        let socket = match timeout(MANAGED_CONNECT_TIMEOUT.min(config.codex_timeout), async {
            loop {
                if let Some(status) = child.try_wait().map_err(|error| {
                    ManagedRuntimeError::new(
                        "provider.managed_start_failed",
                        format!("Could not inspect Codex app-server: {error}"),
                    )
                })? {
                    return Err(ManagedRuntimeError::new(
                        "provider.managed_start_failed",
                        format!("Codex app-server exited before handshake: {status}"),
                    ));
                }
                match tokio_tungstenite::connect_async(&endpoint).await {
                    Ok((socket, _)) => return Ok(socket),
                    Err(_) => tokio::time::sleep(Duration::from_millis(25)).await,
                }
            }
        })
        .await
        {
            Ok(Ok(socket)) => socket,
            Ok(Err(error)) => {
                terminate_provider_process(&mut child).await;
                stderr_task.abort();
                return Err(error);
            }
            Err(_) => {
                terminate_provider_process(&mut child).await;
                stderr_task.abort();
                return Err(ManagedRuntimeError::new(
                    "provider.managed_connect_timeout",
                    "Timed out connecting to Codex app-server",
                ));
            }
        };
        let mut session = Self {
            attempt_id: RuntimeAttemptId::from(format!("attempt-{}", Uuid::new_v4())),
            provider_thread_id: String::new(),
            active_turn_id: None,
            pending: HashMap::new(),
            next_request_id: 1,
            socket,
            child,
            stderr_task,
            step_timeout: config.codex_timeout,
        };
        session
            .request(
                "initialize",
                serde_json::json!({
                    "clientInfo": { "name": "uprava-node", "version": env!("CARGO_PKG_VERSION") },
                    "capabilities": { "experimentalApi": true }
                }),
            )
            .await?;
        session
            .send_json(&serde_json::json!({ "method": "initialized" }))
            .await?;
        Ok(session)
    }

    async fn start_turn(&mut self, content: &str) -> Result<(), ManagedRuntimeError> {
        let response = self
            .request(
                "turn/start",
                serde_json::json!({
                    "threadId": self.provider_thread_id,
                    "input": [{ "type": "text", "text": content }]
                }),
            )
            .await?;
        self.active_turn_id = Some(required_provider_string(&response, &["turn", "id"])?);
        Ok(())
    }

    async fn drive_active_turn(
        &mut self,
        updates: mpsc::Sender<ManagedRuntimeUpdate>,
        mut cancellation: Option<watch::Receiver<bool>>,
    ) {
        loop {
            let next = tokio::select! {
                message = self.next_json() => message,
                _ = wait_for_runtime_cancellation(&mut cancellation), if cancellation.is_some() => {
                    if let Err(error) = self.interrupt_active_turn().await {
                        send_managed_error(&updates, error).await;
                        return;
                    }
                    cancellation = None;
                    continue;
                }
            };
            let message = match next {
                Ok(message) => message,
                Err(error) => {
                    send_managed_error(&updates, error).await;
                    return;
                }
            };
            if let Some(interaction) = self.interaction_from_message(&message) {
                let interaction_id = interaction.0.clone();
                self.pending
                    .insert(interaction_id.to_string(), interaction.3);
                let _ = updates
                    .send(ManagedRuntimeUpdate::InteractionRequested {
                        interaction_id,
                        kind: interaction.1,
                        prompt: interaction.2,
                    })
                    .await;
                return;
            }
            let method = message
                .get("method")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if method == "item/agentMessage/delta" {
                if let Some(delta) =
                    first_json_string_for_keys(&message["params"], &["delta", "text", "content"])
                {
                    let _ = updates
                        .send(ManagedRuntimeUpdate::OutputDelta(bounded_text(
                            &delta,
                            MAX_PROVIDER_ACTIVITY_LINE_CHARS,
                        )))
                        .await;
                }
                continue;
            }
            if method == "item/completed" && message["params"]["item"]["type"] == "agentMessage" {
                if let Some(content) =
                    first_json_string_for_keys(&message["params"]["item"], &["text", "content"])
                {
                    let _ = updates
                        .send(ManagedRuntimeUpdate::MessageCompleted(bounded_text(
                            &content,
                            MAX_CODEX_TRANSCRIPT_CHARS,
                        )))
                        .await;
                }
                continue;
            }
            if method == "turn/completed" && self.message_matches_active_turn(&message) {
                let status = message["params"]["turn"]["status"]
                    .as_str()
                    .unwrap_or("unknown");
                self.active_turn_id = None;
                self.pending.clear();
                let update = if status == "completed" {
                    ManagedRuntimeUpdate::TurnCompleted
                } else if status == "interrupted" {
                    ManagedRuntimeUpdate::TurnInterrupted
                } else {
                    ManagedRuntimeUpdate::Failed {
                        code: "provider.managed_turn_failed",
                        message: format!("Codex managed turn finished with status {status}"),
                    }
                };
                let _ = updates.send(update).await;
                return;
            }
            if !method.is_empty() {
                let known = matches!(
                    method,
                    "turn/started" | "item/started" | "item/updated" | "thread/status/changed"
                );
                let _ = updates
                    .send(ManagedRuntimeUpdate::Activity {
                        method: bounded_text(method, 160),
                        payload: bounded_provider_payload(&message["params"], !known),
                        unknown: !known,
                    })
                    .await;
            }
        }
    }

    fn interaction_from_message(
        &self,
        message: &serde_json::Value,
    ) -> Option<(
        ProviderInteractionId,
        ProviderInteractionKind,
        String,
        PendingInteraction,
    )> {
        let provider_request_id = message.get("id")?.clone();
        let method = message.get("method")?.as_str()?;
        let kind = match method {
            "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
                ProviderInteractionKind::Approval
            }
            "item/tool/requestUserInput" => ProviderInteractionKind::UserInput,
            _ => return None,
        };
        let interaction_id = ProviderInteractionId::from(format!("interaction-{}", Uuid::new_v4()));
        let prompt = first_json_string_for_keys(
            &message["params"],
            &["prompt", "question", "reason", "command", "description"],
        )
        .map(|value| bounded_text(&value, MAX_MANAGED_PROMPT_CHARS))
        .unwrap_or_else(|| match kind {
            ProviderInteractionKind::Approval => "Codex requests approval".to_owned(),
            ProviderInteractionKind::UserInput => "Codex requests user input".to_owned(),
        });
        let question_ids = message["params"]["questions"]
            .as_array()
            .map(|questions| {
                questions
                    .iter()
                    .filter_map(|question| question["id"].as_str())
                    .take(MAX_MANAGED_ANSWERS)
                    .map(|id| bounded_text(id, 256))
                    .collect()
            })
            .unwrap_or_default();
        Some((
            interaction_id,
            kind,
            prompt,
            PendingInteraction {
                provider_request_id,
                kind,
                question_ids,
            },
        ))
    }

    async fn interrupt_active_turn(&mut self) -> Result<(), ManagedRuntimeError> {
        let turn_id = self.active_turn_id.clone().ok_or_else(|| {
            ManagedRuntimeError::new(
                "provider.no_active_turn",
                "The managed runtime has no active turn",
            )
        })?;
        self.pending.clear();
        self.request(
            "turn/interrupt",
            serde_json::json!({
                "threadId": self.provider_thread_id,
                "turnId": turn_id,
            }),
        )
        .await?;
        Ok(())
    }

    fn message_matches_active_turn(&self, message: &serde_json::Value) -> bool {
        self.active_turn_id.as_deref() == message["params"]["turn"]["id"].as_str()
            && message["params"]["threadId"] == self.provider_thread_id
    }

    async fn request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, ManagedRuntimeError> {
        let id = self.next_request_id;
        self.next_request_id = self.next_request_id.saturating_add(1);
        self.send_json(&serde_json::json!({ "id": id, "method": method, "params": params }))
            .await?;
        timeout(self.step_timeout, async {
            loop {
                let message = self.next_json().await?;
                if message["id"] == id {
                    if let Some(error) = message.get("error") {
                        return Err(ManagedRuntimeError::new(
                            "provider.managed_request_failed",
                            bounded_provider_error(error),
                        ));
                    }
                    return message.get("result").cloned().ok_or_else(|| {
                        ManagedRuntimeError::new(
                            "provider.managed_protocol_invalid",
                            "Codex response did not contain a result",
                        )
                    });
                }
                if message.get("id").is_some() && message.get("method").is_some() {
                    self.respond_error(
                        message["id"].clone(),
                        "Provider callback arrived outside an active turn",
                    )
                    .await?;
                }
            }
        })
        .await
        .map_err(|_| {
            ManagedRuntimeError::new(
                "provider.managed_request_timeout",
                format!("Timed out waiting for Codex `{method}` response"),
            )
        })?
    }

    async fn respond(
        &mut self,
        id: serde_json::Value,
        result: serde_json::Value,
    ) -> Result<(), ManagedRuntimeError> {
        self.send_json(&serde_json::json!({ "id": id, "result": result }))
            .await
    }

    async fn respond_error(
        &mut self,
        id: serde_json::Value,
        message: &str,
    ) -> Result<(), ManagedRuntimeError> {
        self.send_json(&serde_json::json!({
            "id": id,
            "error": { "code": -32600, "message": message }
        }))
        .await
    }

    async fn send_json(&mut self, value: &serde_json::Value) -> Result<(), ManagedRuntimeError> {
        let text = serde_json::to_string(value).map_err(|error| {
            ManagedRuntimeError::new(
                "provider.managed_protocol_encode_failed",
                format!("Could not encode Codex protocol request: {error}"),
            )
        })?;
        if text.len() > MAX_MANAGED_PROTOCOL_BYTES {
            return Err(ManagedRuntimeError::new(
                "provider.managed_payload_oversized",
                "Outbound Codex protocol payload exceeded the size limit",
            ));
        }
        self.socket
            .send(WsMessage::Text(text.into()))
            .await
            .map_err(|error| {
                ManagedRuntimeError::new(
                    "provider.managed_disconnected",
                    format!("Codex app-server transport write failed: {error}"),
                )
            })
    }

    async fn next_json(&mut self) -> Result<serde_json::Value, ManagedRuntimeError> {
        loop {
            let message = self.socket.next().await.ok_or_else(|| {
                ManagedRuntimeError::new(
                    "provider.managed_disconnected",
                    "Codex app-server transport closed",
                )
            })?;
            match message {
                Ok(WsMessage::Text(text)) => {
                    if text.len() > MAX_MANAGED_PROTOCOL_BYTES {
                        return Err(ManagedRuntimeError::new(
                            "provider.managed_payload_oversized",
                            "Inbound Codex protocol payload exceeded the size limit",
                        ));
                    }
                    return serde_json::from_str(&text).map_err(|_| {
                        ManagedRuntimeError::new(
                            "provider.managed_protocol_invalid",
                            "Codex app-server emitted invalid JSON",
                        )
                    });
                }
                Ok(WsMessage::Ping(payload)) => {
                    self.socket
                        .send(WsMessage::Pong(payload))
                        .await
                        .map_err(|_| {
                            ManagedRuntimeError::new(
                                "provider.managed_disconnected",
                                "Codex app-server transport pong failed",
                            )
                        })?;
                }
                Ok(WsMessage::Pong(_)) => {}
                Ok(WsMessage::Close(_)) | Err(_) => {
                    return Err(ManagedRuntimeError::new(
                        "provider.managed_disconnected",
                        "Codex app-server transport disconnected",
                    ));
                }
                Ok(WsMessage::Binary(_) | WsMessage::Frame(_)) => {
                    return Err(ManagedRuntimeError::new(
                        "provider.managed_protocol_invalid",
                        "Codex app-server emitted a non-text frame",
                    ));
                }
            }
        }
    }
}

async fn send_managed_error(
    sender: &mpsc::Sender<ManagedRuntimeUpdate>,
    error: ManagedRuntimeError,
) {
    send_managed_failure(sender, error.code, error.message).await;
}

async fn send_managed_failure(
    sender: &mpsc::Sender<ManagedRuntimeUpdate>,
    code: &'static str,
    message: impl Into<String>,
) {
    let _ = sender
        .send(ManagedRuntimeUpdate::Failed {
            code,
            message: message.into(),
        })
        .await;
}

fn available_managed_endpoint() -> Result<String, ManagedRuntimeError> {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .map(|address| format!("ws://127.0.0.1:{}", address.port()))
        .map_err(|error| {
            ManagedRuntimeError::new(
                "provider.managed_endpoint_unavailable",
                format!("Could not reserve a Codex app-server endpoint: {error}"),
            )
        })
}

fn sandbox_literal(mode: ProviderSandboxMode) -> &'static str {
    match mode {
        ProviderSandboxMode::ReadOnly => "read-only",
        ProviderSandboxMode::WorkspaceWrite => "workspace-write",
        ProviderSandboxMode::DangerFullAccess => "danger-full-access",
    }
}

fn approval_literal(mode: uprava_protocol::ProviderApprovalMode) -> &'static str {
    match mode {
        uprava_protocol::ProviderApprovalMode::Untrusted => "untrusted",
        uprava_protocol::ProviderApprovalMode::OnFailure => "on-failure",
        uprava_protocol::ProviderApprovalMode::OnRequest => "on-request",
        uprava_protocol::ProviderApprovalMode::Never => "never",
    }
}

fn provider_version_number(value: &str) -> Option<(u64, u64, u64)> {
    value
        .split_whitespace()
        .find_map(super::config::parse_numeric_version)
}

fn required_provider_string(
    value: &serde_json::Value,
    path: &[&str],
) -> Result<String, ManagedRuntimeError> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment).ok_or_else(|| {
            ManagedRuntimeError::new(
                "provider.managed_protocol_invalid",
                format!("Codex response omitted `{segment}`"),
            )
        })?;
    }
    current
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .map(|value| bounded_text(value, 512))
        .ok_or_else(|| {
            ManagedRuntimeError::new(
                "provider.managed_protocol_invalid",
                "Codex response identity was not a string",
            )
        })
}

fn bounded_provider_payload(value: &serde_json::Value, diagnostic_only: bool) -> serde_json::Value {
    let encoded = serde_json::to_string(value).unwrap_or_default();
    if diagnostic_only {
        let keys = value
            .as_object()
            .map(|object| {
                object
                    .keys()
                    .take(16)
                    .map(|key| bounded_text(key, 80))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        return serde_json::json!({
            "provider_payload_omitted": true,
            "top_level_keys": keys,
            "original_chars": encoded.chars().count(),
        });
    }
    let redacted = redact_provider_value(value, 0);
    let encoded = serde_json::to_string(&redacted).unwrap_or_default();
    if encoded.chars().count() <= MAX_PROVIDER_ACTIVITY_RAW_CHARS {
        return redacted;
    }
    serde_json::json!({
        "truncated": true,
        "preview": bounded_text(&encoded, MAX_PROVIDER_ACTIVITY_RAW_CHARS),
    })
}

fn redact_provider_value(value: &serde_json::Value, depth: usize) -> serde_json::Value {
    if depth >= 8 {
        return serde_json::Value::String("[depth-limited]".to_owned());
    }
    match value {
        serde_json::Value::Object(object) => serde_json::Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let normalized = key.to_ascii_lowercase();
                    let sensitive = [
                        "authorization",
                        "credential",
                        "secret",
                        "token",
                        "environment",
                        "codex_home",
                    ]
                    .iter()
                    .any(|marker| normalized.contains(marker))
                        || normalized == "env";
                    (
                        bounded_text(key, 160),
                        if sensitive {
                            serde_json::Value::String("[redacted]".to_owned())
                        } else {
                            redact_provider_value(value, depth + 1)
                        },
                    )
                })
                .collect(),
        ),
        serde_json::Value::Array(values) => serde_json::Value::Array(
            values
                .iter()
                .take(64)
                .map(|value| redact_provider_value(value, depth + 1))
                .collect(),
        ),
        serde_json::Value::String(value) => {
            serde_json::Value::String(bounded_text(value, MAX_PROVIDER_ACTIVITY_LINE_CHARS))
        }
        scalar => scalar.clone(),
    }
}

fn bounded_provider_error(value: &serde_json::Value) -> String {
    let code = value
        .get("code")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or_default();
    let message = value
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("provider request failed");
    format!("code={code}, message={}", bounded_text(message, 240))
}

/// Reconciles descriptors after a Node process restart. App-server v2 cannot
/// safely reattach to a process-per-attempt loopback socket, so a live-looking
/// descriptor becomes explicitly lost and provider-resumable.
pub(crate) fn reconcile_managed_attempt_descriptors(state: &mut NodeLocalState) -> bool {
    let mut changed = false;
    for (runtime_id, descriptor) in &mut state.managed_attempts {
        if matches!(
            descriptor.state,
            RuntimeAttemptState::Starting
                | RuntimeAttemptState::Ready
                | RuntimeAttemptState::Disconnected
                | RuntimeAttemptState::Reconnecting
                | RuntimeAttemptState::Recovered
                | RuntimeAttemptState::Stopping
        ) {
            descriptor.state = RuntimeAttemptState::Lost;
            descriptor.active_turn_id = None;
            descriptor.stopped_at = Some(Utc::now());
            descriptor.terminal_reason = Some("node_restart_transport_lost".to_owned());
            state
                .runtime_states
                .insert(runtime_id.clone(), RuntimeSessionState::Stale);
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn provider_payload_is_bounded_without_copying_arbitrary_nesting() {
        let value =
            serde_json::json!({ "large_value": "x".repeat(MAX_PROVIDER_ACTIVITY_RAW_CHARS + 1) });
        let bounded = bounded_provider_payload(&value, false);
        assert!(
            serde_json::to_string(&bounded)
                .expect("bounded payload encodes")
                .len()
                < 20_000
        );
        assert!(bounded["large_value"]
            .as_str()
            .is_some_and(|value| value.ends_with("...")));
    }

    #[test]
    fn provider_payload_redacts_secrets_and_omits_unknown_nested_values() {
        let value = serde_json::json!({
            "authorization": "Bearer lease-secret",
            "nested": { "env": { "UPRAVA_MCP_ACCESS_TOKEN": "lease-secret" } },
        });
        let bounded = bounded_provider_payload(&value, false);
        let encoded = bounded.to_string();
        assert!(!encoded.contains("lease-secret"));
        assert_eq!(bounded["authorization"], "[redacted]");
        assert_eq!(bounded["nested"]["env"], "[redacted]");

        let diagnostic = bounded_provider_payload(&value, true);
        assert_eq!(diagnostic["provider_payload_omitted"], true);
        assert!(diagnostic.get("nested").is_none());
    }

    #[test]
    fn provider_error_is_bounded() {
        let error = bounded_provider_error(&serde_json::json!({
            "code": 42,
            "message": "x".repeat(500),
        }));
        assert!(error.len() < 300);
        assert!(error.starts_with("code=42"));
    }

    #[test]
    fn restart_reconciliation_never_reports_a_stale_handle_as_ready() {
        let runtime_id = "managed-runtime-restart".to_owned();
        let mut state = NodeLocalState::default();
        state
            .runtime_states
            .insert(runtime_id.clone(), RuntimeSessionState::Ready);
        state.managed_attempts.insert(
            runtime_id.clone(),
            ManagedAttemptDescriptor {
                runtime_attempt_id: RuntimeAttemptId::from("attempt-before-restart"),
                provider_thread_id: Some("thread-resumable".to_owned()),
                state: RuntimeAttemptState::Ready,
                policy_hash: RuntimePolicyHash("sha256:fixture".to_owned()),
                active_turn_id: Some(TurnId::from("turn-before-restart")),
                started_at: Utc::now(),
                stopped_at: None,
                terminal_reason: None,
            },
        );

        assert!(reconcile_managed_attempt_descriptors(&mut state));
        assert_eq!(
            state.runtime_states.get(&runtime_id),
            Some(&RuntimeSessionState::Stale)
        );
        let descriptor = state
            .managed_attempts
            .get(&runtime_id)
            .expect("descriptor remains resumable");
        assert_eq!(descriptor.state, RuntimeAttemptState::Lost);
        assert_eq!(
            descriptor.terminal_reason.as_deref(),
            Some("node_restart_transport_lost")
        );
        assert_eq!(
            descriptor.provider_thread_id.as_deref(),
            Some("thread-resumable")
        );
    }

    #[cfg(unix)]
    #[test]
    fn fake_codex_app_server_process() {
        let Ok(endpoint) = std::env::var("UPRAVA_FAKE_APP_SERVER_ENDPOINT") else {
            return;
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("fake provider runtime builds");
        runtime.block_on(run_fake_app_server(&endpoint));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn deterministic_fake_provider_keeps_one_live_thread_across_interactions() {
        let script = fake_app_server_binary();
        let config = NodeConfig {
            core_url: "http://127.0.0.1:8080".parse().expect("core URL parses"),
            display_name: "Managed Test Node".to_owned(),
            heartbeat_interval: Duration::from_secs(5),
            state_path: std::env::temp_dir().join(format!("managed-state-{}", Uuid::new_v4())),
            workspace_paths: vec![std::env::temp_dir()],
            codex_binary: script.display().to_string(),
            codex_version: Some("codex-cli 0.144.1".to_owned()),
            codex_managed_unavailable_reason: None,
            codex_ignore_user_config: false,
            codex_timeout: Duration::from_secs(5),
            opensandbox_url: None,
            task_runtime_image: "uprava/codex-runtime:test".to_owned(),
            toolhive_url: "http://127.0.0.1:9".parse().expect("ToolHive URL parses"),
            toolhive_timeout: Duration::from_secs(1),
        };
        let workspace = std::env::temp_dir().display().to_string();
        let policy = managed_policy(&workspace);
        let policy_hash = policy.policy_hash().expect("policy hashes");
        let supervisor = ManagedRuntimeSupervisor::default();
        let runtime_id = RuntimeSessionId::from("managed-runtime-1");

        let descriptor = supervisor
            .start(
                &config,
                &runtime_id,
                &workspace,
                &policy,
                &policy_hash,
                None,
                None,
            )
            .await
            .expect("managed fake starts");
        assert_eq!(
            descriptor.provider_thread_id.as_deref(),
            Some("thread-fake-1")
        );

        for prompt in ["first", "second"] {
            let updates = collect_updates(
                supervisor
                    .send_turn(&runtime_id, prompt.to_owned(), None)
                    .await
                    .expect("turn starts"),
            )
            .await;
            assert!(updates
                .iter()
                .any(|update| matches!(update, ManagedRuntimeUpdate::TurnCompleted)));
        }

        let approval_updates = collect_updates(
            supervisor
                .send_turn(&runtime_id, "approval".to_owned(), None)
                .await
                .expect("approval turn starts"),
        )
        .await;
        let approval_id = approval_updates
            .iter()
            .find_map(|update| match update {
                ManagedRuntimeUpdate::InteractionRequested {
                    interaction_id,
                    kind: ProviderInteractionKind::Approval,
                    ..
                } => Some(interaction_id.clone()),
                _ => None,
            })
            .expect("approval request is normalized");
        let resolved = collect_updates(
            supervisor
                .resolve_approval(&runtime_id, &approval_id, true)
                .await
                .expect("approval resolves"),
        )
        .await;
        assert!(resolved
            .iter()
            .any(|update| matches!(update, ManagedRuntimeUpdate::TurnCompleted)));

        let input_updates = collect_updates(
            supervisor
                .send_turn(&runtime_id, "input".to_owned(), None)
                .await
                .expect("input turn starts"),
        )
        .await;
        let input_id = input_updates
            .iter()
            .find_map(|update| match update {
                ManagedRuntimeUpdate::InteractionRequested {
                    interaction_id,
                    kind: ProviderInteractionKind::UserInput,
                    ..
                } => Some(interaction_id.clone()),
                _ => None,
            })
            .expect("input request is normalized");
        let answered = collect_updates(
            supervisor
                .submit_input(&runtime_id, &input_id, &["Alpha".to_owned()])
                .await
                .expect("input resolves"),
        )
        .await;
        assert!(answered
            .iter()
            .any(|update| matches!(update, ManagedRuntimeUpdate::TurnCompleted)));

        let stopped = supervisor.stop(&runtime_id).await.expect("runtime stops");
        assert_eq!(stopped.state, RuntimeAttemptState::Stopped);
        std::fs::remove_file(script).expect("fake provider script removes");
    }

    #[cfg(unix)]
    async fn collect_updates(mut operation: ManagedOperation) -> Vec<ManagedRuntimeUpdate> {
        let mut updates = Vec::new();
        while let Some(update) = operation.updates.recv().await {
            updates.push(update);
        }
        updates
    }

    #[cfg(unix)]
    fn managed_policy(workspace: &str) -> uprava_protocol::EffectiveRuntimePolicy {
        uprava_protocol::EffectiveRuntimePolicy {
            contract_version: 1,
            execution_profile: AgentExecutionProfile::Managed,
            provider: "codex".to_owned(),
            provider_version: Some("codex-cli 0.144.1".to_owned()),
            provider_capabilities: ProviderRuntimeCapability::required_for_managed_codex().to_vec(),
            sandbox_mode: ProviderSandboxMode::WorkspaceWrite,
            approval_mode: uprava_protocol::ProviderApprovalMode::Untrusted,
            workspace_root: workspace.to_owned(),
            additional_writable_paths: vec![],
            network_posture: uprava_protocol::RuntimeNetworkPosture::Unsupported,
            tool_exposure: uprava_protocol::RuntimeToolExposureSummary {
                server_count: 0,
                tool_count: 0,
                server_names: vec![],
            },
            credential_profile_ref: None,
            unsafe_override: None,
            capability_metadata: BTreeMap::from([(
                "transport".to_owned(),
                "app-server-v2".to_owned(),
            )]),
        }
    }

    #[cfg(unix)]
    fn fake_app_server_binary() -> PathBuf {
        let test_binary = std::env::current_exe().expect("test binary path resolves");
        let path = std::env::temp_dir().join(format!("fake-app-server-{}", Uuid::new_v4()));
        let script = format!(
            r#"#!/bin/sh
endpoint=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--listen" ]; then
    shift
    endpoint="$1"
  fi
  shift
done
UPRAVA_FAKE_APP_SERVER_ENDPOINT="$endpoint" exec '{}' --exact runtime::managed_provider::tests::fake_codex_app_server_process --nocapture
"#,
            test_binary.display()
        );
        std::fs::write(&path, script).expect("fake provider script writes");
        let mut permissions = std::fs::metadata(&path)
            .expect("fake provider metadata reads")
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&path, permissions).expect("fake provider is executable");
        path
    }

    #[cfg(unix)]
    async fn run_fake_app_server(endpoint: &str) {
        let url = Url::parse(endpoint).expect("fake endpoint parses");
        let listener = tokio::net::TcpListener::bind((
            url.host_str().expect("fake endpoint has host"),
            url.port().expect("fake endpoint has port"),
        ))
        .await
        .expect("fake provider binds");
        let (stream, _) = listener.accept().await.expect("fake provider accepts");
        let mut socket = tokio_tungstenite::accept_async(stream)
            .await
            .expect("fake websocket accepts");
        let mut turn = 0u64;
        while let Some(Ok(WsMessage::Text(text))) = socket.next().await {
            let message: serde_json::Value =
                serde_json::from_str(&text).expect("fake request is JSON");
            let Some(method) = message.get("method").and_then(serde_json::Value::as_str) else {
                if message.get("result").is_some() {
                    complete_fake_turn(&mut socket, turn).await;
                }
                continue;
            };
            let id = message.get("id").cloned();
            match method {
                "initialize" => {
                    send_fake(
                        &mut socket,
                        serde_json::json!({ "id": id, "result": { "userAgent": "fake-codex" } }),
                    )
                    .await;
                }
                "initialized" => {}
                "thread/start" | "thread/resume" => {
                    send_fake(
                        &mut socket,
                        serde_json::json!({ "id": id, "result": { "thread": { "id": "thread-fake-1" } } }),
                    )
                    .await;
                }
                "turn/start" => {
                    turn += 1;
                    let turn_id = format!("turn-fake-{turn}");
                    let prompt = message["params"]["input"][0]["text"]
                        .as_str()
                        .unwrap_or_default();
                    send_fake(
                        &mut socket,
                        serde_json::json!({ "id": id, "result": { "turn": { "id": turn_id } } }),
                    )
                    .await;
                    send_fake(
                        &mut socket,
                        serde_json::json!({ "method": "turn/started", "params": { "threadId": "thread-fake-1", "turn": { "id": turn_id } } }),
                    )
                    .await;
                    if prompt == "approval" {
                        send_fake(
                            &mut socket,
                            serde_json::json!({
                                "id": 900 + turn,
                                "method": "item/commandExecution/requestApproval",
                                "params": { "command": "make check" }
                            }),
                        )
                        .await;
                    } else if prompt == "input" {
                        send_fake(
                            &mut socket,
                            serde_json::json!({
                                "id": 900 + turn,
                                "method": "item/tool/requestUserInput",
                                "params": { "questions": [{ "id": "choice", "question": "Choose" }] }
                            }),
                        )
                        .await;
                    } else {
                        complete_fake_turn(&mut socket, turn).await;
                    }
                }
                "turn/interrupt" => {
                    send_fake(&mut socket, serde_json::json!({ "id": id, "result": {} })).await;
                    send_fake(
                        &mut socket,
                        serde_json::json!({
                            "method": "turn/completed",
                            "params": { "threadId": "thread-fake-1", "turn": { "id": format!("turn-fake-{turn}"), "status": "interrupted" } }
                        }),
                    )
                    .await;
                }
                _ => {}
            }
        }
    }

    #[cfg(unix)]
    async fn complete_fake_turn(
        socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
        turn: u64,
    ) {
        send_fake(
            socket,
            serde_json::json!({ "method": "item/agentMessage/delta", "params": { "delta": "done" } }),
        )
        .await;
        send_fake(
            socket,
            serde_json::json!({
                "method": "item/completed",
                "params": { "item": { "type": "agentMessage", "text": "done" } }
            }),
        )
        .await;
        send_fake(
            socket,
            serde_json::json!({
                "method": "turn/completed",
                "params": { "threadId": "thread-fake-1", "turn": { "id": format!("turn-fake-{turn}"), "status": "completed" } }
            }),
        )
        .await;
    }

    #[cfg(unix)]
    async fn send_fake(
        socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
        value: serde_json::Value,
    ) {
        socket
            .send(WsMessage::Text(value.to_string().into()))
            .await
            .expect("fake response sends");
    }
}
