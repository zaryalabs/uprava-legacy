//! Provider selection and the Codex provider adapter.

use super::*;

pub(crate) const UPRAVA_MCP_TOKEN_ENV_PREFIX: &str = "UPRAVA_MCP_ACCESS_TOKEN_";

#[derive(Debug, Clone)]
pub(crate) enum RuntimeManager {
    Codex(CodexProviderAdapter),
    Unsupported(UnsupportedProviderAdapter),
}

impl RuntimeManager {
    pub(crate) fn for_provider(provider_key: &str, config: &NodeConfig) -> Self {
        match provider_key {
            "codex" => Self::Codex(CodexProviderAdapter::new(config)),
            other => Self::Unsupported(UnsupportedProviderAdapter {
                provider_key: other.to_owned(),
            }),
        }
    }

    pub(crate) async fn execute_deduction(
        &self,
        command: &CommandEnvelope,
        workspace_path: Option<&str>,
        cancellation: Option<watch::Receiver<bool>>,
    ) -> (CommandState, JsonValue) {
        match self {
            Self::Codex(provider) => {
                provider
                    .execute_deduction(command, workspace_path, cancellation)
                    .await
            }
            Self::Unsupported(provider) => deduction_error_payload(
                command,
                &provider.provider_key,
                "deduction.provider_unsupported",
                format!(
                    "Provider `{}` cannot execute structured deductions",
                    provider.provider_key
                ),
            ),
        }
    }
}

/// Agent-only provider boundary. Task execution deliberately does not import it.
pub(crate) enum AgentRuntimeDriver<'a> {
    CodexManaged(CodexManagedDriver<'a>),
    CodexExecCompatibility(CodexExecCompatibilityDriver),
    Unsupported(UnsupportedProviderAdapter),
}

/// Existing stateless `codex exec/resume` compatibility implementation.
pub(crate) type CodexExecCompatibilityDriver = CodexProviderAdapter;

pub(crate) struct CodexManagedDriver<'a> {
    config: &'a NodeConfig,
    supervisor: &'a ManagedRuntimeSupervisor,
}

