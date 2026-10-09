use super::*;

#[tokio::test]
async fn validate_workspace_command_emits_placement_scoped_event() {
    let config = config_fixture();
    let workspace_path = std::env::temp_dir();
    let command = placement_command_fixture(
        "command-validate",
        "placement-1",
        "workspace",
        &workspace_path.display().to_string(),
    );
    let mut local_state = NodeLocalState::default();

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;

    assert_eq!(outcome.status, CommandState::Completed);
    assert_eq!(
        event_kinds(&outcome.events_to_send),
        vec![EventKind::WorkspaceValidated]
    );
    assert!(matches!(
        &outcome.events_to_send[0].scope_ref,
        ScopeRef::Placement { .. }
    ));
    assert_eq!(
        outcome.events_to_send[0]
            .payload
            .0
            .get("state")
            .and_then(serde_json::Value::as_str),
        Some("validated")
    );
    assert_eq!(
        local_state.placement_seqs.get("placement-1").copied(),
        Some(1)
    );
    assert_eq!(
        outcome.events_to_send[0]
            .correlation_id
            .as_ref()
            .map(CorrelationId::as_str),
        Some("correlation-1")
    );
}

#[tokio::test]
async fn validate_workspace_command_hard_blocks_outside_allowed_roots() {
    let mut config = config_fixture();
    config.workspace_paths =
        vec![std::env::temp_dir().join(format!("allowed-root-{}", Uuid::new_v4()))];
    let workspace_path = std::env::temp_dir().join(format!("outside-root-{}", Uuid::new_v4()));
    let command = placement_command_fixture(
        "command-validate-blocked",
        "placement-2",
        "workspace",
        &workspace_path.display().to_string(),
    );
    let mut local_state = NodeLocalState::default();

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;

    assert_eq!(
        outcome.events_to_send[0]
            .payload
            .0
            .get("state")
            .and_then(serde_json::Value::as_str),
        Some("error")
    );
    assert_eq!(
        outcome.events_to_send[0]
            .payload
            .0
            .get("resource_badges")
            .and_then(serde_json::Value::as_array)
            .and_then(|badges| badges.first())
            .and_then(|badge| badge.get("kind"))
            .and_then(serde_json::Value::as_str),
        Some("workspace_outside_allowed_roots")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn validate_workspace_command_rejects_symlink_escape_from_allowed_root() {
    let allowed_root = std::env::temp_dir().join(format!("uprava-allowed-{}", Uuid::new_v4()));
    let outside_root = std::env::temp_dir().join(format!("uprava-outside-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&allowed_root).expect("allowed root creates");
    std::fs::create_dir_all(&outside_root).expect("outside root creates");
    let escaped_workspace = allowed_root.join("escaped");
    std::os::unix::fs::symlink(&outside_root, &escaped_workspace)
        .expect("escaped workspace symlink creates");
    let mut config = config_fixture();
    config.workspace_paths = vec![allowed_root.clone()];
    let command = placement_command_fixture(
        "command-validate-symlink-escape",
        "placement-symlink-escape",
        "workspace",
        &escaped_workspace.display().to_string(),
    );
    let mut local_state = NodeLocalState::default();

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;

    std::fs::remove_dir_all(&allowed_root).expect("allowed root removes");
    std::fs::remove_dir_all(&outside_root).expect("outside root removes");
    assert_eq!(
        outcome.events_to_send[0]
            .payload
            .0
            .get("resource_badges")
            .and_then(serde_json::Value::as_array)
            .and_then(|badges| badges.first())
            .and_then(|badge| badge.get("kind"))
            .and_then(serde_json::Value::as_str),
        Some("workspace_outside_allowed_roots")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn codex_start_runtime_rejects_symlink_workspace_escape() {
    let allowed_root = std::env::temp_dir().join(format!("uprava-allowed-{}", Uuid::new_v4()));
    let outside_root = std::env::temp_dir().join(format!("uprava-outside-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&allowed_root).expect("allowed root creates");
    std::fs::create_dir_all(&outside_root).expect("outside root creates");
    let escaped_workspace = allowed_root.join("escaped");
    std::os::unix::fs::symlink(&outside_root, &escaped_workspace)
        .expect("escaped workspace symlink creates");
    let mut config = config_fixture();
    config.workspace_paths = vec![allowed_root.clone()];
    let mut command = command_fixture(
        "command-codex-start-symlink-escape",
        CommandKind::StartRuntime,
    );
    command.payload = CommandPayload::StartRuntime {
        provider: "codex".to_owned(),
        workspace_path: escaped_workspace.display().to_string(),
        execution_profile: AgentExecutionProfile::ExecCompatibility,
        effective_policy: None,
        effective_policy_hash: None,
    };
    let mut local_state = NodeLocalState::default();

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;

    std::fs::remove_dir_all(&allowed_root).expect("allowed root removes");
    std::fs::remove_dir_all(&outside_root).expect("outside root removes");
    assert_eq!(outcome.status, CommandState::Failed);
    assert_eq!(
        event_kinds(&outcome.events_to_send),
        vec![EventKind::RuntimeError]
    );
    assert!(local_state.runtime_workspace_paths.is_empty());
    assert_eq!(
        outcome.events_to_send[0]
            .payload
            .0
            .get("code")
            .and_then(serde_json::Value::as_str),
        Some("workspace.outside_allowed_roots")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn codex_send_turn_rechecks_cached_workspace_against_allowed_roots() {
    let allowed_root = std::env::temp_dir().join(format!("uprava-allowed-{}", Uuid::new_v4()));
    let outside_root = std::env::temp_dir().join(format!("uprava-outside-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&allowed_root).expect("allowed root creates");
    std::fs::create_dir_all(&outside_root).expect("outside root creates");
    let escaped_workspace = allowed_root.join("escaped");
    std::os::unix::fs::symlink(&outside_root, &escaped_workspace)
        .expect("escaped workspace symlink creates");
    let mut config = config_fixture();
    config.workspace_paths = vec![allowed_root.clone()];
    let command = command_fixture("command-codex-send-symlink-escape", CommandKind::SendTurn);
    let mut local_state = NodeLocalState::default();
    local_state
        .runtime_providers
        .insert("runtime-1".to_owned(), "codex".to_owned());
    local_state.runtime_workspace_paths.insert(
        "runtime-1".to_owned(),
        escaped_workspace.display().to_string(),
    );

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;

    std::fs::remove_dir_all(&allowed_root).expect("allowed root removes");
    std::fs::remove_dir_all(&outside_root).expect("outside root removes");
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
        Some("workspace.outside_allowed_roots")
    );
}

#[tokio::test]
async fn refresh_resource_snapshot_command_emits_placement_scoped_event() {
    let config = config_fixture();
    let workspace_path = std::env::temp_dir();
    let mut command = placement_command_fixture(
        "command-refresh",
        "placement-refresh",
        "workspace",
        &workspace_path.display().to_string(),
    );
    command.kind = CommandKind::RefreshResourceSnapshot;
    command.payload = CommandPayload::RefreshResourceSnapshot {
        display_name: "workspace".to_owned(),
        workspace_path: workspace_path.display().to_string(),
    };
    let mut local_state = NodeLocalState::default();

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;

    assert_eq!(outcome.status, CommandState::Completed);
    assert_eq!(
        event_kinds(&outcome.events_to_send),
        vec![EventKind::ResourceSnapshotUpdated]
    );
    assert!(matches!(
        &outcome.events_to_send[0].scope_ref,
        ScopeRef::Placement { .. }
    ));
    assert_eq!(
        local_state.placement_seqs.get("placement-refresh").copied(),
        Some(1)
    );
}

#[tokio::test]
async fn read_workspace_file_command_returns_text_content() {
    let config = config_fixture();
    let workspace_path =
        std::env::temp_dir().join(format!("uprava-node-inspector-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace fixture creates");
    std::fs::write(workspace_path.join("README.md"), "hello inspector")
        .expect("text fixture writes");
    let mut command = placement_command_fixture(
        "command-read-file",
        "placement-read-file",
        "workspace",
        &workspace_path.display().to_string(),
    );
    command.kind = CommandKind::ReadWorkspaceFile;
    command.payload = CommandPayload::ReadWorkspaceFile {
        workspace_path: workspace_path.display().to_string(),
        path: "README.md".to_owned(),
    };
    let mut local_state = NodeLocalState::default();

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;
    let response = serde_json::from_value::<WorkspaceFileContentResponse>(outcome.result_payload.0)
        .expect("workspace file response decodes");
    std::fs::remove_dir_all(&workspace_path).expect("workspace fixture removes");

    assert_eq!(outcome.status, CommandState::Completed);
    assert_eq!(response.metadata.status, WorkspaceEntryStatus::Readable);
    assert_eq!(response.content.as_deref(), Some("hello inspector"));
}

#[tokio::test]
async fn read_workspace_file_command_replays_payload_after_restart() {
    let config = config_fixture();
    let workspace_path =
        std::env::temp_dir().join(format!("uprava-node-inspector-{}", Uuid::new_v4()));
    let state_path = std::env::temp_dir().join(format!("uprava-node-{}.json", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace fixture creates");
    std::fs::write(workspace_path.join("README.md"), "hello inspector")
        .expect("text fixture writes");
    let mut command = placement_command_fixture(
        "command-read-file-replay",
        "placement-read-file-replay",
        "workspace",
        &workspace_path.display().to_string(),
    );
    command.kind = CommandKind::ReadWorkspaceFile;
    command.payload = CommandPayload::ReadWorkspaceFile {
        workspace_path: workspace_path.display().to_string(),
        path: "README.md".to_owned(),
    };
    let mut local_state = NodeLocalState::default();
    let first = prepare_command_dispatch(&config, &mut local_state, &command).await;
    let first_payload = first.result_payload.clone();
    local_state
        .save(&state_path)
        .expect("node state with result payload saves");

    let mut reloaded_state = NodeLocalState::load(&state_path).expect("node state reloads");
    let second = prepare_command_dispatch(&config, &mut reloaded_state, &command).await;
    let response =
        serde_json::from_value::<WorkspaceFileContentResponse>(second.result_payload.0.clone())
            .expect("replayed workspace file response decodes");

    std::fs::remove_file(&state_path).expect("state fixture removes");
    std::fs::remove_dir_all(&workspace_path).expect("workspace fixture removes");
    assert_eq!(second.status, CommandState::Completed);
    assert!(!second.state_changed);
    assert_eq!(second.result_payload.0, first_payload.0);
    assert_eq!(response.content.as_deref(), Some("hello inspector"));
}

#[tokio::test]
async fn list_workspace_tree_marks_and_allows_generated_directories() {
    let config = config_fixture();
    let workspace_path =
        std::env::temp_dir().join(format!("uprava-node-inspector-{}", Uuid::new_v4()));
    std::fs::create_dir_all(workspace_path.join("target/debug"))
        .expect("generated fixture creates");
    std::fs::write(workspace_path.join("target/debug/app"), "compiled")
        .expect("generated file fixture writes");
    let mut command = placement_command_fixture(
        "command-list-tree",
        "placement-list-tree",
        "workspace",
        &workspace_path.display().to_string(),
    );
    command.kind = CommandKind::ListWorkspaceTree;
    command.payload = CommandPayload::ListWorkspaceTree {
        workspace_path: workspace_path.display().to_string(),
        path: ".".to_owned(),
    };
    let mut local_state = NodeLocalState::default();

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;
    let response = serde_json::from_value::<WorkspaceTreeResponse>(outcome.result_payload.0)
        .expect("workspace tree response decodes");
    let target = response
        .root
        .children
        .iter()
        .find(|entry| entry.name == "target")
        .expect("target entry appears");
    std::fs::remove_dir_all(&workspace_path).expect("workspace fixture removes");

    assert_eq!(outcome.status, CommandState::Completed);
    assert_eq!(target.status, WorkspaceEntryStatus::Directory);
    assert_eq!(
        target.classification,
        WorkspaceEntryClassification::Generated
    );
    assert!(target.expandable);
    assert!(target.children.is_empty());
}

#[tokio::test]
async fn list_workspace_tree_shows_dotfiles_and_limits_sorted_children() {
    let config = config_fixture();
    let workspace_path =
        std::env::temp_dir().join(format!("uprava-node-tree-limit-{}", Uuid::new_v4()));
    std::fs::create_dir_all(workspace_path.join(".github")).expect("dot directory creates");
    std::fs::write(workspace_path.join(".env"), "visible").expect("dot file creates");
    for index in 0..100 {
        std::fs::write(
            workspace_path.join(format!("file-{index:03}.txt")),
            "fixture",
        )
        .expect("limit fixture creates");
    }
    let mut command = placement_command_fixture(
        "command-list-limited-tree",
        "placement-list-limited-tree",
        "workspace",
        &workspace_path.display().to_string(),
    );
    command.kind = CommandKind::ListWorkspaceTree;
    command.payload = CommandPayload::ListWorkspaceTree {
        workspace_path: workspace_path.display().to_string(),
        path: ".".to_owned(),
    };
    let mut local_state = NodeLocalState::default();

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;
    let response = serde_json::from_value::<WorkspaceTreeResponse>(outcome.result_payload.0)
        .expect("workspace tree response decodes");
    std::fs::remove_dir_all(&workspace_path).expect("workspace fixture removes");

    assert_eq!(outcome.status, CommandState::Completed);
    assert_eq!(response.total_entries, Some(102));
    assert!(response.truncated);
    assert_eq!(response.root.children.len(), 100);
    assert_eq!(response.root.children[0].name, ".github");
    assert_eq!(response.root.children[1].name, ".env");
}

#[tokio::test]
async fn read_workspace_file_command_rejects_parent_path_escape() {
    let config = config_fixture();
    let workspace_path =
        std::env::temp_dir().join(format!("uprava-node-inspector-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace fixture creates");
    let mut command = placement_command_fixture(
        "command-read-escape",
        "placement-read-escape",
        "workspace",
        &workspace_path.display().to_string(),
    );
    command.kind = CommandKind::ReadWorkspaceFile;
    command.payload = CommandPayload::ReadWorkspaceFile {
        workspace_path: workspace_path.display().to_string(),
        path: "../secret.txt".to_owned(),
    };
    let mut local_state = NodeLocalState::default();

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;
    let error_code = outcome
        .result_payload
        .0
        .get("error_code")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    std::fs::remove_dir_all(&workspace_path).expect("workspace fixture removes");

    assert_eq!(outcome.status, CommandState::Failed);
    assert_eq!(error_code.as_deref(), Some("workspace.path_escape"));
}

#[tokio::test]
async fn write_workspace_file_command_updates_text_when_expected_content_matches() {
    let config = config_fixture();
    let workspace_path =
        std::env::temp_dir().join(format!("uprava-node-inspector-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace fixture creates");
    std::fs::write(workspace_path.join("README.md"), "before").expect("text fixture writes");
    let mut command = placement_command_fixture(
        "command-write-file",
        "placement-write-file",
        "workspace",
        &workspace_path.display().to_string(),
    );
    command.kind = CommandKind::WriteWorkspaceFile;
    command.payload = CommandPayload::WriteWorkspaceFile {
        workspace_path: workspace_path.display().to_string(),
        request: WorkspaceFileWriteRequest {
            path: "README.md".to_owned(),
            content: "after".to_owned(),
            expected_content: Some("before".to_owned()),
        },
    };
    let mut local_state = NodeLocalState::default();

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;
    let response = serde_json::from_value::<WorkspaceFileWriteResponse>(outcome.result_payload.0)
        .expect("workspace write response decodes");
    let written =
        std::fs::read_to_string(workspace_path.join("README.md")).expect("written file reads");
    std::fs::remove_dir_all(&workspace_path).expect("workspace fixture removes");

    assert_eq!(outcome.status, CommandState::Completed);
    assert_eq!(response.path, "README.md");
    assert_eq!(written, "after");
    assert_eq!(
        event_kinds(&outcome.events_to_send),
        vec![EventKind::WorkspaceFileWritten]
    );
    assert!(outcome.events_to_send[0]
        .cause_refs
        .iter()
        .any(|reference| matches!(
            reference,
            UpravaRef::Command { command_id } if command_id == &command.command_id
        )));
    assert!(outcome.events_to_send[0]
        .result_refs
        .iter()
        .any(|reference| matches!(reference, UpravaRef::WorkspaceEdit { .. })));
}

#[tokio::test]
async fn write_workspace_file_command_replays_payload_after_restart_without_rewriting() {
    let config = config_fixture();
    let workspace_path =
        std::env::temp_dir().join(format!("uprava-node-inspector-{}", Uuid::new_v4()));
    let state_path = std::env::temp_dir().join(format!("uprava-node-{}.json", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace fixture creates");
    std::fs::write(workspace_path.join("README.md"), "before").expect("text fixture writes");
    let mut command = placement_command_fixture(
        "command-write-file-replay",
        "placement-write-file-replay",
        "workspace",
        &workspace_path.display().to_string(),
    );
    command.kind = CommandKind::WriteWorkspaceFile;
    command.payload = CommandPayload::WriteWorkspaceFile {
        workspace_path: workspace_path.display().to_string(),
        request: WorkspaceFileWriteRequest {
            path: "README.md".to_owned(),
            content: "after".to_owned(),
            expected_content: Some("before".to_owned()),
        },
    };
    let mut local_state = NodeLocalState::default();
    let first = prepare_command_dispatch(&config, &mut local_state, &command).await;
    let first_payload = first.result_payload.clone();
    local_state
        .save(&state_path)
        .expect("node state with write result payload saves");
    std::fs::write(workspace_path.join("README.md"), "external change")
        .expect("post-command file fixture changes");

    let mut reloaded_state = NodeLocalState::load(&state_path).expect("node state reloads");
    let second = prepare_command_dispatch(&config, &mut reloaded_state, &command).await;
    let written =
        std::fs::read_to_string(workspace_path.join("README.md")).expect("written file reads");

    std::fs::remove_file(&state_path).expect("state fixture removes");
    std::fs::remove_dir_all(&workspace_path).expect("workspace fixture removes");
    assert_eq!(second.status, CommandState::Completed);
    assert!(!second.state_changed);
    assert_eq!(second.result_payload.0, first_payload.0);
    assert_eq!(written, "external change");
}

#[tokio::test]
async fn write_workspace_file_command_rejects_stale_expected_content() {
    let config = config_fixture();
    let workspace_path =
        std::env::temp_dir().join(format!("uprava-node-inspector-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace fixture creates");
    std::fs::write(workspace_path.join("README.md"), "current").expect("text fixture writes");
    let mut command = placement_command_fixture(
        "command-write-conflict",
        "placement-write-conflict",
        "workspace",
        &workspace_path.display().to_string(),
    );
    command.kind = CommandKind::WriteWorkspaceFile;
    command.payload = CommandPayload::WriteWorkspaceFile {
        workspace_path: workspace_path.display().to_string(),
        request: WorkspaceFileWriteRequest {
            path: "README.md".to_owned(),
            content: "after".to_owned(),
            expected_content: Some("before".to_owned()),
        },
    };
    let mut local_state = NodeLocalState::default();

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;
    let error_code = outcome
        .result_payload
        .0
        .get("error_code")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    std::fs::remove_dir_all(&workspace_path).expect("workspace fixture removes");

    assert_eq!(outcome.status, CommandState::Failed);
    assert_eq!(error_code.as_deref(), Some("workspace.write_conflict"));
}

#[cfg(unix)]
#[tokio::test]
async fn write_workspace_file_command_rejects_symlink_target() {
    let config = config_fixture();
    let workspace_path =
        std::env::temp_dir().join(format!("uprava-node-inspector-{}", Uuid::new_v4()));
    let outside_path = std::env::temp_dir().join(format!("uprava-node-outside-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace fixture creates");
    std::os::unix::fs::symlink(&outside_path, workspace_path.join("link.txt"))
        .expect("symlink fixture creates");
    let mut command = placement_command_fixture(
        "command-write-symlink",
        "placement-write-symlink",
        "workspace",
        &workspace_path.display().to_string(),
    );
    command.kind = CommandKind::WriteWorkspaceFile;
    command.payload = CommandPayload::WriteWorkspaceFile {
        workspace_path: workspace_path.display().to_string(),
        request: WorkspaceFileWriteRequest {
            path: "link.txt".to_owned(),
            content: "after".to_owned(),
            expected_content: None,
        },
    };
    let mut local_state = NodeLocalState::default();

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;
    let error_code = outcome
        .result_payload
        .0
        .get("error_code")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    std::fs::remove_dir_all(&workspace_path).expect("workspace fixture removes");

    assert_eq!(outcome.status, CommandState::Failed);
    assert_eq!(error_code.as_deref(), Some("workspace.write_symlink"));
    assert!(!outside_path.exists());
}

#[tokio::test]
async fn run_workspace_command_captures_stdout_and_exit_status() {
    let config = config_fixture();
    let workspace_path =
        std::env::temp_dir().join(format!("uprava-node-inspector-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace fixture creates");
    let mut command = placement_command_fixture(
        "command-run-workspace",
        "placement-run-workspace",
        "workspace",
        &workspace_path.display().to_string(),
    );
    command.kind = CommandKind::RunWorkspaceCommand;
    command.payload = CommandPayload::RunWorkspaceCommand {
        workspace_path: workspace_path.display().to_string(),
        request: WorkspaceCommandRunRequest {
            command: "rustc".to_owned(),
            args: vec!["--version".to_owned()],
            intent: WorkspaceCommandIntent::Command,
            label: None,
            timeout_seconds: Some(30),
        },
    };
    let mut local_state = NodeLocalState::default();

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;
    let response = serde_json::from_value::<WorkspaceCommandRunResponse>(outcome.result_payload.0)
        .expect("workspace command response decodes");
    std::fs::remove_dir_all(&workspace_path).expect("workspace fixture removes");

    assert_eq!(outcome.status, CommandState::Completed);
    assert!(response.success, "stderr: {}", response.stderr);
    assert!(response.stdout.contains("rustc"));
}

#[tokio::test]
async fn run_workspace_command_rejects_disallowed_executable() {
    let config = config_fixture();
    let workspace_path =
        std::env::temp_dir().join(format!("uprava-node-inspector-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace fixture creates");
    let mut command = placement_command_fixture(
        "command-run-disallowed",
        "placement-run-disallowed",
        "workspace",
        &workspace_path.display().to_string(),
    );
    command.kind = CommandKind::RunWorkspaceCommand;
    command.payload = CommandPayload::RunWorkspaceCommand {
        workspace_path: workspace_path.display().to_string(),
        request: WorkspaceCommandRunRequest {
            command: "sh".to_owned(),
            args: vec!["-c".to_owned(), "echo blocked".to_owned()],
            intent: WorkspaceCommandIntent::Command,
            label: None,
            timeout_seconds: Some(30),
        },
    };
    let mut local_state = NodeLocalState::default();

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;
    let error_code = outcome
        .result_payload
        .0
        .get("error_code")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    std::fs::remove_dir_all(&workspace_path).expect("workspace fixture removes");

    assert_eq!(outcome.status, CommandState::Failed);
    assert_eq!(error_code.as_deref(), Some("workspace.command_not_allowed"));
}

#[tokio::test]
async fn run_workspace_process_caps_stdout_during_execution() {
    let output = run_workspace_process(
        &std::env::temp_dir(),
        "rustc",
        &["--print".to_owned(), "target-list".to_owned()],
        Duration::from_secs(30),
        64,
        64,
    )
    .await;

    assert!(output.success, "stderr: {}", output.stderr);
    assert!(output.stdout.len() <= 64);
    assert!(output.stdout_truncated);
}

#[tokio::test]
async fn read_workspace_diff_command_returns_git_diff() {
    let config = config_fixture();
    let workspace_path =
        std::env::temp_dir().join(format!("uprava-node-inspector-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace fixture creates");
    StdCommand::new("git")
        .arg("init")
        .current_dir(&workspace_path)
        .status()
        .expect("git init starts");
    std::fs::write(workspace_path.join("README.md"), "before\n").expect("text fixture writes");
    StdCommand::new("git")
        .args(["add", "README.md"])
        .current_dir(&workspace_path)
        .status()
        .expect("git add starts");
    StdCommand::new("git")
        .args(["-c", "user.email=test@example.invalid"])
        .args(["-c", "user.name=Uprava Test"])
        .args(["commit", "-m", "initial"])
        .current_dir(&workspace_path)
        .status()
        .expect("git commit starts");
    std::fs::write(workspace_path.join("README.md"), "after\n").expect("text fixture writes");
    let mut command = placement_command_fixture(
        "command-read-diff",
        "placement-read-diff",
        "workspace",
        &workspace_path.display().to_string(),
    );
    command.kind = CommandKind::ReadWorkspaceDiff;
    command.payload = CommandPayload::ReadWorkspaceDiff {
        workspace_path: workspace_path.display().to_string(),
        request: WorkspaceDiffRequest::default(),
    };
    let mut local_state = NodeLocalState::default();

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;
    let response = serde_json::from_value::<WorkspaceDiffResponse>(outcome.result_payload.0)
        .expect("workspace diff response decodes");
    std::fs::remove_dir_all(&workspace_path).expect("workspace fixture removes");

    assert_eq!(outcome.status, CommandState::Completed);
    assert!(response.diff.contains("-before"));
    assert!(response.diff.contains("+after"));
    assert_eq!(response.scope, WorkspaceDiffScope::All);
    assert_eq!(response.git_snapshot.state, GitRepositoryState::Ready);
    assert_eq!(response.changed_files.len(), 1);
    assert_eq!(response.hunks.len(), 1);
    assert!(response.hunks[0].patch.contains("+after"));
}

#[tokio::test]
async fn selected_untracked_file_returns_bounded_monaco_contents() {
    let config = config_fixture();
    let workspace_path =
        std::env::temp_dir().join(format!("uprava-node-untracked-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace fixture creates");
    StdCommand::new("git")
        .arg("init")
        .current_dir(&workspace_path)
        .status()
        .expect("git init starts");
    std::fs::write(workspace_path.join("notes.md"), "new review note\n")
        .expect("untracked fixture writes");
    let mut command = placement_command_fixture(
        "command-untracked-diff",
        "placement-untracked-diff",
        "workspace",
        &workspace_path.display().to_string(),
    );
    command.kind = CommandKind::ReadWorkspaceDiff;
    command.payload = CommandPayload::ReadWorkspaceDiff {
        workspace_path: workspace_path.display().to_string(),
        request: WorkspaceDiffRequest {
            scope: WorkspaceDiffScope::All,
            path: Some("notes.md".to_owned()),
        },
    };
    let mut local_state = NodeLocalState::default();

    let outcome = prepare_command_dispatch(&config, &mut local_state, &command).await;
    let response = serde_json::from_value::<WorkspaceDiffResponse>(outcome.result_payload.0)
        .expect("workspace diff response decodes");
    std::fs::remove_dir_all(&workspace_path).expect("workspace fixture removes");

    assert_eq!(outcome.status, CommandState::Completed);
    assert_eq!(response.original.as_deref(), Some(""));
    assert_eq!(response.modified.as_deref(), Some("new review note\n"));
    assert_eq!(response.changed_files[0].path, "notes.md");
    assert_eq!(
        response.changed_files[0].worktree_status,
        Some(GitChangeKind::Untracked)
    );
}

#[test]
fn git_snapshot_reports_index_worktree_and_linked_worktree_state() {
    let fixture_root = std::env::temp_dir().join(format!("uprava-git-snapshot-{}", Uuid::new_v4()));
    let repository = fixture_root.join("repository");
    let linked = fixture_root.join("linked");
    std::fs::create_dir_all(&repository).expect("repository fixture creates");
    StdCommand::new("git")
        .arg("init")
        .current_dir(&repository)
        .status()
        .expect("git init starts");
    std::fs::write(repository.join("README.md"), "before\n").expect("fixture writes");
    StdCommand::new("git")
        .args(["add", "README.md"])
        .current_dir(&repository)
        .status()
        .expect("git add starts");
    StdCommand::new("git")
        .args(["-c", "user.email=test@example.invalid"])
        .args(["-c", "user.name=Uprava Test"])
        .args(["commit", "-m", "initial"])
        .current_dir(&repository)
        .status()
        .expect("git commit starts");
    std::fs::write(repository.join("README.md"), "staged\n").expect("staged fixture writes");
    StdCommand::new("git")
        .args(["add", "README.md"])
        .current_dir(&repository)
        .status()
        .expect("git add starts");
    std::fs::write(repository.join("README.md"), "unstaged\n").expect("unstaged fixture writes");
    std::fs::write(repository.join("notes.md"), "untracked\n").expect("fixture writes");

    let snapshot = git_workspace_snapshot(&repository).expect("git snapshot returns");
    StdCommand::new("git")
        .args(["worktree", "add", "-b", "review-linked"])
        .arg(&linked)
        .current_dir(&repository)
        .status()
        .expect("linked worktree creates");
    let linked_snapshot = git_workspace_snapshot(&linked).expect("linked snapshot returns");
    std::fs::remove_dir_all(&fixture_root).expect("fixture removes");

    assert_eq!(snapshot.state, GitRepositoryState::Ready);
    assert_eq!(snapshot.staged_count, 1);
    assert_eq!(snapshot.unstaged_count, 2);
    assert_eq!(snapshot.untracked_count, 1);
    assert_eq!(snapshot.worktree_kind, Some(GitWorktreeKind::Primary));
    assert_eq!(linked_snapshot.worktree_kind, Some(GitWorktreeKind::Linked));
}

#[test]
fn git_snapshot_bounds_changed_file_projection() {
    let mut status = String::from("# branch.oid (initial)\0# branch.head main\0");
    for index in 0..=MAX_GIT_CHANGED_FILES {
        status.push_str(&format!("? file-{index}.txt\0"));
    }

    let snapshot = parse_git_porcelain_v2(&status, Utc::now());

    assert_eq!(snapshot.changed_files.len(), MAX_GIT_CHANGED_FILES);
    assert_eq!(snapshot.untracked_count as usize, MAX_GIT_CHANGED_FILES + 1);
    assert!(snapshot.truncated);
}

#[test]
fn workspace_diff_bounds_hunk_projection() {
    let diff = "@@ -1 +1 @@\n-old\n+new\n".repeat(MAX_WORKSPACE_DIFF_HUNKS + 1);

    let hunks = parse_workspace_diff_hunks("diff-1", &diff);

    assert_eq!(hunks.len(), MAX_WORKSPACE_DIFF_HUNKS);
    assert_eq!(hunks[0].hunk_id, "diff-1:hunk-1");
    assert_eq!(
        hunks.last().map(|hunk| hunk.hunk_id.as_str()),
        Some("diff-1:hunk-256")
    );
}

#[test]
fn git_status_badges_parse_branch_dirty_and_tracking_state() {
    let status = "## main...origin/main [ahead 1, behind 2]\n M src/main.rs\n?? notes.md\n";

    let badges = git_status_badges(status);

    assert_eq!(
        badge_kinds(&badges),
        vec![
            "git_branch",
            "dirty_workspace",
            "branch_behind",
            "branch_ahead",
        ]
    );
    assert_eq!(badges[0].label, "Git branch: main");
    assert_eq!(badges[1].severity, WarningSeverity::Warning);
    assert_eq!(badges[2].severity, WarningSeverity::Warning);
    assert_eq!(badges[3].severity, WarningSeverity::Info);
}

#[test]
fn git_status_badges_report_clean_branch_without_dirty_warning() {
    let status = "## feature/runtime-controls...origin/feature/runtime-controls\n";

    let badges = git_status_badges(status);

    assert_eq!(badge_kinds(&badges), vec!["git_branch"]);
    assert_eq!(badges[0].label, "Git branch: feature/runtime-controls");
}

#[test]
fn resource_warnings_ignore_non_git_workspace() {
    let workspace_path = std::env::temp_dir().join(format!("uprava-non-git-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_path).expect("workspace dir creates");

    let badges = resource_warnings(&workspace_path);
    std::fs::remove_dir_all(&workspace_path).expect("workspace dir removes");

    assert!(badges.is_empty());
}
