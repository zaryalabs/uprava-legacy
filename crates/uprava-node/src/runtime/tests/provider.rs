use super::*;

#[tokio::test]
async fn codex_start_runtime_records_provider_and_workspace_metadata() {
    let config = config_fixture();
    let workspace_path_buf = std::env::temp_dir();
    let workspace_path = workspace_path_buf.display().to_string();
    let canonical_workspace_path = std::fs::canonicalize(&workspace_path_buf)
        .expect("temp dir canonicalizes")
        .display()
        .to_string();
    let mut command = command_fixture("command-codex-start", CommandKind::StartRuntime);
    command.payload = CommandPayload::StartRuntime {
        provider: "codex".to_owned(),
        workspace_path: workspace_path.clone(),
        execution_profile: AgentExecutionProfile::ExecCompatibility,
        effective_policy: None,
        effective_policy_hash: None,
    };
    let mut local_state = NodeLocalState::default();

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;

    assert_eq!(
        event_kinds(&outcome.events_to_send),
        vec![EventKind::RuntimeStarting, EventKind::RuntimeReady]
    );
    assert_eq!(
        local_state
            .runtime_providers
            .get("runtime-1")
            .map(String::as_str),
        Some("codex")
    );
    assert_eq!(
        local_state
            .runtime_workspace_paths
            .get("runtime-1")
            .map(String::as_str),
        Some(canonical_workspace_path.as_str())
    );
    assert_eq!(
        local_state
            .runtime_transcripts
            .get("runtime-1")
            .map(Vec::len),
        Some(0)
    );
}

#[cfg(unix)]
#[tokio::test]
async fn codex_deduction_is_ephemeral_read_only_and_returns_structured_output() {
    let capture_path =
        std::env::temp_dir().join(format!("uprava-deduction-args-{}", Uuid::new_v4()));
    let codex_binary = fake_codex_deduction_binary(&capture_path);
    let config = config_fixture_with_codex_binary(codex_binary.display().to_string());
    let scope_ref = UpravaRef::Session {
        session_thread_id: SessionThreadId::from("session-1"),
    };
    let package = DeductionInputPackage {
        deduction_id: uprava_protocol::DeductionId::from("deduction-1"),
        session_thread_id: SessionThreadId::from("session-1"),
        scope_ref: scope_ref.clone(),
        question: "What caused the result?".to_owned(),
        evidence_snapshot_hash: "snapshot-hash-1".to_owned(),
        trace_steps: vec![],
        events: vec![],
        allowed_refs: vec![scope_ref],
        truncated: false,
        generated_at: Utc::now(),
    };
    let mut command = command_fixture("command-deduction", CommandKind::RequestDeduction);
    command.payload = CommandPayload::RequestDeduction {
        package: Box::new(package),
    };
    let mut local_state = NodeLocalState::default();
    local_state
        .runtime_providers
        .insert("runtime-1".to_owned(), "codex".to_owned());
    local_state.runtime_workspace_paths.insert(
        "runtime-1".to_owned(),
        std::env::temp_dir().display().to_string(),
    );

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;
    let output = serde_json::from_value::<DeductionProviderOutput>(outcome.result_payload.0)
        .expect("deduction output decodes");
    let args = std::fs::read_to_string(&capture_path).expect("deduction args captured");

    std::fs::remove_file(codex_binary).expect("codex fixture removes");
    std::fs::remove_file(capture_path).expect("capture fixture removes");
    assert_eq!(outcome.status, CommandState::Completed);
    assert!(outcome.events_to_send.is_empty());
    assert_eq!(output.deduction_id.as_str(), "deduction-1");
    assert_eq!(output.schema_version, DEDUCTION_SCHEMA_VERSION);
    assert!(output.result.is_some());
    assert!(args.lines().any(|arg| arg == "--ephemeral"));
    assert!(args.lines().any(|arg| arg == "read-only"));
    assert!(args.lines().any(|arg| arg == "--output-schema"));
    assert!(!args.contains("dangerously-bypass-approvals-and-sandbox"));
    assert!(!args.lines().any(|arg| arg == "resume"));
    assert!(local_state.runtime_transcripts.is_empty());
    assert!(local_state.runtime_provider_resume_refs.is_empty());
}