impl<'a> AgentRuntimeDriver<'a> {
    pub(crate) fn for_command(
        provider_key: &str,
        profile: AgentExecutionProfile,
        config: &'a NodeConfig,
        supervisor: Option<&'a ManagedRuntimeSupervisor>,
    ) -> Self {
        match (provider_key, profile, supervisor) {
            ("codex", AgentExecutionProfile::Managed, Some(supervisor)) => {
                Self::CodexManaged(CodexManagedDriver { config, supervisor })
            }
            ("codex", AgentExecutionProfile::Managed, None) => {
                Self::Unsupported(UnsupportedProviderAdapter {
                    provider_key: "codex.managed_supervisor_unavailable".to_owned(),
                })
            }
            ("codex", AgentExecutionProfile::ExecCompatibility, _) => {
                Self::CodexExecCompatibility(CodexProviderAdapter::new(config))
            }
            (other, _, _) => Self::Unsupported(UnsupportedProviderAdapter {
                provider_key: other.to_owned(),
            }),
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "runtime execution bridges durable runtime maps, workspace context, live events, cancellation, and attempt descriptors"
    )]
    pub(crate) async fn execute_command(
        &self,
        command: &CommandEnvelope,
        runtime_seqs: &mut HashMap<String, i64>,
        workspace_path: Option<&str>,
        runtime_transcripts: &mut HashMap<String, Vec<ProviderTranscriptMessage>>,
        runtime_provider_resume_refs: &mut HashMap<String, ProviderResumeRef>,
        provider_mcp_access: Option<&ProviderMcpAccess>,
        live_event_sink: Option<&mut NodeLiveEventSink<'_>>,
        cancellation: Option<watch::Receiver<bool>>,
        managed_attempts: &mut HashMap<String, ManagedAttemptDescriptor>,
    ) -> Vec<EventEnvelope> {
        match self {
            Self::CodexManaged(driver) => {
                driver
                    .events_for_command(
                        command,
                        runtime_seqs,
                        workspace_path,
                        runtime_provider_resume_refs,
                        provider_mcp_access,
                        live_event_sink,
                        cancellation,
                        managed_attempts,
                    )
                    .await
            }
            Self::CodexExecCompatibility(provider) => {
                provider
                    .events_for_command(
                        command,
                        runtime_seqs,
                        workspace_path,
                        runtime_transcripts,
                        runtime_provider_resume_refs,
                        provider_mcp_access,
                        live_event_sink,
                        cancellation,
                    )
                    .await
            }
            Self::Unsupported(provider) => provider.events_for_command(command, runtime_seqs),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CodexProviderAdapter {
    pub(crate) codex_binary: String,
    pub(crate) ignore_user_config: bool,
    pub(crate) timeout: Duration,
    pub(crate) workspace_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderStartFailure {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

pub(crate) struct CodexProcessOutput {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) stdout_truncated: bool,
    pub(crate) stderr_truncated: bool,
    pub(crate) dropped_activity_count: usize,
    pub(crate) approval_requests: Vec<CodexApprovalRequest>,
    pub(crate) provider_resume_ref: Option<serde_json::Value>,
    pub(crate) activity_events: Vec<EventEnvelope>,
}

impl ProviderStartFailure {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl CodexManagedDriver<'_> {
    #[expect(
        clippy::too_many_arguments,
        reason = "managed execution coordinates durable sequencing, live transport, cancellation, and attempt descriptors"
    )]
    async fn events_for_command(
        &self,
        command: &CommandEnvelope,
        runtime_seqs: &mut HashMap<String, i64>,
        workspace_path: Option<&str>,
        runtime_provider_resume_refs: &mut HashMap<String, ProviderResumeRef>,
        provider_mcp_access: Option<&ProviderMcpAccess>,
        mut live_event_sink: Option<&mut NodeLiveEventSink<'_>>,
        cancellation: Option<watch::Receiver<bool>>,
        managed_attempts: &mut HashMap<String, ManagedAttemptDescriptor>,
    ) -> Vec<EventEnvelope> {
        let Some(runtime_id) = command.target.runtime_session_id().cloned() else {
            return vec![];
        };
        match command.kind {
            CommandKind::StartRuntime | CommandKind::ResumeRuntime => {
                self.start_or_resume(
                    command,
                    runtime_seqs,
                    runtime_id,
                    workspace_path,
                    runtime_provider_resume_refs,
                    provider_mcp_access,
                    managed_attempts,
                )
                .await
            }
            CommandKind::SendTurn => {
                let CommandPayload::SendTurn {
                    content,
                    turn_id,
                    collaboration_mode,
                } = &command.payload
                else {
                    return vec![runtime_error_event(
                        "codex",
                        command,
                        runtime_seqs,
                        runtime_id,
                        None,
                        "protocol.command_payload_mismatch",
                        "SendTurn payload does not match its command kind",
                    )];
                };
                if let Some(access) = provider_mcp_access {
                    if let Err(error) = self
                        .supervisor
                        .refresh_mcp_access(self.config, &runtime_id, access)
                        .await
                    {
                        return vec![runtime_error_event(
                            "codex",
                            command,
                            runtime_seqs,
                            runtime_id,
                            Some(turn_id.clone()),
                            error.code,
                            error.message,
                        )];
                    }
                }
                if let Some(attempt) = managed_attempts.get_mut(runtime_id.as_str()) {
                    attempt.active_turn_id = Some(turn_id.clone());
                }
                match self
                    .supervisor
                    .send_turn(
                        &runtime_id,
                        content.clone(),
                        collaboration_mode.clone(),
                        cancellation,
                    )
                    .await
                {
                    Ok(operation) => {
                        managed_operation_events(
                            command,
                            runtime_seqs,
                            runtime_id,
                            Some(turn_id.clone()),
                            managed_attempts,
                            operation,
                            &mut live_event_sink,
                        )
                        .await
                    }
                    Err(error) => vec![runtime_error_event(
                        "codex",
                        command,
                        runtime_seqs,
                        runtime_id,
                        Some(turn_id.clone()),
                        error.code,
                        error.message,
                    )],
                }
            }
            CommandKind::ResolveApproval => {
                let CommandPayload::ResolveApproval {
                    provider_interaction_id: Some(interaction_id),
                    approved,
                    ..
                } = &command.payload
                else {
                    return vec![runtime_error_event(
                        "codex",
                        command,
                        runtime_seqs,
                        runtime_id,
                        None,
                        "provider.interaction_id_required",
                        "Managed approval resolution requires a provider interaction id",
                    )];
                };
                match self
                    .supervisor
                    .resolve_approval(&runtime_id, interaction_id, *approved)
                    .await
                {
                    Ok(operation) => {
                        managed_operation_events(
                            command,
                            runtime_seqs,
                            runtime_id,
                            None,
                            managed_attempts,
                            operation,
                            &mut live_event_sink,
                        )
                        .await
                    }
                    Err(error) => vec![runtime_error_event(
                        "codex",
                        command,
                        runtime_seqs,
                        runtime_id,
                        None,
                        error.code,
                        error.message,
                    )],
                }
            }
            CommandKind::SubmitUserInput => {
                let CommandPayload::SubmitUserInput {
                    provider_interaction_id,
                    answers,
                } = &command.payload
                else {
                    unreachable!("command payload kind is validated before provider dispatch")
                };
                match self
                    .supervisor
                    .submit_input(&runtime_id, provider_interaction_id, answers)
                    .await
                {
                    Ok(operation) => {
                        managed_operation_events(
                            command,
                            runtime_seqs,
                            runtime_id,
                            None,
                            managed_attempts,
                            operation,
                            &mut live_event_sink,
                        )
                        .await
                    }
                    Err(error) => vec![runtime_error_event(
                        "codex",
                        command,
                        runtime_seqs,
                        runtime_id,
                        None,
                        error.code,
                        error.message,
                    )],
                }
            }
            CommandKind::InterruptRuntime => {
                if let Some(error) =
                    validate_managed_attempt(command, &runtime_id, managed_attempts)
                {
                    return vec![runtime_error_event(
                        "codex",
                        command,
                        runtime_seqs,
                        runtime_id,
                        None,
                        error.code,
                        error.message,
                    )];
                }
                match self.supervisor.interrupt(&runtime_id).await {
                    Ok(operation) => {
                        managed_operation_events(
                            command,
                            runtime_seqs,
                            runtime_id,
                            None,
                            managed_attempts,
                            operation,
                            &mut live_event_sink,
                        )
                        .await
                    }
                    Err(error) => vec![runtime_error_event(
                        "codex",
                        command,
                        runtime_seqs,
                        runtime_id,
                        None,
                        error.code,
                        error.message,
                    )],
                }
            }
            CommandKind::StopRuntime => {
                if let Some(error) =
                    validate_managed_attempt(command, &runtime_id, managed_attempts)
                {
                    return vec![runtime_error_event(
                        "codex",
                        command,
                        runtime_seqs,
                        runtime_id,
                        None,
                        error.code,
                        error.message,
                    )];
                }
                match self.supervisor.stop(&runtime_id).await {
                    Ok(mut stopped) => {
                        if let Some(previous) = managed_attempts.get(runtime_id.as_str()) {
                            stopped.policy_hash = previous.policy_hash.clone();
                            stopped.started_at = previous.started_at;
                        }
                        let attempt_id = stopped.runtime_attempt_id.clone();
                        if let Some(thread_id) = stopped.provider_thread_id.clone() {
                            runtime_provider_resume_refs.insert(
                                runtime_id.to_string(),
                                ProviderResumeRef {
                                    provider_session_id: Some(thread_id),
                                    resume_cursor: None,
                                },
                            );
                        }
                        managed_attempts.insert(runtime_id.to_string(), stopped);
                        let reason = match &command.payload {
                            CommandPayload::StopRuntime { reason, .. } => {
                                reason.as_deref().unwrap_or("explicit_stop")
                            }
                            _ => "explicit_stop",
                        };
                        vec![event_for_command(
                            "codex",
                            command,
                            runtime_seqs,
                            runtime_id,
                            None,
                            EventKind::RuntimeStopped,
                            serde_json::json!({
                                "provider": "codex",
                                "mode": "managed",
                                "runtime_attempt_id": attempt_id,
                                "reason": reason,
                            }),
                        )]
                    }
                    Err(error) => vec![runtime_error_event(
                        "codex",
                        command,
                        runtime_seqs,
                        runtime_id,
                        None,
                        error.code,
                        error.message,
                    )],
                }
            }
            _ => vec![],
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "managed start validates policy and updates durable attempt and resume projections"
    )]
    async fn start_or_resume(
        &self,
        command: &CommandEnvelope,
        runtime_seqs: &mut HashMap<String, i64>,
        runtime_id: RuntimeSessionId,
        workspace_path: Option<&str>,
        runtime_provider_resume_refs: &mut HashMap<String, ProviderResumeRef>,
        provider_mcp_access: Option<&ProviderMcpAccess>,
        managed_attempts: &mut HashMap<String, ManagedAttemptDescriptor>,
    ) -> Vec<EventEnvelope> {
        let (policy, supplied_hash) = match &command.payload {
            CommandPayload::StartRuntime {
                effective_policy,
                effective_policy_hash,
                ..
            }
            | CommandPayload::ResumeRuntime {
                effective_policy,
                effective_policy_hash,
                ..
            } => (effective_policy.as_ref(), effective_policy_hash.as_ref()),
            _ => (None, None),
        };
        let (Some(policy), Some(supplied_hash)) = (policy, supplied_hash) else {
            return vec![runtime_error_event(
                "codex",
                command,
                runtime_seqs,
                runtime_id,
                None,
                "provider.managed_policy_required",
                "Managed Codex start requires an effective policy and hash",
            )];
        };
        if policy.execution_profile != AgentExecutionProfile::Managed {
            return vec![runtime_error_event(
                "codex",
                command,
                runtime_seqs,
                runtime_id,
                None,
                "provider.managed_policy_mismatch",
                "Effective policy does not select the managed execution profile",
            )];
        }
        let calculated_hash = match policy.policy_hash() {
            Ok(hash) if &hash == supplied_hash => hash,
            Ok(_) => {
                return vec![runtime_error_event(
                    "codex",
                    command,
                    runtime_seqs,
                    runtime_id,
                    None,
                    "provider.managed_policy_hash_mismatch",
                    "Effective managed policy hash did not match the supplied snapshot",
                )]
            }
            Err(_) => {
                return vec![runtime_error_event(
                    "codex",
                    command,
                    runtime_seqs,
                    runtime_id,
                    None,
                    "provider.managed_policy_invalid",
                    "Effective managed policy could not be validated",
                )]
            }
        };
        let Some(workspace) = workspace_path else {
            return vec![runtime_error_event(
                "codex",
                command,
                runtime_seqs,
                runtime_id,
                None,
                "provider.workspace_missing",
                "Managed Codex runtime requires a workspace path",
            )];
        };
        let workspace = match canonical_workspace_root_for_allowed_paths(
            &self.config.workspace_paths,
            workspace,
        ) {
            Ok(workspace) => workspace,
            Err(error) => {
                return vec![runtime_error_event(
                    "codex",
                    command,
                    runtime_seqs,
                    runtime_id,
                    None,
                    error.code,
                    error.message,
                )]
            }
        };
        let policy_workspace = match std::fs::canonicalize(&policy.workspace_root) {
            Ok(workspace) => workspace,
            Err(error) => {
                return vec![runtime_error_event(
                    "codex",
                    command,
                    runtime_seqs,
                    runtime_id,
                    None,
                    "provider.managed_policy_workspace_invalid",
                    format!("Effective policy workspace could not be canonicalized: {error}"),
                )]
            }
        };
        if workspace != policy_workspace {
            return vec![runtime_error_event(
                "codex",
                command,
                runtime_seqs,
                runtime_id,
                None,
                "provider.managed_policy_workspace_mismatch",
                "Managed command workspace does not match the immutable effective policy",
            )];
        }
        let workspace = workspace.display().to_string();
        let resume_thread_id = if command.kind == CommandKind::ResumeRuntime {
            command_provider_resume_ref(command)
                .or_else(|| {
                    runtime_provider_resume_refs
                        .get(runtime_id.as_str())
                        .cloned()
                })
                .and_then(|resume_ref| resume_ref.provider_session_id)
        } else {
            None
        };
        match self
            .supervisor
            .start(
                self.config,
                &runtime_id,
                &workspace,
                policy,
                &calculated_hash,
                resume_thread_id.as_deref(),
                provider_mcp_access,
            )
            .await
        {
            Ok(descriptor) => {
                let attempt_id = descriptor.runtime_attempt_id.clone();
                let provider_thread_id = descriptor.provider_thread_id.clone();
                if let Some(thread_id) = provider_thread_id.clone() {
                    runtime_provider_resume_refs.insert(
                        runtime_id.to_string(),
                        ProviderResumeRef {
                            provider_session_id: Some(thread_id),
                            resume_cursor: None,
                        },
                    );
                }
                managed_attempts.insert(runtime_id.to_string(), descriptor);
                let resumed = command.kind == CommandKind::ResumeRuntime;
                vec![
                    event_for_command(
                        "codex",
                        command,
                        runtime_seqs,
                        runtime_id.clone(),
                        None,
                        EventKind::RuntimeAttemptStarted,
                        serde_json::json!({
                            "runtime_attempt_id": attempt_id,
                            "state": "starting",
                            "reason": if resumed { "explicit_resume" } else { "explicit_start" },
                        }),
                    ),
                    event_for_command(
                        "codex",
                        command,
                        runtime_seqs,
                        runtime_id.clone(),
                        None,
                        EventKind::RuntimePolicyEffective,
                        serde_json::json!({
                            "policy": policy,
                            "policy_hash": calculated_hash,
                        }),
                    ),
                    event_for_command(
                        "codex",
                        command,
                        runtime_seqs,
                        runtime_id.clone(),
                        None,
                        if resumed {
                            EventKind::RuntimeAttemptRecovered
                        } else {
                            EventKind::RuntimeAttemptReady
                        },
                        serde_json::json!({
                            "runtime_attempt_id": attempt_id,
                            "state": if resumed { "recovered" } else { "ready" },
                            "reason": if resumed { "provider_native_resume" } else { "handshake_completed" },
                        }),
                    ),
                    event_for_command(
                        "codex",
                        command,
                        runtime_seqs,
                        runtime_id,
                        None,
                        EventKind::RuntimeReady,
                        serde_json::json!({
                            "provider": "codex",
                            "mode": "managed",
                            "runtime_attempt_id": attempt_id,
                            "provider_resume_ref": {
                                "provider_session_id": provider_thread_id,
                            }
                        }),
                    ),
                ]
            }
            Err(error) => vec![runtime_error_event(
                "codex",
                command,
                runtime_seqs,
                runtime_id,
                None,
                error.code,
                error.message,
            )],
        }
    }
}

async fn managed_operation_events(
    command: &CommandEnvelope,
    runtime_seqs: &mut HashMap<String, i64>,
    runtime_id: RuntimeSessionId,
    turn_id: Option<TurnId>,
    managed_attempts: &mut HashMap<String, ManagedAttemptDescriptor>,
    mut operation: ManagedOperation,
    live_event_sink: &mut Option<&mut NodeLiveEventSink<'_>>,
) -> Vec<EventEnvelope> {
    let attempt_id = managed_attempts
        .get(runtime_id.as_str())
        .map(|attempt| attempt.runtime_attempt_id.clone())
        .unwrap_or_else(|| RuntimeAttemptId::from("attempt-unknown"));
    let effective_turn_id = turn_id.or_else(|| {
        managed_attempts
            .get(runtime_id.as_str())
            .and_then(|attempt| attempt.active_turn_id.clone())
    });
    let mut events = Vec::new();
    while let Some(update) = operation.updates.recv().await {
        let (kind, payload, update_turn_id) = match update {
            ManagedRuntimeUpdate::TurnStarted => (
                EventKind::TurnStarted,
                serde_json::json!({}),
                effective_turn_id.clone(),
            ),
            ManagedRuntimeUpdate::OutputDelta(content) => (
                EventKind::ProviderOutputDelta,
                serde_json::json!({ "content": content }),
                effective_turn_id.clone(),
            ),
            ManagedRuntimeUpdate::MessageCompleted(content) => (
                EventKind::ProviderMessageCompleted,
                serde_json::json!({ "content": content }),
                effective_turn_id.clone(),
            ),
            ManagedRuntimeUpdate::Activity {
                method,
                payload,
                unknown,
            } => (
                EventKind::ProviderActivity,
                serde_json::json!({
                    "provider": "codex",
                    "source": "app-server-v2",
                    "provider_event_type": method,
                    "phase": "observed",
                    "status": if unknown { "protocol_drift" } else { "observed" },
                    "raw_event": payload,
                }),
                effective_turn_id.clone(),
            ),
            ManagedRuntimeUpdate::InteractionRequested {
                interaction_id,
                kind,
                prompt,
            } => (
                EventKind::ProviderInteractionRequested,
                serde_json::json!({
                    "provider_interaction_id": interaction_id,
                    "runtime_attempt_id": attempt_id,
                    "interaction_kind": kind,
                    "prompt": prompt,
                    "expires_at": Utc::now() + chrono::Duration::hours(24),
                }),
                effective_turn_id.clone(),
            ),
            ManagedRuntimeUpdate::InteractionResolved {
                interaction_id,
                kind,
                approved,
                answers,
            } => (
                EventKind::ProviderInteractionResolved,
                serde_json::json!({
                    "provider_interaction_id": interaction_id,
                    "runtime_attempt_id": attempt_id,
                    "interaction_kind": kind,
                    "approved": approved,
                    "answers": answers,
                }),
                effective_turn_id.clone(),
            ),
            ManagedRuntimeUpdate::TurnCompleted => (
                EventKind::TurnCompleted,
                serde_json::json!({}),
                effective_turn_id.clone(),
            ),
            ManagedRuntimeUpdate::TurnInterrupted => (
                EventKind::TurnInterrupted,
                serde_json::json!({
                    "provider": "codex",
                    "code": "provider.interrupted",
                    "message": "Codex managed turn was interrupted",
                }),
                effective_turn_id.clone(),
            ),
            ManagedRuntimeUpdate::Failed { code, message } => (
                EventKind::RuntimeError,
                serde_json::json!({ "code": code, "message": message }),
                effective_turn_id.clone(),
            ),
        };
        let event = event_for_command(
            "codex",
            command,
            runtime_seqs,
            runtime_id.clone(),
            update_turn_id,
            kind,
            payload,
        );
        if let Some(sink) = live_event_sink.as_mut() {
            sink.emit(&event);
        }
        let terminal = matches!(
            kind,
            EventKind::ProviderInteractionRequested
                | EventKind::TurnCompleted
                | EventKind::TurnInterrupted
                | EventKind::RuntimeError
        );
        events.push(event);
        if terminal {
            if !matches!(kind, EventKind::ProviderInteractionRequested) {
                if let Some(attempt) = managed_attempts.get_mut(runtime_id.as_str()) {
                    attempt.active_turn_id = None;
                    if kind == EventKind::RuntimeError {
                        attempt.state = RuntimeAttemptState::Failed;
                        attempt.stopped_at = Some(Utc::now());
                        attempt.terminal_reason = Some("managed_operation_failed".to_owned());
                    }
                }
            }
            match kind {
                EventKind::ProviderInteractionRequested => events.push(event_for_command(
                    "codex",
                    command,
                    runtime_seqs,
                    runtime_id.clone(),
                    None,
                    EventKind::RuntimeBlocked,
                    serde_json::json!({
                        "provider": "codex",
                        "mode": "managed",
                        "reason": "provider_interaction_requested",
                    }),
                )),
                EventKind::TurnCompleted | EventKind::TurnInterrupted => {
                    events.push(event_for_command(
                        "codex",
                        command,
                        runtime_seqs,
                        runtime_id.clone(),
                        None,
                        EventKind::RuntimeReady,
                        serde_json::json!({ "provider": "codex", "mode": "managed" }),
                    ))
                }
                _ => {}
            }
            break;
        }
    }
    events
}

fn validate_managed_attempt(
    command: &CommandEnvelope,
    runtime_id: &RuntimeSessionId,
    attempts: &HashMap<String, ManagedAttemptDescriptor>,
) -> Option<ManagedRuntimeError> {
    let requested = match &command.payload {
        CommandPayload::InterruptRuntime { runtime_attempt_id }
        | CommandPayload::StopRuntime {
            runtime_attempt_id, ..
        } => runtime_attempt_id.as_ref(),
        _ => None,
    }?;
    let current = attempts.get(runtime_id.as_str())?;
    (requested != &current.runtime_attempt_id).then(|| {
        ManagedRuntimeError::new(
            "provider.runtime_attempt_conflict",
            "The command targets a stale managed runtime attempt",
        )
    })
}

impl CodexProviderAdapter {
    pub(crate) fn new(config: &NodeConfig) -> Self {
        Self {
            codex_binary: config.codex_binary.clone(),
            ignore_user_config: config.codex_ignore_user_config,
            timeout: config.codex_timeout,
            workspace_paths: config.workspace_paths.clone(),
        }
    }

    pub(crate) fn provider_key(&self) -> &'static str {
        "codex"
    }

    pub(crate) fn authorized_workspace_path(
        &self,
        workspace_path: &str,
    ) -> Result<String, WorkspaceInspectError> {
        canonical_workspace_root_for_allowed_paths(&self.workspace_paths, workspace_path)
            .map(|path| path.display().to_string())
    }

    pub(crate) async fn execute_deduction(
        &self,
        command_context: &CommandEnvelope,
        workspace_path: Option<&str>,
        cancellation: Option<watch::Receiver<bool>>,
    ) -> (CommandState, JsonValue) {
        let CommandPayload::RequestDeduction { package } = &command_context.payload else {
            return deduction_error_payload(
                command_context,
                self.provider_key(),
                "protocol.command_payload_mismatch",
                "RequestDeduction payload does not match its command kind",
            );
        };
        let Some(runtime_session_id) = command_context.target.runtime_session_id().cloned() else {
            return deduction_error_payload(
                command_context,
                self.provider_key(),
                "deduction.runtime_missing",
                "Deduction command is missing a runtime session target",
            );
        };
        let Some(workspace_path) = workspace_path.filter(|value| !value.trim().is_empty()) else {
            return deduction_error_payload(
                command_context,
                self.provider_key(),
                "deduction.workspace_missing",
                "Deduction requires the session workspace path",
            );
        };
        let workspace_path = match self.authorized_workspace_path(workspace_path) {
            Ok(path) => path,
            Err(error) => {
                return deduction_error_payload(
                    command_context,
                    self.provider_key(),
                    error.code,
                    error.message,
                );
            }
        };

        let last_message_path = codex_last_message_path(&command_context.command_id);
        let schema_path = codex_deduction_schema_path(&command_context.command_id);
        let schema = deduction_output_schema(package);
        if let Err(error) = std::fs::write(
            &schema_path,
            serde_json::to_vec_pretty(&schema).unwrap_or_default(),
        ) {
            return deduction_error_payload(
                command_context,
                self.provider_key(),
                "deduction.schema_write_failed",
                format!("Could not write the temporary deduction schema: {error}"),
            );
        }
        let prompt = deduction_prompt(package);
        let mut command = TokioCommand::new(&self.codex_binary);
        configure_provider_environment(&mut command);
        command.arg("exec");
        if self.ignore_user_config {
            command.arg("--ignore-user-config");
        }
        command
            .arg("--cd")
            .arg(&workspace_path)
            .arg("--ephemeral")
            .arg("--sandbox")
            .arg("read-only")
            .arg("--skip-git-repo-check")
            .arg("--json")
            .arg("--output-schema")
            .arg(&schema_path)
            .arg("--output-last-message")
            .arg(&last_message_path)
            .arg(prompt)
            .current_dir(&workspace_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        configure_provider_process(&mut command);

        let mut isolated_seqs = HashMap::new();
        let output = self
            .run_codex_command(
                command,
                "Codex deduction",
                command_context,
                &mut isolated_seqs,
                &runtime_session_id,
                None,
                None,
                cancellation,
            )
            .await;
        let raw_text = std::fs::read_to_string(&last_message_path).unwrap_or_default();
        let _ = std::fs::remove_file(&last_message_path);
        let _ = std::fs::remove_file(&schema_path);
        let raw_truncated = raw_text.chars().count() > MAX_DEDUCTION_RAW_CHARS;
        let raw_text = bounded_text(&raw_text, MAX_DEDUCTION_RAW_CHARS);

        match output {
            Ok(output) if output.status.success() => {
                let parsed = serde_json::from_str::<DeductionProviderResult>(raw_text.trim());
                let (result, error_code, error_message) = match parsed {
                    Ok(result) => (Some(result), None, None),
                    Err(error) => (
                        None,
                        Some("deduction.output_invalid_json".to_owned()),
                        Some(format!(
                            "Structured deduction output could not be parsed: {error}"
                        )),
                    ),
                };
                deduction_output_payload(
                    package,
                    self.provider_key(),
                    result,
                    raw_text,
                    raw_truncated,
                    error_code,
                    error_message,
                    CommandState::Completed,
                )
            }
            Ok(output) => deduction_output_payload(
                package,
                self.provider_key(),
                None,
                raw_text,
                raw_truncated,
                Some("deduction.provider_failed".to_owned()),
                Some(codex_failure_message(&output)),
                CommandState::Failed,
            ),
            Err(error) => deduction_output_payload(
                package,
                self.provider_key(),
                None,
                raw_text,
                raw_truncated,
                Some(error.code.to_owned()),
                Some(error.message),
                CommandState::Failed,
            ),
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "provider command execution updates runtime sequence, transcript, resume, live event, and cancellation state"
    )]
    pub(crate) async fn events_for_command(
        &self,
        command: &CommandEnvelope,
        runtime_seqs: &mut HashMap<String, i64>,
        workspace_path: Option<&str>,
        runtime_transcripts: &mut HashMap<String, Vec<ProviderTranscriptMessage>>,
        runtime_provider_resume_refs: &mut HashMap<String, ProviderResumeRef>,
        provider_mcp_access: Option<&ProviderMcpAccess>,
        live_event_sink: Option<&mut NodeLiveEventSink<'_>>,
        cancellation: Option<watch::Receiver<bool>>,
    ) -> Vec<EventEnvelope> {
        let Some(runtime_session_id) = command.target.runtime_session_id().cloned() else {
            return vec![];
        };
        match command.kind {
            CommandKind::StartRuntime => {
                runtime_transcripts.insert(runtime_session_id.to_string(), Vec::new());
                runtime_provider_resume_refs.remove(runtime_session_id.as_str());
                vec![
                    event_for_command(
                        self.provider_key(),
                        command,
                        runtime_seqs,
                        runtime_session_id.clone(),
                        None,
                        EventKind::RuntimeStarting,
                        serde_json::json!({ "provider": self.provider_key(), "mode": "exec" }),
                    ),
                    event_for_command(
                        self.provider_key(),
                        command,
                        runtime_seqs,
                        runtime_session_id,
                        None,
                        EventKind::RuntimeReady,
                        serde_json::json!({
                            "provider": self.provider_key(),
                            "mode": "exec",
                            "resume_source": "node_local_transcript",
                        }),
                    ),
                ]
            }
            CommandKind::ResumeRuntime => {
                let transcript_len = runtime_transcripts
                    .get(runtime_session_id.as_str())
                    .map(Vec::len)
                    .unwrap_or(0);
                let provider_resume_ref = command_provider_resume_ref(command).or_else(|| {
                    runtime_provider_resume_refs
                        .get(runtime_session_id.as_str())
                        .cloned()
                });
                if let Some(provider_resume_ref) = provider_resume_ref {
                    runtime_provider_resume_refs
                        .insert(runtime_session_id.to_string(), provider_resume_ref.clone());
                    return vec![
                        event_for_command(
                            self.provider_key(),
                            command,
                            runtime_seqs,
                            runtime_session_id.clone(),
                            None,
                            EventKind::RuntimeResuming,
                            serde_json::json!({
                                "provider": self.provider_key(),
                                "mode": "exec",
                                "resume_source": "provider_resume_ref",
                            }),
                        ),
                        event_for_command(
                            self.provider_key(),
                            command,
                            runtime_seqs,
                            runtime_session_id,
                            None,
                            EventKind::RuntimeReady,
                            serde_json::json!({
                                "provider": self.provider_key(),
                                "mode": "exec",
                                "resume_source": "provider_resume_ref",
                                "provider_resume_ref": provider_resume_ref_json(&provider_resume_ref),
                            }),
                        ),
                    ];
                }
                vec![
                    event_for_command(
                        self.provider_key(),
                        command,
                        runtime_seqs,
                        runtime_session_id.clone(),
                        None,
                        EventKind::RuntimeResuming,
                        serde_json::json!({ "provider": self.provider_key(), "mode": "exec" }),
                    ),
                    event_for_command(
                        self.provider_key(),
                        command,
                        runtime_seqs,
                        runtime_session_id,
                        None,
                        EventKind::RuntimeReady,
                        serde_json::json!({
                            "provider": self.provider_key(),
                            "mode": "exec",
                            "resume_source": "node_local_transcript",
                            "transcript_messages": transcript_len,
                        }),
                    ),
                ]
            }
            CommandKind::SendTurn => {
                self.send_turn_events(
                    command,
                    runtime_seqs,
                    runtime_session_id,
                    workspace_path,
                    runtime_transcripts,
                    runtime_provider_resume_refs,
                    provider_mcp_access,
                    live_event_sink,
                    cancellation,
                )
                .await
            }
            CommandKind::ResolveApproval => {
                let CommandPayload::ResolveApproval {
                    approval_id,
                    approved,
                    message,
                    ..
                } = &command.payload
                else {
                    return vec![runtime_error_event(
                        self.provider_key(),
                        command,
                        runtime_seqs,
                        runtime_session_id,
                        None,
                        "protocol.command_payload_mismatch",
                        "ResolveApproval payload does not match its command kind",
                    )];
                };
                let default_message = if *approved {
                    "Approval accepted"
                } else {
                    "Approval denied"
                };
                let message = message.as_deref().unwrap_or(default_message);
                vec![
                    event_for_command(
                        self.provider_key(),
                        command,
                        runtime_seqs,
                        runtime_session_id.clone(),
                        None,
                        EventKind::ApprovalResolved,
                        serde_json::json!({
                            "approval_id": approval_id,
                            "approved": *approved,
                            "message": message,
                        }),
                    ),
                    event_for_command(
                        self.provider_key(),
                        command,
                        runtime_seqs,
                        runtime_session_id,
                        None,
                        EventKind::RuntimeReady,
                        serde_json::json!({ "provider": self.provider_key(), "mode": "exec" }),
                    ),
                ]
            }
            CommandKind::InterruptRuntime => vec![event_for_command(
                self.provider_key(),
                command,
                runtime_seqs,
                runtime_session_id,
                None,
                EventKind::RuntimeError,
                serde_json::json!({
                    "code": "provider.interrupt_unsupported",
                    "message": "Codex interrupt is not supported by the stateless exec adapter yet",
                }),
            )],
            CommandKind::StopRuntime => vec![event_for_command(
                self.provider_key(),
                command,
                runtime_seqs,
                runtime_session_id,
                None,
                EventKind::RuntimeStopped,
                serde_json::json!({ "provider": self.provider_key(), "mode": "exec" }),
            )],
            _ => vec![],
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "provider turn execution carries command, runtime, transcript, resume, and live stream state"
    )]
    pub(crate) async fn send_turn_events(
        &self,
        command: &CommandEnvelope,
        runtime_seqs: &mut HashMap<String, i64>,
        runtime_session_id: RuntimeSessionId,
        workspace_path: Option<&str>,
        runtime_transcripts: &mut HashMap<String, Vec<ProviderTranscriptMessage>>,
        runtime_provider_resume_refs: &mut HashMap<String, ProviderResumeRef>,
        provider_mcp_access: Option<&ProviderMcpAccess>,
        mut live_event_sink: Option<&mut NodeLiveEventSink<'_>>,
        cancellation: Option<watch::Receiver<bool>>,
    ) -> Vec<EventEnvelope> {
        let CommandPayload::SendTurn {
            content, turn_id, ..
        } = &command.payload
        else {
            return vec![runtime_error_event(
                self.provider_key(),
                command,
                runtime_seqs,
                runtime_session_id,
                None,
                "protocol.command_payload_mismatch",
                "SendTurn payload does not match its command kind",
            )];
        };
        let turn_id = Some(turn_id.clone());
        let mut events = vec![
            event_for_command(
                self.provider_key(),
                command,
                runtime_seqs,
                runtime_session_id.clone(),
                None,
                EventKind::RuntimeRunning,
                serde_json::json!({ "provider": self.provider_key(), "mode": "exec" }),
            ),
            event_for_command(
                self.provider_key(),
                command,
                runtime_seqs,
                runtime_session_id.clone(),
                turn_id.clone(),
                EventKind::TurnStarted,
                serde_json::json!({}),
            ),
        ];
        if let Some(sink) = live_event_sink.as_mut() {
            for event in &events {
                sink.emit(event);
            }
        }

        let Some(workspace_path) = workspace_path.filter(|value| !value.trim().is_empty()) else {
            events.push(runtime_error_event(
                self.provider_key(),
                command,
                runtime_seqs,
                runtime_session_id,
                turn_id,
                "provider.workspace_missing",
                "Codex provider requires a workspace path from StartRuntime",
            ));
            return events;
        };
        let workspace_path = match self.authorized_workspace_path(workspace_path) {
            Ok(path) => path,
            Err(error) => {
                events.push(runtime_error_event(
                    self.provider_key(),
                    command,
                    runtime_seqs,
                    runtime_session_id,
                    turn_id,
                    error.code,
                    error.message,
                ));
                return events;
            }
        };

        let last_message_path = codex_last_message_path(&command.command_id);
        let provider_resume_ref = runtime_provider_resume_refs
            .get(runtime_session_id.as_str())
            .cloned();
        let output = if let Some(provider_session_id) = provider_resume_ref
            .as_ref()
            .and_then(|resume_ref| resume_ref.provider_session_id.as_deref())
        {
            self.run_codex_exec_resume(
                &workspace_path,
                provider_session_id,
                content,
                &last_message_path,
                command,
                runtime_seqs,
                &runtime_session_id,
                turn_id.clone(),
                provider_mcp_access,
                live_event_sink,
                cancellation,
            )
            .await
        } else {
            let prompt = codex_exec_prompt(
                content,
                runtime_transcripts
                    .get(runtime_session_id.as_str())
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
            );
            self.run_codex_exec(
                &workspace_path,
                &prompt,
                &last_message_path,
                command,
                runtime_seqs,
                &runtime_session_id,
                turn_id.clone(),
                provider_mcp_access,
                live_event_sink,
                cancellation,
            )
            .await
        };
        let last_message = std::fs::read_to_string(&last_message_path).unwrap_or_default();
        let _ = std::fs::remove_file(&last_message_path);

        match output {
            Ok(output) if output.status.success() => {
                events.extend(output.activity_events.iter().cloned());
                append_codex_process_limit_events(
                    self.provider_key(),
                    command,
                    runtime_seqs,
                    runtime_session_id.clone(),
                    turn_id.clone(),
                    &output,
                    &mut events,
                );
                let approval_requests = codex_approval_requests_from_output(&output);
                let provider_resume_ref = codex_resume_ref_from_output(&output);
                if let Some(provider_resume_ref) = provider_resume_ref
                    .as_ref()
                    .and_then(provider_resume_ref_from_json)
                {
                    runtime_provider_resume_refs
                        .insert(runtime_session_id.to_string(), provider_resume_ref);
                }
                if !approval_requests.is_empty() {
                    for approval_request in approval_requests {
                        events.push(event_for_command(
                            self.provider_key(),
                            command,
                            runtime_seqs,
                            runtime_session_id.clone(),
                            turn_id.clone(),
                            EventKind::ApprovalRequested,
                            serde_json::json!({
                                "approval_id": approval_request.approval_id.as_str(),
                                "prompt": approval_request.prompt,
                                "provider": self.provider_key(),
                                "provider_event_type": approval_request.provider_event_type,
                                "source": "codex.exec.jsonl",
                            }),
                        ));
                    }
                    events.push(event_for_command(
                        self.provider_key(),
                        command,
                        runtime_seqs,
                        runtime_session_id,
                        None,
                        EventKind::RuntimeBlocked,
                        serde_json::json!({
                            "provider": self.provider_key(),
                            "mode": "exec",
                            "reason": "provider_approval_requested",
                        }),
                    ));
                    return events;
                }

                let assistant_content = last_message.trim();
                if assistant_content.is_empty() {
                    events.push(runtime_error_event(
                        self.provider_key(),
                        command,
                        runtime_seqs,
                        runtime_session_id,
                        turn_id,
                        "provider.empty_output",
                        "Codex exec completed without a final assistant message",
                    ));
                } else {
                    record_codex_transcript_turn(
                        runtime_transcripts,
                        &runtime_session_id,
                        content,
                        assistant_content,
                    );
                    events.extend([
                        event_for_command(
                            self.provider_key(),
                            command,
                            runtime_seqs,
                            runtime_session_id.clone(),
                            turn_id.clone(),
                            EventKind::ProviderMessageCompleted,
                            serde_json::json!({ "content": assistant_content }),
                        ),
                        event_for_command(
                            self.provider_key(),
                            command,
                            runtime_seqs,
                            runtime_session_id.clone(),
                            turn_id,
                            EventKind::TurnCompleted,
                            serde_json::json!({}),
                        ),
                        event_for_command(
                            self.provider_key(),
                            command,
                            runtime_seqs,
                            runtime_session_id,
                            None,
                            EventKind::RuntimeReady,
                            codex_runtime_ready_payload(self.provider_key(), provider_resume_ref),
                        ),
                    ]);
                }
            }
            Ok(output) => {
                events.extend(output.activity_events.iter().cloned());
                append_codex_process_limit_events(
                    self.provider_key(),
                    command,
                    runtime_seqs,
                    runtime_session_id.clone(),
                    turn_id.clone(),
                    &output,
                    &mut events,
                );
                events.push(runtime_error_event(
                    self.provider_key(),
                    command,
                    runtime_seqs,
                    runtime_session_id,
                    turn_id,
                    "provider.exec_failed",
                    codex_failure_message(&output),
                ));
            }
            Err(error) if error.code == "provider.cancelled" => {
                events.push(event_for_command(
                    self.provider_key(),
                    command,
                    runtime_seqs,
                    runtime_session_id,
                    turn_id,
                    EventKind::TurnInterrupted,
                    serde_json::json!({
                        "provider": self.provider_key(),
                        "code": error.code,
                        "message": error.message,
                    }),
                ));
            }
            Err(error) => {
                events.push(runtime_error_event(
                    self.provider_key(),
                    command,
                    runtime_seqs,
                    runtime_session_id,
                    turn_id,
                    error.code,
                    error.message,
                ));
            }
        }
        events
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Codex process launch needs workspace, output file, runtime, and live stream context"
    )]
    pub(crate) async fn run_codex_exec(
        &self,
        workspace_path: &str,
        content: &str,
        last_message_path: &Path,
        command_context: &CommandEnvelope,
        runtime_seqs: &mut HashMap<String, i64>,
        runtime_session_id: &RuntimeSessionId,
        turn_id: Option<TurnId>,
        provider_mcp_access: Option<&ProviderMcpAccess>,
        live_event_sink: Option<&mut NodeLiveEventSink<'_>>,
        cancellation: Option<watch::Receiver<bool>>,
    ) -> Result<CodexProcessOutput, ProviderStartFailure> {
        let mut command = TokioCommand::new(&self.codex_binary);
        configure_provider_environment(&mut command);
        configure_uprava_mcp(&mut command, provider_mcp_access)?;
        command.arg("exec");
        if self.ignore_user_config {
            command.arg("--ignore-user-config");
        }
        command
            .arg("--cd")
            .arg(workspace_path)
            .arg("--skip-git-repo-check")
            .arg("--dangerously-bypass-approvals-and-sandbox")
            .arg("--json")
            .arg("--output-last-message")
            .arg(last_message_path)
            .arg(content)
            .current_dir(workspace_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        configure_provider_process(&mut command);

        self.run_codex_command(
            command,
            "Codex exec",
            command_context,
            runtime_seqs,
            runtime_session_id,
            turn_id,
            live_event_sink,
            cancellation,
        )
        .await
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Codex resume launch needs resume id plus the same runtime and live stream context"
    )]
    pub(crate) async fn run_codex_exec_resume(
        &self,
        workspace_path: &str,
        provider_session_id: &str,
        content: &str,
        last_message_path: &Path,
        command_context: &CommandEnvelope,
        runtime_seqs: &mut HashMap<String, i64>,
        runtime_session_id: &RuntimeSessionId,
        turn_id: Option<TurnId>,
        provider_mcp_access: Option<&ProviderMcpAccess>,
        live_event_sink: Option<&mut NodeLiveEventSink<'_>>,
        cancellation: Option<watch::Receiver<bool>>,
    ) -> Result<CodexProcessOutput, ProviderStartFailure> {
        let mut command = TokioCommand::new(&self.codex_binary);
        configure_provider_environment(&mut command);
        configure_uprava_mcp(&mut command, provider_mcp_access)?;
        command.arg("exec");
        if self.ignore_user_config {
            command.arg("--ignore-user-config");
        }
        command
            .arg("resume")
            .arg("--skip-git-repo-check")
            .arg("--dangerously-bypass-approvals-and-sandbox")
            .arg("--json")
            .arg("--output-last-message")
            .arg(last_message_path)
            .arg(provider_session_id)
            .arg(content)
            .current_dir(workspace_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        configure_provider_process(&mut command);

        self.run_codex_command(
            command,
            "Codex exec resume",
            command_context,
            runtime_seqs,
            runtime_session_id,
            turn_id,
            live_event_sink,
            cancellation,
        )
        .await
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "shared child-process reader carries command, runtime sequence, turn, and live sink context"
    )]
    pub(crate) async fn run_codex_command(
        &self,
        mut command: TokioCommand,
        command_label: &'static str,
        command_context: &CommandEnvelope,
        runtime_seqs: &mut HashMap<String, i64>,
        runtime_session_id: &RuntimeSessionId,
        turn_id: Option<TurnId>,
        mut live_event_sink: Option<&mut NodeLiveEventSink<'_>>,
        mut cancellation: Option<watch::Receiver<bool>>,
    ) -> Result<CodexProcessOutput, ProviderStartFailure> {
        let mut child = command.spawn().map_err(|error| {
            let code = if error.kind() == ErrorKind::NotFound {
                "provider.missing_binary"
            } else {
                "provider.start_failed"
            };
            ProviderStartFailure::new(
                code,
                format!(
                    "{command_label} could not start using `{}`: {error}",
                    self.codex_binary
                ),
            )
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            ProviderStartFailure::new(
                "provider.start_failed",
                format!("{command_label} did not expose stdout"),
            )
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            ProviderStartFailure::new(
                "provider.start_failed",
                format!("{command_label} did not expose stderr"),
            )
        })?;

        let run = async {
            let mut stdout_lines = BufReader::new(stdout).lines();
            let mut stderr_lines = BufReader::new(stderr).lines();
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let mut stdout_truncated = false;
            let mut stderr_truncated = false;
            let mut activity_events = Vec::new();
            let mut approval_requests = Vec::new();
            let mut provider_resume_ref = None;
            let mut dropped_activity_count = 0usize;
            let mut stdout_done = false;
            let mut stderr_done = false;
            let mut status = None;
            let wait = child.wait();
            tokio::pin!(wait);

            loop {
                if status.is_some() && stdout_done && stderr_done {
                    break;
                }

                tokio::select! {
                    line = stdout_lines.next_line(), if !stdout_done => {
                        match line {
                            Ok(Some(line)) => {
                                if approval_requests.len() < MAX_PROVIDER_APPROVAL_REQUESTS {
                                    if let Some(approval_request) =
                                        codex_approval_request_from_json_line(&line)
                                    {
                                        approval_requests.push(approval_request);
                                    }
                                }
                                if provider_resume_ref.is_none() {
                                    provider_resume_ref = codex_resume_ref_from_json_line(&line);
                                }
                                append_capped_process_line(
                                    &mut stdout,
                                    &line,
                                    MAX_PROVIDER_PROCESS_OUTPUT_BYTES,
                                    &mut stdout_truncated,
                                );
                                let event = codex_stdout_activity_event(
                                    self.provider_key(),
                                    command_context,
                                    runtime_seqs,
                                    runtime_session_id.clone(),
                                    turn_id.clone(),
                                    &line,
                                );
                                emit_codex_activity_event(
                                    &mut activity_events,
                                    &mut dropped_activity_count,
                                    event,
                                    &mut live_event_sink,
                                )
                                .await?;
                            }
                            Ok(None) => stdout_done = true,
                            Err(error) => {
                                return Err(ProviderStartFailure::new(
                                    "provider.stdout_read_failed",
                                    format!("{command_label} stdout could not be read: {error}"),
                                ));
                            }
                        }
                    }
                    line = stderr_lines.next_line(), if !stderr_done => {
                        match line {
                            Ok(Some(line)) => {
                                append_capped_process_line(
                                    &mut stderr,
                                    &line,
                                    MAX_PROVIDER_PROCESS_OUTPUT_BYTES,
                                    &mut stderr_truncated,
                                );
                                let event = codex_stderr_activity_event(
                                    self.provider_key(),
                                    command_context,
                                    runtime_seqs,
                                    runtime_session_id.clone(),
                                    turn_id.clone(),
                                    &line,
                                );
                                emit_codex_activity_event(
                                    &mut activity_events,
                                    &mut dropped_activity_count,
                                    event,
                                    &mut live_event_sink,
                                )
                                .await?;
                            }
                            Ok(None) => stderr_done = true,
                            Err(error) => {
                                return Err(ProviderStartFailure::new(
                                    "provider.stderr_read_failed",
                                    format!("{command_label} stderr could not be read: {error}"),
                                ));
                            }
                        }
                    }
                    wait_result = &mut wait, if status.is_none() => {
                        status = Some(wait_result.map_err(|error| {
                            ProviderStartFailure::new(
                                "provider.wait_failed",
                                format!("{command_label} wait failed: {error}"),
                            )
                        })?);
                    }
                    _ = wait_for_runtime_cancellation(&mut cancellation), if cancellation.is_some() => {
                        return Err(ProviderStartFailure::new(
                            "provider.cancelled",
                            format!("{command_label} was cancelled by a runtime control command"),
                        ));
                    }
                }
            }

            let status = status.ok_or_else(|| {
                ProviderStartFailure::new(
                    "provider.wait_failed",
                    format!("{command_label} exited without a status"),
                )
            })?;

            Ok(CodexProcessOutput {
                status,
                stdout,
                stderr,
                stdout_truncated,
                stderr_truncated,
                dropped_activity_count,
                approval_requests,
                provider_resume_ref,
                activity_events,
            })
        };

        match timeout(self.timeout, run).await {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(error)) => {
                if error.code == "provider.cancelled" {
                    terminate_provider_process(&mut child).await;
                }
                Err(error)
            }
            Err(_) => {
                terminate_provider_process(&mut child).await;
                Err(ProviderStartFailure::new(
                    "provider.execution_timeout",
                    format!(
                        "{command_label} timed out after {} seconds",
                        self.timeout.as_secs()
                    ),
                ))
            }
        }
    }
}

