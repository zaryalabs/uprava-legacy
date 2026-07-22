use super::*;
use crate::runtime::config::parse_numeric_version;

#[test]
fn node_config_from_env_requires_workspace_roots() {
    let _lock = env_lock();
    let _env = EnvGuard::cleared(NODE_CONFIG_ENV_VARS);

    let error = NodeConfig::from_env().expect_err("missing workspace roots should fail");

    assert!(error
        .to_string()
        .contains("UPRAVA_NODE_WORKSPACES must list one or more allowed workspace roots"));
}

#[test]
fn node_config_from_env_defaults_codex_timeout_to_one_day() {
    let _lock = env_lock();
    let _env = EnvGuard::cleared(NODE_CONFIG_ENV_VARS);
    std::env::set_var("UPRAVA_NODE_WORKSPACES", std::env::temp_dir());

    let config = NodeConfig::from_env().expect("default node config parses");

    assert_eq!(config.codex_timeout, Duration::from_secs(24 * 60 * 60));
}

#[test]
fn node_config_from_env_parses_overrides_and_workspace_list() {
    let _lock = env_lock();
    let _env = EnvGuard::cleared(NODE_CONFIG_ENV_VARS);
    let state_path = std::env::temp_dir().join(format!("uprava-node-{}.json", Uuid::new_v4()));
    std::env::set_var("UPRAVA_CORE_URL", "http://127.0.0.1:19090");
    std::env::set_var("UPRAVA_NODE_DISPLAY_NAME", "Desktop Node");
    std::env::set_var("UPRAVA_NODE_HEARTBEAT_SECONDS", "2");
    std::env::set_var("UPRAVA_NODE_STATE_PATH", &state_path);
    std::env::set_var("UPRAVA_NODE_WORKSPACES", "/tmp/a, ,/tmp/b");
    std::env::set_var("UPRAVA_CODEX_BINARY", "/usr/local/bin/codex");
    std::env::set_var("UPRAVA_CODEX_IGNORE_USER_CONFIG", "true");
    std::env::set_var("UPRAVA_CODEX_TIMEOUT_SECONDS", "7");

    let config = NodeConfig::from_env().expect("overridden node config parses");

    assert_eq!(config.core_url.as_str(), "http://127.0.0.1:19090/");
    assert_eq!(config.display_name, "Desktop Node");
    assert_eq!(config.heartbeat_interval, Duration::from_secs(2));
    assert_eq!(config.state_path, state_path);
    assert_eq!(
        config.workspace_paths,
        vec![PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")]
    );
    assert_eq!(config.codex_binary, "/usr/local/bin/codex");
    assert!(config.codex_ignore_user_config);
    assert_eq!(config.codex_timeout, Duration::from_secs(7));
}

#[test]
fn node_config_from_env_rejects_invalid_duration_values() {
    let _lock = env_lock();
    let _env = EnvGuard::cleared(NODE_CONFIG_ENV_VARS);
    std::env::set_var("UPRAVA_NODE_WORKSPACES", std::env::temp_dir());
    std::env::set_var("UPRAVA_NODE_HEARTBEAT_SECONDS", "soon");

    let error = NodeConfig::from_env().expect_err("invalid heartbeat should fail");

    assert!(error
        .to_string()
        .contains("UPRAVA_NODE_HEARTBEAT_SECONDS must be an unsigned integer"));
}

#[test]
fn node_config_restricts_deferred_auth_opensandbox_to_loopback() {
    let _lock = env_lock();
    let _env = EnvGuard::cleared(NODE_CONFIG_ENV_VARS);
    std::env::set_var("UPRAVA_NODE_WORKSPACES", std::env::temp_dir());
    std::env::set_var("UPRAVA_OPENSANDBOX_URL", "http://opensandbox.internal:8080");

    let error = NodeConfig::from_env().expect_err("remote insecure OpenSandbox rejects");

    assert!(error.to_string().contains("loopback HTTP URL"));
}

#[test]
fn command_available_returns_false_for_missing_absolute_binary() {
    let missing = std::env::temp_dir().join(format!("missing-codex-{}", Uuid::new_v4()));

    assert!(!command_available(&missing.display().to_string()));
}

#[test]
fn command_available_resolves_binary_from_path() {
    let bin_dir = std::env::temp_dir().join(format!("uprava-node-bin-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&bin_dir).expect("bin dir creates");
    let codex_path = bin_dir.join("codex");
    std::fs::write(&codex_path, "").expect("codex fixture writes");

    let available = command_available_in_search_path("codex", bin_dir.as_os_str());
    std::fs::remove_dir_all(bin_dir).expect("bin dir removes");

    assert!(available);
}

#[test]
fn capabilities_report_codex_unavailable_when_binary_is_missing() {
    let config = config_fixture_with_codex_binary(
        std::env::temp_dir()
            .join(format!("missing-codex-{}", Uuid::new_v4()))
            .display()
            .to_string(),
    );

    let capability = capabilities(&config)
        .into_iter()
        .find(|capability| capability.key == "provider.codex")
        .expect("codex capability exists");

    assert!(matches!(
        capability.value,
        CapabilityValue::Provider {
            available: false,
            ..
        }
    ));
}

#[cfg(unix)]
#[test]
fn managed_capabilities_require_a_supported_version_probe() {
    let codex_binary = fake_codex_success_binary();
    let mut config = config_fixture_with_codex_binary(codex_binary.display().to_string());
    config.codex_managed_unavailable_reason = Some("version_unsupported".to_owned());

    let capabilities = capabilities(&config);
    std::fs::remove_file(codex_binary).expect("fake Codex removes");

    for required in ProviderRuntimeCapability::required_for_managed_codex() {
        let capability = capabilities
            .iter()
            .find(|capability| capability.key == required.as_str())
            .expect("managed capability is reported");
        assert!(matches!(
            &capability.value,
            CapabilityValue::Provider {
                available: false,
                unavailable_reason: Some(reason),
                ..
            } if reason == "version_unsupported"
        ));
    }
}

#[test]
fn capabilities_report_the_probed_codex_version_for_policy_pinning() {
    let mut config = config_fixture();
    config.codex_version = Some("codex-cli 0.144.1".to_owned());

    let capability = capabilities(&config)
        .into_iter()
        .find(|capability| capability.key == "provider.codex.version")
        .expect("Codex version capability exists");

    assert!(matches!(
        capability.value,
        CapabilityValue::Extension { name, value }
            if name == "provider_version" && value.0 == serde_json::json!("codex-cli 0.144.1")
    ));
}

#[test]
fn codex_version_parser_accepts_pinned_and_prerelease_versions() {
    assert_eq!(parse_numeric_version("0.144.1"), Some((0, 144, 1)));
    assert_eq!(parse_numeric_version("v0.145.0-beta.1"), Some((0, 145, 0)));
    assert_eq!(parse_numeric_version("unknown"), None);
}

#[test]
fn compatible_control_frame_has_no_protocol_error() {
    let frame = ControlFrame::HelloAck {
        frame_id: "hello-ack-1".to_owned(),
        protocol_version: API_VERSION.to_owned(),
        sent_at: Utc::now(),
    };

    assert!(control_frame_protocol_error(&frame).is_none());
}

#[test]
fn incompatible_control_frame_builds_safe_protocol_error() {
    let frame = ControlFrame::CommandDispatch {
        frame_id: "dispatch-1".to_owned(),
        protocol_version: "v0".to_owned(),
        sent_at: Utc::now(),
        command: Box::new(command_fixture(
            "bad-protocol-command",
            CommandKind::SendTurn,
        )),
    };

    let error_frame = control_frame_protocol_error(&frame).expect("incompatible frame rejects");

    let ControlFrame::ControlError {
        protocol_version,
        error,
        ..
    } = error_frame
    else {
        panic!("expected control error frame");
    };
    assert_eq!(protocol_version, API_VERSION);
    assert_eq!(error.error_code, "control.protocol_incompatible");
    assert!(!error.retryable);
    assert_eq!(
        error
            .details
            .0
            .get("received_protocol_version")
            .and_then(serde_json::Value::as_str),
        Some("v0")
    );
}