#[tokio::test]
async fn cancelled_deduction_tombstone_prevents_late_provider_start() {
    let config = config_fixture_with_codex_binary("missing-codex-that-must-not-start");
    let mut cancel = command_fixture("command-cancel-deduction", CommandKind::CancelDeduction);
    cancel.payload = CommandPayload::CancelDeduction {
        deduction_id: uprava_protocol::DeductionId::from("deduction-cancelled"),
    };
    let scope_ref = UpravaRef::Session {
        session_thread_id: SessionThreadId::from("session-1"),
    };
    let mut request = command_fixture("command-late-deduction", CommandKind::RequestDeduction);
    request.payload = CommandPayload::RequestDeduction {
        package: Box::new(DeductionInputPackage {
            deduction_id: uprava_protocol::DeductionId::from("deduction-cancelled"),
            session_thread_id: SessionThreadId::from("session-1"),
            scope_ref: scope_ref.clone(),
            question: "Why?".to_owned(),
            evidence_snapshot_hash: "snapshot-hash".to_owned(),
            trace_steps: vec![],
            events: vec![],
            allowed_refs: vec![scope_ref],
            truncated: false,
            generated_at: Utc::now(),
        }),
    };
    let mut local_state = NodeLocalState::default();
    local_state
        .runtime_providers
        .insert("runtime-1".to_owned(), "codex".to_owned());
    local_state.runtime_workspace_paths.insert(
        "runtime-1".to_owned(),
        std::env::temp_dir().display().to_string(),
    );

    let cancelled = prepare_command_dispatch(&config, &mut local_state, &cancel).await;
    let late = prepare_command_dispatch(&config, &mut local_state, &request).await;
    let output = serde_json::from_value::<DeductionProviderOutput>(late.result_payload.0)
        .expect("cancelled provider output decodes");

    assert_eq!(cancelled.status, CommandState::Completed);
    assert_eq!(late.status, CommandState::Failed);
    assert_eq!(output.error_code.as_deref(), Some("deduction.cancelled"));
    assert!(!local_state
        .cancelled_deductions
        .contains("deduction-cancelled"));
}

#[tokio::test]
async fn codex_send_turn_maps_missing_binary_to_missing_binary_error() {
    let missing_binary = std::env::temp_dir()
        .join(format!("missing-codex-{}", Uuid::new_v4()))
        .display()
        .to_string();
    let config = config_fixture_with_codex_binary(missing_binary);
    let command = command_fixture("command-codex-send", CommandKind::SendTurn);
    let mut local_state = NodeLocalState::default();
    local_state
        .runtime_providers
        .insert("runtime-1".to_owned(), "codex".to_owned());
    local_state.runtime_workspace_paths.insert(
        "runtime-1".to_owned(),
        std::env::temp_dir().display().to_string(),
    );

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;

    assert_eq!(outcome.status, CommandState::Failed);
    assert_eq!(
        event_kinds(&outcome.events_to_send),
        vec![
            EventKind::RuntimeRunning,
            EventKind::TurnStarted,
            EventKind::RuntimeError
        ]
    );
    assert_eq!(
        outcome.events_to_send[2]
            .payload
            .0
            .get("code")
            .and_then(serde_json::Value::as_str),
        Some("provider.missing_binary")
    );
    assert_eq!(
        local_state
            .command_status
            .get("command-codex-send")
            .copied(),
        Some(CommandState::Failed)
    );
}

#[tokio::test]
async fn codex_send_turn_maps_missing_workspace_to_workspace_missing_error() {
    let config = config_fixture();
    let command = command_fixture("command-codex-workspace-missing", CommandKind::SendTurn);
    let mut local_state = NodeLocalState::default();
    local_state
        .runtime_providers
        .insert("runtime-1".to_owned(), "codex".to_owned());

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;

    assert_eq!(outcome.status, CommandState::Failed);
    assert_eq!(
        event_kinds(&outcome.events_to_send),
        vec![
            EventKind::RuntimeRunning,
            EventKind::TurnStarted,
            EventKind::RuntimeError
        ]
    );
    assert_eq!(
        outcome.events_to_send[2]
            .payload
            .0
            .get("code")
            .and_then(serde_json::Value::as_str),
        Some("provider.workspace_missing")
    );
    assert_eq!(
        local_state.runtime_states.get("runtime-1").copied(),
        Some(RuntimeSessionState::Error)
    );
}