pub(crate) fn configure_uprava_mcp(
    command: &mut TokioCommand,
    access: Option<&ProviderMcpAccess>,
) -> Result<(), ProviderStartFailure> {
    let Some(access) = access else {
        return Ok(());
    };
    let endpoint = Url::parse(&access.endpoint_url).map_err(|_| {
        ProviderStartFailure::new(
            "provider.mcp_endpoint_invalid",
            "Core returned an invalid Uprava MCP endpoint",
        )
    })?;
    if !matches!(endpoint.scheme(), "http" | "https") {
        return Err(ProviderStartFailure::new(
            "provider.mcp_endpoint_invalid",
            "Core returned an unsupported Uprava MCP endpoint",
        ));
    }
    let endpoint_literal = serde_json::to_string(endpoint.as_str()).map_err(|_| {
        ProviderStartFailure::new(
            "provider.mcp_endpoint_invalid",
            "Uprava MCP endpoint could not be encoded for Codex",
        )
    })?;
    // Codex persists shell environment snapshots between app-server launches. A stable secret
    // variable name can therefore resolve to a previous runtime's revoked lease even though the
    // child process received the current value. A per-process name keeps the credential ephemeral
    // without placing its value in argv or provider configuration.
    let token_env = format!("{UPRAVA_MCP_TOKEN_ENV_PREFIX}{}", Uuid::new_v4().simple());
    command
        .arg("--disable")
        .arg("shell_snapshot")
        .arg("--config")
        .arg("shell_environment_policy.inherit=all")
        .arg("--config")
        .arg(format!("mcp_servers.uprava.url={endpoint_literal}"))
        .arg("--config")
        .arg(format!(
            "mcp_servers.uprava.bearer_token_env_var=\"{token_env}\""
        ))
        .arg("--config")
        .arg("mcp_servers.uprava.default_tools_approval_mode=\"approve\"")
        .env(token_env, access.access_token.expose_secret());
    Ok(())
}

const PROVIDER_ENV_ALLOWLIST: &[&str] = &[
    "PATH",
    "PATHEXT",
    "SYSTEMROOT",
    "COMSPEC",
    "SHELL",
    "CODEX_SHELL",
    "HOME",
    "USER",
    "LOGNAME",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "TEMP",
    "TMP",
    "TMPDIR",
    "CODEX_HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "XDG_RUNTIME_DIR",
    "LANG",
    "LC_ALL",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
    "OPENAI_API_KEY",
    "OPENAI_BASE_URL",
    "OPENAI_ORGANIZATION",
    "OPENAI_PROJECT",
    "AZURE_OPENAI_API_KEY",
    "AZURE_OPENAI_ENDPOINT",
    "AZURE_OPENAI_API_VERSION",
];

pub(crate) fn configure_provider_environment(command: &mut TokioCommand) {
    let inherited = PROVIDER_ENV_ALLOWLIST
        .iter()
        .filter_map(|name| std::env::var_os(name).map(|value| (*name, value)))
        .collect::<Vec<_>>();
    command.env_clear().envs(inherited);
}

#[cfg(unix)]
pub(crate) fn configure_provider_process(command: &mut TokioCommand) {
    command.as_std_mut().process_group(0);
}

#[cfg(not(unix))]
pub(crate) fn configure_provider_process(_command: &mut TokioCommand) {}