#[cfg(unix)]
#[tokio::test]
async fn codex_send_turn_executes_binary_and_emits_completed_assistant_message() {
    let codex_binary = fake_codex_success_binary();
    let workspace_path =
        std::env::temp_dir().join(format!("uprava-codex-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace fixture creates");
    let config = config_fixture_with_codex_binary(codex_binary.display().to_string());
    let command = command_fixture_with_content(
        "command-codex-success",
        CommandKind::SendTurn,
        "build status",
    );
    let mut local_state = NodeLocalState::default();
    local_state
        .runtime_providers
        .insert("runtime-1".to_owned(), "codex".to_owned());
    local_state
        .runtime_workspace_paths
        .insert("runtime-1".to_owned(), workspace_path.display().to_string());

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;

    std::fs::remove_file(codex_binary).expect("codex fixture removes");
    std::fs::remove_dir_all(workspace_path).expect("workspace fixture removes");
    assert_eq!(
        outcome.status,
        CommandState::Completed,
        "Codex success fixture returned unexpected events: {:#?}",
        outcome.events_to_send
    );
    assert_eq!(
        event_kinds(&outcome.events_to_send),
        vec![
            EventKind::RuntimeRunning,
            EventKind::TurnStarted,
            EventKind::ProviderActivity,
            EventKind::ProviderMessageCompleted,
            EventKind::TurnCompleted,
            EventKind::RuntimeReady,
        ]
    );
    assert_eq!(
        outcome.events_to_send[3]
            .payload
            .0
            .get("content")
            .and_then(serde_json::Value::as_str),
        Some("Codex fake accepted")
    );
    assert_eq!(
        outcome.events_to_send[2]
            .payload
            .0
            .get("provider_event_type")
            .and_then(serde_json::Value::as_str),
        Some("response.completed")
    );
    assert_eq!(
        outcome.events_to_send[2]
            .payload
            .0
            .get("raw_event")
            .and_then(|value| value.get("session_id"))
            .and_then(serde_json::Value::as_str),
        Some("codex-session-1")
    );
    assert_eq!(
        local_state.runtime_states.get("runtime-1").copied(),
        Some(RuntimeSessionState::Ready)
    );
    assert_eq!(
        local_state
            .command_status
            .get("command-codex-success")
            .copied(),
        Some(CommandState::Completed)
    );
    assert_eq!(
        local_state
            .runtime_transcripts
            .get("runtime-1")
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(
        local_state
            .runtime_provider_resume_refs
            .get("runtime-1")
            .and_then(|resume_ref| resume_ref.provider_session_id.as_deref()),
        Some("codex-session-1")
    );
    assert_eq!(
        outcome.events_to_send[5]
            .payload
            .0
            .get("provider_resume_ref")
            .and_then(|value| value.get("provider_session_id"))
            .and_then(serde_json::Value::as_str),
        Some("codex-session-1")
    );
    assert_eq!(
        outcome.events_to_send[5]
            .payload
            .0
            .get("provider_resume_ref")
            .and_then(|value| value.get("resume_cursor"))
            .and_then(serde_json::Value::as_str),
        Some("cursor-1")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn codex_send_turn_includes_required_noninteractive_flags() {
    let capture_path = std::env::temp_dir().join(format!("uprava-codex-args-{}", Uuid::new_v4()));
    let codex_binary = fake_codex_args_capture_binary(&capture_path);
    let workspace_path =
        std::env::temp_dir().join(format!("uprava-codex-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace fixture creates");
    let config = config_fixture_with_codex_binary(codex_binary.display().to_string());
    let command =
        command_fixture_with_content("command-codex-args", CommandKind::SendTurn, "status");
    let mut local_state = NodeLocalState::default();
    local_state
        .runtime_providers
        .insert("runtime-1".to_owned(), "codex".to_owned());
    local_state
        .runtime_workspace_paths
        .insert("runtime-1".to_owned(), workspace_path.display().to_string());

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;

    let captured_args = std::fs::read_to_string(&capture_path).expect("captured args read");
    std::fs::remove_file(codex_binary).expect("codex fixture removes");
    std::fs::remove_file(capture_path).expect("args capture removes");
    std::fs::remove_dir_all(workspace_path).expect("workspace fixture removes");
    assert_eq!(outcome.status, CommandState::Completed);
    assert_codex_launch_flags(&captured_args);
}

#[test]
fn codex_mcp_delivery_keeps_lease_out_of_process_arguments_and_debug_output() {
    let access = ProviderMcpAccess {
        endpoint_url: "http://127.0.0.1:8080/mcp".to_owned(),
        access_token: uprava_protocol::McpAccessToken::new("lease-secret-value"),
        expires_at: Utc::now() + chrono::Duration::minutes(10),
    };
    let mut command = TokioCommand::new("codex");

    configure_uprava_mcp(&mut command, Some(&access)).expect("MCP config applies");

    let args = command
        .as_std()
        .get_args()
        .map(|value| value.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    let token_env = command
        .as_std()
        .get_envs()
        .find(|(name, _)| {
            name.to_string_lossy()
                .starts_with(UPRAVA_MCP_TOKEN_ENV_PREFIX)
        })
        .and_then(|(_, value)| value)
        .map(|value| value.to_string_lossy().into_owned());

    assert!(args.contains("mcp_servers.uprava.url="));
    assert!(args.contains("mcp_servers.uprava.bearer_token_env_var="));
    assert!(args.contains("mcp_servers.uprava.default_tools_approval_mode=\"approve\""));
    assert!(args.contains("shell_snapshot"));
    assert!(args.contains("shell_environment_policy.inherit=all"));
    assert!(!args.contains("lease-secret-value"));
    assert_eq!(token_env.as_deref(), Some("lease-secret-value"));
    assert!(!format!("{access:?}").contains("lease-secret-value"));
}

#[test]
fn provider_environment_keeps_only_allowlisted_inherited_values() {
    let _lock = env_lock();
    let _env = EnvGuard::cleared(&[
        "OPENAI_PROJECT",
        "SHELL",
        "CODEX_SHELL",
        "UPRAVA_SECRET_SENTINEL",
    ]);
    std::env::set_var("OPENAI_PROJECT", "allowed-project");
    std::env::set_var("SHELL", "/bin/zsh");
    std::env::set_var("CODEX_SHELL", "/bin/zsh");
    std::env::set_var("UPRAVA_SECRET_SENTINEL", "must-not-leak");
    let mut command = TokioCommand::new("codex");

    configure_provider_environment(&mut command);

    let environment = command
        .as_std()
        .get_envs()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.map(|value| value.to_string_lossy().into_owned()),
            )
        })
        .collect::<HashMap<_, _>>();
    assert_eq!(
        environment.get("OPENAI_PROJECT"),
        Some(&Some("allowed-project".to_owned()))
    );
    assert_eq!(environment.get("SHELL"), Some(&Some("/bin/zsh".to_owned())));
    assert_eq!(
        environment.get("CODEX_SHELL"),
        Some(&Some("/bin/zsh".to_owned()))
    );
    assert!(!environment.contains_key("UPRAVA_SECRET_SENTINEL"));
}

#[test]
fn provider_mcp_access_failure_stops_turn_before_codex_launch() {
    let command = command_fixture("command-mcp-unavailable", CommandKind::SendTurn);
    let mut local_state = NodeLocalState::default();
    local_state
        .runtime_providers
        .insert("runtime-1".to_owned(), "codex".to_owned());

    let outcome = provider_mcp_access_failure_outcome(&mut local_state, &command);

    assert_eq!(outcome.status, CommandState::Failed);
    assert_eq!(outcome.events_to_send.len(), 1);
    assert_eq!(outcome.events_to_send[0].kind, EventKind::RuntimeError);
    assert_eq!(
        outcome.events_to_send[0]
            .payload
            .0
            .get("code")
            .and_then(serde_json::Value::as_str),
        Some("provider.mcp_access_unavailable")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn codex_send_turn_includes_prior_transcript_context() {
    let capture_path = std::env::temp_dir().join(format!("uprava-codex-prompt-{}", Uuid::new_v4()));
    let codex_binary = fake_codex_prompt_capture_binary(&capture_path);
    let workspace_path =
        std::env::temp_dir().join(format!("uprava-codex-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace fixture creates");
    let config = config_fixture_with_codex_binary(codex_binary.display().to_string());
    let command = command_fixture_with_content(
        "command-codex-transcript",
        CommandKind::SendTurn,
        "second question",
    );
    let mut local_state = NodeLocalState::default();
    local_state
        .runtime_providers
        .insert("runtime-1".to_owned(), "codex".to_owned());
    local_state
        .runtime_workspace_paths
        .insert("runtime-1".to_owned(), workspace_path.display().to_string());
    local_state.runtime_transcripts.insert(
        "runtime-1".to_owned(),
        vec![
            ProviderTranscriptMessage {
                role: "user".to_owned(),
                content: "first question".to_owned(),
            },
            ProviderTranscriptMessage {
                role: "assistant".to_owned(),
                content: "first answer".to_owned(),
            },
        ],
    );

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;

    let captured_prompt = std::fs::read_to_string(&capture_path).expect("captured prompt reads");
    std::fs::remove_file(codex_binary).expect("codex fixture removes");
    std::fs::remove_file(capture_path).expect("prompt capture removes");
    std::fs::remove_dir_all(workspace_path).expect("workspace fixture removes");
    assert_eq!(outcome.status, CommandState::Completed);
    assert!(captured_prompt.contains("Continue this Uprava session"));
    assert!(captured_prompt.contains("user: first question"));
    assert!(captured_prompt.contains("assistant: first answer"));
    assert!(captured_prompt.contains("Latest user message:\nsecond question"));
    let transcript = local_state
        .runtime_transcripts
        .get("runtime-1")
        .expect("runtime transcript exists");
    assert_eq!(transcript.len(), 4);
    assert_eq!(transcript[2].content, "second question");
    assert_eq!(transcript[3].content, "Codex contextual answer");
}

#[cfg(unix)]
#[tokio::test]
async fn codex_send_turn_uses_provider_native_resume_when_session_id_exists() {
    let capture_path =
        std::env::temp_dir().join(format!("uprava-codex-resume-args-{}", Uuid::new_v4()));
    let codex_binary = fake_codex_resume_capture_binary(&capture_path);
    let workspace_path =
        std::env::temp_dir().join(format!("uprava-codex-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace fixture creates");
    let config = config_fixture_with_codex_binary(codex_binary.display().to_string());
    let command = command_fixture_with_content(
        "command-codex-native-resume",
        CommandKind::SendTurn,
        "third question",
    );
    let mut local_state = NodeLocalState::default();
    local_state
        .runtime_providers
        .insert("runtime-1".to_owned(), "codex".to_owned());
    local_state
        .runtime_workspace_paths
        .insert("runtime-1".to_owned(), workspace_path.display().to_string());
    local_state.runtime_provider_resume_refs.insert(
        "runtime-1".to_owned(),
        ProviderResumeRef {
            provider_session_id: Some("codex-session-1".to_owned()),
            resume_cursor: Some("cursor-1".to_owned()),
        },
    );

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;

    let captured_args = std::fs::read_to_string(&capture_path).expect("captured args read");
    std::fs::remove_file(codex_binary).expect("codex fixture removes");
    std::fs::remove_file(capture_path).expect("resume capture removes");
    std::fs::remove_dir_all(workspace_path).expect("workspace fixture removes");
    assert_eq!(outcome.status, CommandState::Completed);
    assert!(captured_args.contains("exec\nresume\n"));
    assert_codex_launch_flags(&captured_args);
    assert!(captured_args.contains("\ncodex-session-1\n"));
    assert!(captured_args.contains("\nthird question\n"));
    assert!(!captured_args.contains("Latest user message:"));
    assert_eq!(
        local_state
            .runtime_provider_resume_refs
            .get("runtime-1")
            .and_then(|resume_ref| resume_ref.resume_cursor.as_deref()),
        Some("cursor-2")
    );
    assert_eq!(
        outcome.events_to_send[3]
            .payload
            .0
            .get("content")
            .and_then(serde_json::Value::as_str),
        Some("Codex resume accepted")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn codex_send_turn_maps_stdout_approval_request_to_blocked_runtime() {
    let codex_binary = fake_codex_approval_request_binary();
    let workspace_path =
        std::env::temp_dir().join(format!("uprava-codex-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace fixture creates");
    let config = config_fixture_with_codex_binary(codex_binary.display().to_string());
    let command = command_fixture_with_content(
        "command-codex-approval",
        CommandKind::SendTurn,
        "change files",
    );
    let mut local_state = NodeLocalState::default();
    local_state
        .runtime_providers
        .insert("runtime-1".to_owned(), "codex".to_owned());
    local_state
        .runtime_workspace_paths
        .insert("runtime-1".to_owned(), workspace_path.display().to_string());

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;

    std::fs::remove_file(codex_binary).expect("codex fixture removes");
    std::fs::remove_dir_all(workspace_path).expect("workspace fixture removes");
    assert_eq!(outcome.status, CommandState::Completed);
    assert_eq!(
        event_kinds(&outcome.events_to_send),
        vec![
            EventKind::RuntimeRunning,
            EventKind::TurnStarted,
            EventKind::ProviderActivity,
            EventKind::ApprovalRequested,
            EventKind::RuntimeBlocked,
        ]
    );
    assert_eq!(
        outcome.events_to_send[3]
            .payload
            .0
            .get("approval_id")
            .and_then(serde_json::Value::as_str),
        Some("approval-codex-1")
    );
    assert_eq!(
        outcome.events_to_send[3]
            .payload
            .0
            .get("prompt")
            .and_then(serde_json::Value::as_str),
        Some("Allow file edit?")
    );
    assert_eq!(
        outcome.events_to_send[3]
            .payload
            .0
            .get("source")
            .and_then(serde_json::Value::as_str),
        Some("codex.exec.jsonl")
    );
    assert_eq!(
        outcome.events_to_send[2]
            .payload
            .0
            .get("raw_event")
            .and_then(|value| value.get("approval_id"))
            .and_then(serde_json::Value::as_str),
        Some("approval-codex-1")
    );
    assert_eq!(
        local_state.runtime_states.get("runtime-1").copied(),
        Some(RuntimeSessionState::Blocked)
    );
    assert!(local_state
        .runtime_transcripts
        .get("runtime-1")
        .is_none_or(Vec::is_empty));
}

#[test]
fn codex_approval_request_parser_accepts_nested_user_input_request() {
    let request = codex_approval_request_from_json_line(
            r#"{"type":"provider.user_input.requested","payload":{"request_id":"input-1","question":"Need confirmation"}}"#,
        )
        .expect("approval request parses");

    assert_eq!(request.approval_id.as_str(), "input-1");
    assert_eq!(request.prompt, "Need confirmation");
    assert_eq!(
        request.provider_event_type.as_deref(),
        Some("provider.user_input.requested")
    );
}

#[test]
fn codex_activity_payload_preserves_unknown_raw_json_fields() {
    let payload = codex_activity_payload_from_json(
        "codex",
        serde_json::json!({
            "type": "item.completed",
            "item": {
                "id": "item-1",
                "type": "command_execution",
                "command": "make c",
                "status": "completed",
                "future_field": { "kept": true }
            }
        }),
    );

    assert_eq!(
        payload
            .get("provider_event_type")
            .and_then(serde_json::Value::as_str),
        Some("item.completed")
    );
    assert_eq!(
        payload
            .get("provider_item_type")
            .and_then(serde_json::Value::as_str),
        Some("command_execution")
    );
    assert_eq!(
        payload
            .get("raw_event")
            .and_then(|raw| raw.get("item"))
            .and_then(|item| item.get("future_field"))
            .and_then(|field| field.get("kept"))
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
}

#[test]
fn codex_stdout_activity_event_records_malformed_jsonl_as_parse_error() {
    let command = command_fixture("command-codex-parse-error", CommandKind::SendTurn);
    let mut runtime_seqs = HashMap::new();

    let event = codex_stdout_activity_event(
        "codex",
        &command,
        &mut runtime_seqs,
        RuntimeSessionId::from("runtime-1"),
        Some(TurnId::from("turn-1")),
        "{bad json",
    );

    assert_eq!(event.kind, EventKind::ProviderActivity);
    assert_eq!(
        event
            .payload
            .0
            .get("provider_event_type")
            .and_then(serde_json::Value::as_str),
        Some("parse_error")
    );
    assert_eq!(runtime_seqs.get("runtime-1").copied(), Some(1));
}

#[cfg(unix)]
fn assert_codex_launch_flags(captured_args: &str) {
    for flag in [
        "--skip-git-repo-check",
        "--dangerously-bypass-approvals-and-sandbox",
    ] {
        assert!(
            captured_args.contains(&format!("\n{flag}\n")),
            "captured args did not include {flag}: {captured_args}"
        );
    }
}

#[tokio::test]
async fn codex_resume_runtime_reports_local_transcript_source() {
    let config = config_fixture_with_codex_binary("codex");
    let command = command_fixture("command-codex-resume", CommandKind::ResumeRuntime);
    let mut local_state = NodeLocalState::default();
    local_state
        .runtime_providers
        .insert("runtime-1".to_owned(), "codex".to_owned());
    local_state.runtime_transcripts.insert(
        "runtime-1".to_owned(),
        vec![
            ProviderTranscriptMessage {
                role: "user".to_owned(),
                content: "first question".to_owned(),
            },
            ProviderTranscriptMessage {
                role: "assistant".to_owned(),
                content: "first answer".to_owned(),
            },
        ],
    );

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;

    assert_eq!(outcome.status, CommandState::Completed);
    assert_eq!(
        event_kinds(&outcome.events_to_send),
        vec![EventKind::RuntimeResuming, EventKind::RuntimeReady]
    );
    assert_eq!(
        outcome.events_to_send[1]
            .payload
            .0
            .get("resume_source")
            .and_then(serde_json::Value::as_str),
        Some("node_local_transcript")
    );
    assert_eq!(
        outcome.events_to_send[1]
            .payload
            .0
            .get("transcript_messages")
            .and_then(serde_json::Value::as_i64),
        Some(2)
    );
}

#[cfg(unix)]
#[tokio::test]
async fn codex_send_turn_maps_nonzero_exit_to_exec_failed_error() {
    let codex_binary = fake_codex_failing_binary();
    let workspace_path =
        std::env::temp_dir().join(format!("uprava-codex-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace fixture creates");
    let config = config_fixture_with_codex_binary(codex_binary.display().to_string());
    let command =
        command_fixture_with_content("command-codex-failed", CommandKind::SendTurn, "status");
    let mut local_state = NodeLocalState::default();
    local_state
        .runtime_providers
        .insert("runtime-1".to_owned(), "codex".to_owned());
    local_state
        .runtime_workspace_paths
        .insert("runtime-1".to_owned(), workspace_path.display().to_string());

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;

    std::fs::remove_file(codex_binary).expect("codex fixture removes");
    std::fs::remove_dir_all(workspace_path).expect("workspace fixture removes");
    assert_eq!(outcome.status, CommandState::Failed);
    assert_eq!(
        event_kinds(&outcome.events_to_send),
        vec![
            EventKind::RuntimeRunning,
            EventKind::TurnStarted,
            EventKind::ProviderActivity,
            EventKind::RuntimeError,
        ]
    );
    assert_eq!(
        outcome.events_to_send[3]
            .payload
            .0
            .get("code")
            .and_then(serde_json::Value::as_str),
        Some("provider.exec_failed")
    );
    assert_eq!(
        outcome.events_to_send[2]
            .payload
            .0
            .get("provider_event_type")
            .and_then(serde_json::Value::as_str),
        Some("stderr")
    );
    assert!(outcome.events_to_send[3]
        .payload
        .0
        .get("message")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|message| {
            message.contains("status 42") && message.contains("provider crashed")
        }));
    assert_eq!(
        local_state.runtime_states.get("runtime-1").copied(),
        Some(RuntimeSessionState::Error)
    );
}

#[cfg(unix)]
#[tokio::test]
async fn codex_send_turn_maps_empty_final_message_to_empty_output_error() {
    let codex_binary = fake_codex_empty_output_binary();
    let workspace_path =
        std::env::temp_dir().join(format!("uprava-codex-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace fixture creates");
    let config = config_fixture_with_codex_binary(codex_binary.display().to_string());
    let command =
        command_fixture_with_content("command-codex-empty", CommandKind::SendTurn, "status");
    let mut local_state = NodeLocalState::default();
    local_state
        .runtime_providers
        .insert("runtime-1".to_owned(), "codex".to_owned());
    local_state
        .runtime_workspace_paths
        .insert("runtime-1".to_owned(), workspace_path.display().to_string());

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;

    std::fs::remove_file(codex_binary).expect("codex fixture removes");
    std::fs::remove_dir_all(workspace_path).expect("workspace fixture removes");
    assert_eq!(outcome.status, CommandState::Failed);
    let event_kinds = event_kinds(&outcome.events_to_send);
    assert!(event_kinds.len() >= 3);
    assert_eq!(event_kinds.first(), Some(&EventKind::RuntimeRunning));
    assert_eq!(event_kinds.get(1), Some(&EventKind::TurnStarted));
    assert!(event_kinds[2..event_kinds.len() - 1]
        .iter()
        .all(|kind| *kind == EventKind::ProviderActivity));
    assert_eq!(event_kinds.last(), Some(&EventKind::RuntimeError));
    let runtime_error = outcome
        .events_to_send
        .last()
        .expect("timeout emits a runtime error");
    assert_eq!(
        runtime_error
            .payload
            .0
            .get("code")
            .and_then(serde_json::Value::as_str),
        Some("provider.empty_output")
    );
    assert_eq!(
        local_state.runtime_states.get("runtime-1").copied(),
        Some(RuntimeSessionState::Error)
    );
}

#[cfg(unix)]
#[tokio::test]
async fn codex_send_turn_cancellation_interrupts_active_process() {
    let codex_binary = fake_codex_slow_binary();
    let workspace_path =
        std::env::temp_dir().join(format!("uprava-codex-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace fixture creates");
    let mut config = config_fixture_with_codex_binary(codex_binary.display().to_string());
    config.codex_timeout = Duration::from_secs(5);
    let command =
        command_fixture_with_content("command-codex-cancel", CommandKind::SendTurn, "status");
    let mut local_state = NodeLocalState::default();
    local_state
        .runtime_providers
        .insert("runtime-1".to_owned(), "codex".to_owned());
    local_state
        .runtime_workspace_paths
        .insert("runtime-1".to_owned(), workspace_path.display().to_string());
    let (cancel_tx, cancel_rx) = watch::channel(false);
    let cancel_task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = cancel_tx.send(true);
    });

    let outcome = prepare_command_dispatch_with_live_socket(
        &config,
        &mut local_state,
        &command,
        CommandExecutionContext {
            cancellation: Some(cancel_rx),
            ..CommandExecutionContext::default()
        },
    )
    .await;

    cancel_task.await.expect("cancel task joins");
    std::fs::remove_file(codex_binary).expect("codex fixture removes");
    std::fs::remove_dir_all(workspace_path).expect("workspace fixture removes");
    assert_eq!(outcome.status, CommandState::Failed);
    assert_eq!(
        event_kinds(&outcome.events_to_send),
        vec![
            EventKind::RuntimeRunning,
            EventKind::TurnStarted,
            EventKind::TurnInterrupted,
        ]
    );
    assert_eq!(
        outcome.events_to_send[2]
            .payload
            .0
            .get("code")
            .and_then(serde_json::Value::as_str),
        Some("provider.cancelled")
    );
    assert_eq!(
        local_state.runtime_states.get("runtime-1").copied(),
        Some(RuntimeSessionState::Interrupted)
    );
}

#[tokio::test]
async fn runtime_cancellation_signals_before_dispatch_capacity_is_available() {
    let cancellations = ExecutionCancellationRegistry::default();
    let guard = cancellations
        .begin(runtime_cancellation_key(&RuntimeSessionId::from(
            "runtime-1",
        )))
        .await;
    let mut cancellation = guard.receiver();
    let command = command_fixture("command-cancel-saturated", CommandKind::InterruptRuntime);
    let concurrency = Arc::new(Semaphore::new(0));
    let task = tokio::spawn({
        let cancellations = cancellations.clone();
        let concurrency = concurrency.clone();
        async move { prepare_command_dispatch_task(&command, &cancellations, concurrency).await }
    });

    timeout(Duration::from_secs(1), cancellation.changed())
        .await
        .expect("cancellation is not blocked by dispatch capacity")
        .expect("cancellation sender remains available");

    task.abort();
    let _ = task.await;
    cancellations.finish(guard).await;
    assert!(*cancellation.borrow());
}

#[tokio::test]
async fn deduction_cancellation_is_scoped_away_from_live_turns() {
    let cancellations = ExecutionCancellationRegistry::default();
    let live_guard = cancellations
        .begin(runtime_cancellation_key(&RuntimeSessionId::from(
            "runtime-1",
        )))
        .await;
    let deduction_guard = cancellations
        .begin(deduction_cancellation_key(
            &uprava_protocol::DeductionId::from("deduction-1"),
        ))
        .await;
    let live_cancellation = live_guard.receiver();
    let mut deduction_cancellation = deduction_guard.receiver();
    let mut command = command_fixture("command-cancel-deduction", CommandKind::CancelDeduction);
    command.payload = CommandPayload::CancelDeduction {
        deduction_id: uprava_protocol::DeductionId::from("deduction-1"),
    };

    let permit =
        prepare_command_dispatch_task(&command, &cancellations, Arc::new(Semaphore::new(1))).await;

    assert!(permit.is_some());
    assert!(!live_cancellation.has_changed().expect("live sender exists"));
    deduction_cancellation
        .changed()
        .await
        .expect("deduction sender remains available");
    assert!(*deduction_cancellation.borrow());
    cancellations.finish(live_guard).await;
    cancellations.finish(deduction_guard).await;

    let mut early_cancel = command_fixture(
        "command-cancel-early-deduction",
        CommandKind::CancelDeduction,
    );
    early_cancel.payload = CommandPayload::CancelDeduction {
        deduction_id: uprava_protocol::DeductionId::from("deduction-early"),
    };
    let permit =
        prepare_command_dispatch_task(&early_cancel, &cancellations, Arc::new(Semaphore::new(1)))
            .await;
    assert!(permit.is_some());
    let early_guard = cancellations
        .begin(deduction_cancellation_key(
            &uprava_protocol::DeductionId::from("deduction-early"),
        ))
        .await;
    assert!(*early_guard.receiver().borrow());
    cancellations.finish(early_guard).await;
}

#[cfg(unix)]
#[tokio::test]
async fn codex_send_turn_maps_slow_process_to_execution_timeout_error() {
    let codex_binary = fake_codex_slow_binary();
    let workspace_path =
        std::env::temp_dir().join(format!("uprava-codex-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace fixture creates");
    let mut config = config_fixture_with_codex_binary(codex_binary.display().to_string());
    config.codex_timeout = Duration::from_millis(50);
    let command =
        command_fixture_with_content("command-codex-timeout", CommandKind::SendTurn, "status");
    let mut local_state = NodeLocalState::default();
    local_state
        .runtime_providers
        .insert("runtime-1".to_owned(), "codex".to_owned());
    local_state
        .runtime_workspace_paths
        .insert("runtime-1".to_owned(), workspace_path.display().to_string());

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;

    std::fs::remove_file(codex_binary).expect("codex fixture removes");
    std::fs::remove_dir_all(workspace_path).expect("workspace fixture removes");
    assert_eq!(outcome.status, CommandState::Failed);
    assert_eq!(
        event_kinds(&outcome.events_to_send),
        vec![
            EventKind::RuntimeRunning,
            EventKind::TurnStarted,
            EventKind::RuntimeError,
        ]
    );
    assert_eq!(
        outcome.events_to_send[2]
            .payload
            .0
            .get("code")
            .and_then(serde_json::Value::as_str),
        Some("provider.execution_timeout")
    );
    assert_eq!(
        local_state.runtime_states.get("runtime-1").copied(),
        Some(RuntimeSessionState::Error)
    );
}