pub(crate) async fn terminate_provider_process(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        signal_provider_process_group(pid, "-TERM").await;
    }
    #[cfg(not(unix))]
    let _ = child.start_kill();

    if timeout(PROVIDER_PROCESS_SHUTDOWN_TIMEOUT, child.wait())
        .await
        .is_ok()
    {
        return;
    }

    #[cfg(unix)]
    if let Some(pid) = child.id() {
        signal_provider_process_group(pid, "-KILL").await;
    }
    #[cfg(not(unix))]
    let _ = child.start_kill();
    let _ = timeout(PROVIDER_PROCESS_SHUTDOWN_TIMEOUT, child.wait()).await;
}

#[cfg(unix)]
pub(crate) async fn signal_provider_process_group(pid: u32, signal: &str) {
    let _ = TokioCommand::new("/bin/kill")
        .arg(signal)
        .arg("--")
        .arg(format!("-{pid}"))
        .status()
        .await;
}

pub(crate) async fn wait_for_runtime_cancellation(
    cancellation: &mut Option<watch::Receiver<bool>>,
) {
    let Some(receiver) = cancellation.as_mut() else {
        std::future::pending::<()>().await;
        return;
    };
    loop {
        if *receiver.borrow_and_update() {
            return;
        }
        if receiver.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

pub(crate) async fn emit_codex_activity_event(
    activity_events: &mut Vec<EventEnvelope>,
    dropped_activity_count: &mut usize,
    event: EventEnvelope,
    live_event_sink: &mut Option<&mut NodeLiveEventSink<'_>>,
) -> Result<(), ProviderStartFailure> {
    if let Some(sink) = live_event_sink.as_mut() {
        sink.emit(&event);
    }
    if activity_events.len() < MAX_PROVIDER_ACTIVITY_EVENTS {
        activity_events.push(event);
    } else {
        *dropped_activity_count = dropped_activity_count.saturating_add(1);
    }
    Ok(())
}

pub(crate) fn append_capped_process_line(
    output: &mut Vec<u8>,
    line: &str,
    max_bytes: usize,
    truncated: &mut bool,
) {
    if max_bytes == 0 {
        *truncated = true;
        return;
    }
    let remaining = max_bytes.saturating_sub(output.len());
    if remaining == 0 {
        *truncated = true;
        return;
    }
    let line_bytes = line.as_bytes();
    let copied = line_bytes.len().min(remaining);
    output.extend_from_slice(&line_bytes[..copied]);
    if copied < line_bytes.len() {
        *truncated = true;
        return;
    }
    if output.len() < max_bytes {
        output.push(b'\n');
    } else {
        *truncated = true;
    }
}

pub(crate) fn append_codex_process_limit_events(
    provider_key: &str,
    command: &CommandEnvelope,
    runtime_seqs: &mut HashMap<String, i64>,
    runtime_session_id: RuntimeSessionId,
    turn_id: Option<TurnId>,
    output: &CodexProcessOutput,
    events: &mut Vec<EventEnvelope>,
) {
    if !output.stdout_truncated && !output.stderr_truncated && output.dropped_activity_count == 0 {
        return;
    }
    events.push(provider_activity_event(
        provider_key,
        command,
        runtime_seqs,
        runtime_session_id,
        turn_id,
        serde_json::json!({
            "provider": provider_key,
            "source": "codex.exec.limits",
            "provider_event_type": "output_truncated",
            "phase": "warning",
            "status": "warning",
            "summary": "Codex provider output exceeded node retention limits",
            "stdout_truncated": output.stdout_truncated,
            "stderr_truncated": output.stderr_truncated,
            "dropped_activity_count": output.dropped_activity_count,
            "max_process_output_bytes": MAX_PROVIDER_PROCESS_OUTPUT_BYTES,
            "max_activity_events": MAX_PROVIDER_ACTIVITY_EVENTS,
        }),
    ));
}

pub(crate) fn codex_stdout_activity_event(
    provider_key: &str,
    command: &CommandEnvelope,
    runtime_seqs: &mut HashMap<String, i64>,
    runtime_session_id: RuntimeSessionId,
    turn_id: Option<TurnId>,
    line: &str,
) -> EventEnvelope {
    match serde_json::from_str::<serde_json::Value>(line) {
        Ok(value) => provider_activity_event(
            provider_key,
            command,
            runtime_seqs,
            runtime_session_id,
            turn_id,
            codex_activity_payload_from_json(provider_key, value),
        ),
        Err(error) => provider_activity_event(
            provider_key,
            command,
            runtime_seqs,
            runtime_session_id,
            turn_id,
            serde_json::json!({
                "provider": provider_key,
                "source": "codex.exec.jsonl",
                "provider_event_type": "parse_error",
                "phase": "error",
                "status": "error",
                "summary": format!("Codex JSONL parse error: {error}"),
                "raw_line_preview": bounded_text(line, MAX_PROVIDER_ACTIVITY_LINE_CHARS),
                "raw_line_truncated": line.chars().count() > MAX_PROVIDER_ACTIVITY_LINE_CHARS,
                "raw_line_original_chars": line.chars().count(),
                "parse_error": error.to_string(),
            }),
        ),
    }
}

pub(crate) fn codex_stderr_activity_event(
    provider_key: &str,
    command: &CommandEnvelope,
    runtime_seqs: &mut HashMap<String, i64>,
    runtime_session_id: RuntimeSessionId,
    turn_id: Option<TurnId>,
    line: &str,
) -> EventEnvelope {
    provider_activity_event(
        provider_key,
        command,
        runtime_seqs,
        runtime_session_id,
        turn_id,
        serde_json::json!({
            "provider": provider_key,
            "source": "codex.exec.stderr",
            "provider_event_type": "stderr",
            "phase": "warning",
            "status": "warning",
            "summary": bounded_text(line.trim(), MAX_PROVIDER_ACTIVITY_SUMMARY_CHARS),
            "raw_event": {
                "stream": "stderr",
                "line": bounded_text(line, MAX_PROVIDER_ACTIVITY_LINE_CHARS),
            },
            "raw_event_truncated": line.chars().count() > MAX_PROVIDER_ACTIVITY_LINE_CHARS,
            "raw_event_original_chars": line.chars().count(),
        }),
    )
}

pub(crate) fn provider_activity_event(
    provider_key: &str,
    command: &CommandEnvelope,
    runtime_seqs: &mut HashMap<String, i64>,
    runtime_session_id: RuntimeSessionId,
    turn_id: Option<TurnId>,
    payload: serde_json::Value,
) -> EventEnvelope {
    event_for_command(
        provider_key,
        command,
        runtime_seqs,
        runtime_session_id,
        turn_id,
        EventKind::ProviderActivity,
        payload,
    )
}

pub(crate) fn codex_activity_payload_from_json(
    provider_key: &str,
    value: serde_json::Value,
) -> serde_json::Value {
    let provider_event_type = top_level_json_string(&value, &["type", "event", "kind"])
        .unwrap_or_else(|| "unknown".to_owned());
    let provider_item = value.get("item");
    let provider_item_id = provider_item
        .and_then(|item| item.get("id"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| value.get("item_id").and_then(serde_json::Value::as_str))
        .map(|text| bounded_text(text, 512));
    let provider_item_type = provider_item
        .and_then(|item| item.get("type"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| value.get("item_type").and_then(serde_json::Value::as_str))
        .map(|text| bounded_text(text, 512));
    let status = first_json_string_for_keys(&value, &["status", "state"])
        .map(|text| bounded_text(text.trim(), MAX_PROVIDER_ACTIVITY_SUMMARY_CHARS));
    let phase = codex_activity_phase(&provider_event_type, status.as_deref());
    let summary = codex_activity_summary(&value).unwrap_or_else(|| provider_event_type.clone());

    let mut payload = serde_json::json!({
        "provider": provider_key,
        "source": "codex.exec.jsonl",
        "provider_event_type": provider_event_type,
        "phase": phase,
        "summary": summary,
    });
    if let Some(provider_item_id) = provider_item_id {
        payload["provider_item_id"] = serde_json::Value::String(provider_item_id);
    }
    if let Some(provider_item_type) = provider_item_type {
        payload["provider_item_type"] = serde_json::Value::String(provider_item_type);
    }
    if let Some(status) = status {
        payload["status"] = serde_json::Value::String(status);
    }
    append_bounded_raw_event(&mut payload, value);
    payload
}

pub(crate) fn codex_activity_phase(provider_event_type: &str, status: Option<&str>) -> String {
    let normalized_type = provider_event_type.to_ascii_lowercase();
    if normalized_type.contains("failed") || normalized_type.contains("error") {
        return "error".to_owned();
    }
    if normalized_type.contains("completed") || normalized_type.contains("done") {
        return "completed".to_owned();
    }
    if normalized_type.contains("started") || normalized_type.contains("created") {
        return "started".to_owned();
    }
    if normalized_type.contains("delta") || normalized_type.contains("output") {
        return "running".to_owned();
    }
    if let Some(status) = status {
        let normalized_status = status.to_ascii_lowercase();
        if normalized_status.contains("failed") || normalized_status.contains("error") {
            return "error".to_owned();
        }
        if normalized_status.contains("completed") || normalized_status.contains("done") {
            return "completed".to_owned();
        }
        if normalized_status.contains("running") || normalized_status.contains("started") {
            return "running".to_owned();
        }
    }
    "observed".to_owned()
}

pub(crate) fn codex_activity_summary(value: &serde_json::Value) -> Option<String> {
    first_json_string_for_keys(
        value,
        &[
            "command",
            "summary",
            "message",
            "text",
            "content",
            "delta",
            "reason",
            "description",
            "path",
        ],
    )
    .map(|text| bounded_text(text.trim(), MAX_PROVIDER_ACTIVITY_SUMMARY_CHARS))
    .filter(|text| !text.is_empty())
}

pub(crate) fn append_bounded_raw_event(
    payload: &mut serde_json::Value,
    raw_event: serde_json::Value,
) {
    let raw_event_chars = serde_json::to_string(&raw_event)
        .map(|text| text.chars().count())
        .unwrap_or(0);
    if raw_event_chars <= MAX_PROVIDER_ACTIVITY_RAW_CHARS {
        payload["raw_event"] = raw_event;
        payload["raw_event_truncated"] = serde_json::Value::Bool(false);
        return;
    }
    let preview = serde_json::to_string(&raw_event)
        .map(|text| bounded_text(&text, MAX_PROVIDER_ACTIVITY_RAW_CHARS))
        .unwrap_or_else(|_| "<unserializable provider event>".to_owned());
    payload["raw_event_truncated"] = serde_json::Value::Bool(true);
    payload["raw_event_original_chars"] =
        serde_json::Value::Number(serde_json::Number::from(raw_event_chars));
    payload["raw_event_preview"] = serde_json::Value::String(preview);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexApprovalRequest {
    pub(crate) approval_id: ApprovalId,
    pub(crate) prompt: String,
    pub(crate) provider_event_type: Option<String>,
}

pub(crate) fn codex_approval_requests_from_output(
    output: &CodexProcessOutput,
) -> Vec<CodexApprovalRequest> {
    output.approval_requests.clone()
}

pub(crate) fn codex_approval_request_from_json_line(line: &str) -> Option<CodexApprovalRequest> {
    let value = serde_json::from_str::<serde_json::Value>(line).ok()?;
    if !codex_json_is_approval_request(&value) {
        return None;
    }
    let provider_event_type = top_level_json_string(&value, &["type", "event", "kind"]);
    let approval_id = first_json_string_for_keys(&value, &["approval_id", "request_id", "id"])
        .filter(|value| !value.trim().is_empty())
        .map(ApprovalId::from)
        .unwrap_or_default();
    let prompt = first_json_string_for_keys(
        &value,
        &["prompt", "message", "question", "reason", "description"],
    )
    .map(|value| bounded_text(value.trim(), 1200))
    .filter(|value| !value.is_empty())
    .unwrap_or_else(|| "Codex requested approval".to_owned());

    Some(CodexApprovalRequest {
        approval_id,
        prompt,
        provider_event_type,
    })
}

pub(crate) fn codex_json_is_approval_request(value: &serde_json::Value) -> bool {
    let Some(event_name) = top_level_json_string(value, &["type", "event", "kind"]) else {
        return first_json_string_for_keys(value, &["approval_id"]).is_some()
            && first_json_string_for_keys(value, &["prompt", "message", "question"]).is_some();
    };
    let normalized = event_name.to_ascii_lowercase();
    (normalized.contains("approval")
        && (normalized.contains("request") || normalized.contains("requested")))
        || (normalized.contains("user")
            && normalized.contains("input")
            && normalized.contains("request"))
}

pub(crate) fn top_level_json_string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    let object = value.as_object()?;
    keys.iter()
        .find_map(|key| object.get(*key).and_then(serde_json::Value::as_str))
        .map(str::to_owned)
}

pub(crate) fn first_json_string_for_keys(
    value: &serde_json::Value,
    keys: &[&str],
) -> Option<String> {
    match value {
        serde_json::Value::Object(object) => {
            for key in keys {
                if let Some(text) = object.get(*key).and_then(serde_json::Value::as_str) {
                    return Some(text.to_owned());
                }
            }
            object
                .values()
                .find_map(|nested| first_json_string_for_keys(nested, keys))
        }
        serde_json::Value::Array(values) => values
            .iter()
            .find_map(|nested| first_json_string_for_keys(nested, keys)),
        _ => None,
    }
}

pub(crate) fn codex_resume_ref_from_output(
    output: &CodexProcessOutput,
) -> Option<serde_json::Value> {
    output.provider_resume_ref.clone()
}

pub(crate) fn codex_resume_ref_from_json_line(line: &str) -> Option<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(codex_resume_ref_from_json)
}

pub(crate) fn codex_resume_ref_from_json(value: serde_json::Value) -> Option<serde_json::Value> {
    let mut resume_ref = serde_json::Map::new();
    if let Some(session_id) = first_json_string_for_keys(
        &value,
        &[
            "provider_session_id",
            "session_id",
            "conversation_id",
            "thread_id",
        ],
    )
    .filter(|value| !value.trim().is_empty())
    {
        resume_ref.insert(
            "provider_session_id".to_owned(),
            serde_json::Value::String(bounded_text(session_id.trim(), 512)),
        );
    }
    if let Some(cursor) = first_json_string_for_keys(&value, &["resume_cursor", "cursor"])
        .filter(|value| !value.trim().is_empty())
    {
        resume_ref.insert(
            "resume_cursor".to_owned(),
            serde_json::Value::String(bounded_text(cursor.trim(), 512)),
        );
    }
    if resume_ref.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(resume_ref))
    }
}

pub(crate) fn codex_runtime_ready_payload(
    provider_key: &str,
    provider_resume_ref: Option<serde_json::Value>,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "provider": provider_key,
        "mode": "exec",
    });
    if let Some(provider_resume_ref) = provider_resume_ref {
        payload["provider_resume_ref"] = provider_resume_ref;
    }
    payload
}

pub(crate) fn codex_exec_prompt(content: &str, transcript: &[ProviderTranscriptMessage]) -> String {
    let mut selected = Vec::new();
    let mut context_chars = 0usize;
    for message in transcript.iter().rev() {
        let message_chars = message.role.chars().count() + message.content.chars().count() + 4;
        if !selected.is_empty()
            && context_chars + message_chars + content.chars().count() > MAX_CODEX_TRANSCRIPT_CHARS
        {
            break;
        }
        selected.push(message);
        context_chars += message_chars;
        if selected.len() >= MAX_CODEX_TRANSCRIPT_MESSAGES {
            break;
        }
    }
    selected.reverse();

    if selected.is_empty() {
        return content.to_owned();
    }

    let mut prompt = String::from(
        "Continue this Uprava session. Use the transcript only as prior context, then answer the latest user message.\n\nTranscript:\n",
    );
    for message in selected {
        prompt.push_str(&message.role);
        prompt.push_str(": ");
        prompt.push_str(&message.content);
        prompt.push('\n');
    }
    prompt.push_str("\nLatest user message:\n");
    prompt.push_str(content);
    prompt
}

pub(crate) fn record_codex_transcript_turn(
    runtime_transcripts: &mut HashMap<String, Vec<ProviderTranscriptMessage>>,
    runtime_session_id: &RuntimeSessionId,
    user_content: &str,
    assistant_content: &str,
) {
    let transcript = runtime_transcripts
        .entry(runtime_session_id.to_string())
        .or_default();
    transcript.push(ProviderTranscriptMessage {
        role: "user".to_owned(),
        content: user_content.to_owned(),
    });
    transcript.push(ProviderTranscriptMessage {
        role: "assistant".to_owned(),
        content: assistant_content.to_owned(),
    });
    trim_codex_transcript(transcript);
}

pub(crate) fn trim_codex_transcript(transcript: &mut Vec<ProviderTranscriptMessage>) {
    if transcript.len() > MAX_CODEX_TRANSCRIPT_MESSAGES {
        let overflow = transcript.len() - MAX_CODEX_TRANSCRIPT_MESSAGES;
        transcript.drain(0..overflow);
    }
    while transcript.len() > 2
        && transcript
            .iter()
            .map(|message| message.role.chars().count() + message.content.chars().count())
            .sum::<usize>()
            > MAX_CODEX_TRANSCRIPT_CHARS
    {
        transcript.remove(0);
    }
}

#[derive(Debug, Clone)]
pub(crate) struct UnsupportedProviderAdapter {
    pub(crate) provider_key: String,
}

pub(crate) fn missing_provider_events_for_command(
    command: &CommandEnvelope,
    runtime_seqs: &mut HashMap<String, i64>,
) -> Vec<EventEnvelope> {
    let Some(runtime_session_id) = command.target.runtime_session_id().cloned() else {
        return vec![];
    };
    vec![event_for_command(
        "unknown",
        command,
        runtime_seqs,
        runtime_session_id,
        None,
        EventKind::RuntimeError,
        serde_json::json!({
            "code": "provider.missing",
            "message": "Runtime command is missing provider metadata",
        }),
    )]
}

impl UnsupportedProviderAdapter {
    pub(crate) fn events_for_command(
        &self,
        command: &CommandEnvelope,
        runtime_seqs: &mut HashMap<String, i64>,
    ) -> Vec<EventEnvelope> {
        let Some(runtime_session_id) = command.target.runtime_session_id().cloned() else {
            return vec![];
        };
        vec![event_for_command(
            &self.provider_key,
            command,
            runtime_seqs,
            runtime_session_id,
            None,
            EventKind::RuntimeError,
            serde_json::json!({
                "code": "provider.unsupported",
                "message": format!("Provider `{}` is not supported by this node", self.provider_key),
            }),
        )]
    }
}

pub(crate) fn runtime_error_event(
    provider_key: &str,
    command: &CommandEnvelope,
    runtime_seqs: &mut HashMap<String, i64>,
    runtime_session_id: RuntimeSessionId,
    turn_id: Option<TurnId>,
    code: &str,
    message: impl Into<String>,
) -> EventEnvelope {
    event_for_command(
        provider_key,
        command,
        runtime_seqs,
        runtime_session_id,
        turn_id,
        EventKind::RuntimeError,
        serde_json::json!({
            "code": code,
            "message": message.into(),
        }),
    )
}

pub(crate) fn runtime_workspace_error_events(
    provider_key: &str,
    command: &CommandEnvelope,
    runtime_seqs: &mut HashMap<String, i64>,
    error: WorkspaceInspectError,
) -> Vec<EventEnvelope> {
    let Some(runtime_session_id) = command.target.runtime_session_id().cloned() else {
        return vec![];
    };
    vec![runtime_error_event(
        provider_key,
        command,
        runtime_seqs,
        runtime_session_id,
        None,
        error.code,
        error.message,
    )]
}

pub(crate) fn codex_last_message_path(command_id: &CommandId) -> PathBuf {
    std::env::temp_dir().join(format!(
        "uprava-codex-{}-{}.txt",
        sanitize_filename_segment(command_id.as_str()),
        Uuid::new_v4()
    ))
}

pub(crate) fn codex_deduction_schema_path(command_id: &CommandId) -> PathBuf {
    std::env::temp_dir().join(format!(
        "uprava-deduction-schema-{}-{}.json",
        sanitize_filename_segment(command_id.as_str()),
        Uuid::new_v4()
    ))
}

pub(crate) fn deduction_output_schema(package: &DeductionInputPackage) -> serde_json::Value {
    let allowed_refs = package
        .allowed_refs
        .iter()
        .filter_map(|reference| serde_json::to_value(reference).ok())
        .collect::<Vec<_>>();
    serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": false,
        "required": [
            "title", "conclusion", "certainty", "steps", "assumptions", "unknowns", "alternatives"
        ],
        "properties": {
            "title": { "type": "string", "minLength": 1, "maxLength": 240 },
            "conclusion": { "type": "string", "minLength": 1, "maxLength": 4000 },
            "certainty": { "type": "string", "enum": ["high", "medium", "low", "unknown"] },
            "steps": {
                "type": "array",
                "minItems": 1,
                "maxItems": 100,
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["step_id", "classification", "summary", "support_refs"],
                    "properties": {
                        "step_id": { "type": "string", "minLength": 1, "maxLength": 120 },
                        "classification": {
                            "type": "string",
                            "enum": ["observed", "inference", "assumption", "unknown", "alternative"]
                        },
                        "summary": { "type": "string", "minLength": 1, "maxLength": 1600 },
                        "support_refs": {
                            "type": "array",
                            "maxItems": 100,
                            "items": { "enum": allowed_refs }
                        }
                    }
                }
            },
            "assumptions": {
                "type": "array",
                "maxItems": 100,
                "items": { "type": "string", "maxLength": 1000 }
            },
            "unknowns": {
                "type": "array",
                "maxItems": 100,
                "items": { "type": "string", "maxLength": 1000 }
            },
            "alternatives": {
                "type": "array",
                "maxItems": 100,
                "items": { "type": "string", "maxLength": 1000 }
            }
        }
    })
}

pub(crate) fn deduction_prompt(package: &DeductionInputPackage) -> String {
    let package_json = serde_json::to_string_pretty(package).unwrap_or_else(|_| "{}".to_owned());
    format!(
        "You are Uprava's isolated causality analyst. Analyze only the bounded evidence package below.\n\
         Do not edit files, run commands, continue the interactive agent session, or invent references.\n\
         Classify direct facts as observed, derived claims as inference, unsupported premises as assumption,\n\
         missing information as unknown, and competing explanations as alternative.\n\
         Every observed step must cite one or more exact support_refs copied from allowed_refs.\n\
         Never cite a reference outside allowed_refs. Return only JSON matching the supplied schema.\n\n\
         EVIDENCE PACKAGE:\n{package_json}"
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "deduction provider envelope keeps output, fallback, error, and command state explicit"
)]
pub(crate) fn deduction_output_payload(
    package: &DeductionInputPackage,
    provider: &str,
    result: Option<DeductionProviderResult>,
    raw_text: String,
    raw_truncated: bool,
    error_code: Option<String>,
    error_message: Option<String>,
    status: CommandState,
) -> (CommandState, JsonValue) {
    let output = DeductionProviderOutput {
        deduction_id: package.deduction_id.clone(),
        provider: provider.to_owned(),
        model: None,
        schema_version: DEDUCTION_SCHEMA_VERSION.to_owned(),
        evidence_snapshot_hash: package.evidence_snapshot_hash.clone(),
        result,
        raw_text,
        raw_truncated,
        error_code,
        error_message,
    };
    match serde_json::to_value(output) {
        Ok(value) => (status, JsonValue(value)),
        Err(error) => (
            CommandState::Failed,
            JsonValue(serde_json::json!({
                "error_code": "deduction.output_serialization_failed",
                "message": error.to_string(),
            })),
        ),
    }
}

pub(crate) fn deduction_error_payload(
    command: &CommandEnvelope,
    provider: &str,
    code: impl Into<String>,
    message: impl Into<String>,
) -> (CommandState, JsonValue) {
    if let CommandPayload::RequestDeduction { package } = &command.payload {
        return deduction_output_payload(
            package,
            provider,
            None,
            String::new(),
            false,
            Some(code.into()),
            Some(message.into()),
            CommandState::Failed,
        );
    }
    (
        CommandState::Failed,
        JsonValue(serde_json::json!({
            "error_code": code.into(),
            "message": message.into(),
        })),
    )
}

pub(crate) fn sanitize_filename_segment(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

pub(crate) fn codex_failure_message(output: &CodexProcessOutput) -> String {
    let status = output
        .status
        .code()
        .map(|code| code.to_string())
        .unwrap_or_else(|| "terminated_by_signal".to_owned());
    let stderr = bounded_text(&String::from_utf8_lossy(&output.stderr), 1200);
    if !stderr.trim().is_empty() {
        return format!("Codex exec failed with status {status}: {}", stderr.trim());
    }
    let stdout = bounded_text(&String::from_utf8_lossy(&output.stdout), 1200);
    if !stdout.trim().is_empty() {
        return format!("Codex exec failed with status {status}: {}", stdout.trim());
    }
    format!("Codex exec failed with status {status}")
}

pub(crate) fn bounded_text(value: &str, max_chars: usize) -> String {
    let mut text = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        text.push_str("...");
    }
    text
}

pub(crate) fn event_for_command(
    provider_key: &str,
    command: &CommandEnvelope,
    runtime_seqs: &mut HashMap<String, i64>,
    runtime_session_id: RuntimeSessionId,
    turn_id: Option<TurnId>,
    kind: EventKind,
    payload: serde_json::Value,
) -> EventEnvelope {
    let seq = next_runtime_seq(runtime_seqs, &runtime_session_id);
    EventEnvelope {
        event_id: EventId::new(),
        command_id: Some(command.command_id.clone()),
        correlation_id: Some(command.correlation_id.clone()),
        actor_ref: ActorRef::Provider {
            provider: provider_key.to_owned(),
        },
        scope_ref: ScopeRef::Runtime {
            runtime_session_id: runtime_session_id.clone(),
        },
        node_id: Some(command.target.node_id().clone()),
        runtime_session_id: Some(runtime_session_id),
        session_thread_id: command.target.session_thread_id().cloned(),
        turn_id,
        seq,
        session_projection_seq: None,
        kind,
        happened_at: Utc::now(),
        source_refs: vec![],
        evidence_refs: vec![],
        cause_refs: command.cause_refs.clone(),
        result_refs: vec![],
        payload: EventPayload::from_json(kind, payload),
    }
}

pub(crate) fn next_runtime_seq(
    runtime_seqs: &mut HashMap<String, i64>,
    runtime_session_id: &RuntimeSessionId,
) -> i64 {
    let entry = runtime_seqs
        .entry(runtime_session_id.to_string())
        .and_modify(|seq| *seq += 1)
        .or_insert(1);
    *entry
}
