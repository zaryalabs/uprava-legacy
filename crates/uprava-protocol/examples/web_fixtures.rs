use std::collections::BTreeMap;

use chrono::{TimeZone, Utc};
use serde::Serialize;
use serde_json::{json, Map, Value};
use uprava_protocol::{
    compute_tool_schema_hash, ActorRef, AgentExecutionProfile, ArtifactDetail, ArtifactId,
    ArtifactState, ArtifactSummary, ArtifactVersion, CommandAcceptedResponse, CommandKind,
    CommandState, ContributionResolutionMode, ContributionTarget, ContributionTargetResolution,
    CorrelationId, EffectiveContribution, EffectiveContributionState, EventEnvelope, EventId,
    EventKind, ExecuteToolRequest, ExecuteToolResponse, InspectToolRequest, InspectToolResponse,
    IntegrationAuthState, IntegrationConnectRequest, IntegrationConnectResponse,
    IntegrationConnectionSummary, IntegrationConnectionsResponse, IntegrationDesiredState,
    IntegrationDisconnectRequest, IntegrationDisconnectResponse, McpAccessLeaseClaims,
    McpAccessLeaseId, McpDependencyActualState, McpDependencyInstanceId, McpDependencyStatus,
    McpDependencyStatusesResponse, MonacoThemeV1, ObservedCapabilitiesResponse, ObservedCapability,
    ObservedCapabilityState, PluginCompatibility, PluginCompatibilityState, PluginContribution,
    PluginDesiredState, PluginEffectiveState, PluginId, PluginInstallSource,
    PluginInstallationSummary, PluginListResponse, PluginPackageSummary, PluginTrustLevel,
    PolicyDecision, ProjectId, ProjectPlacementId, ProviderApprovalMode, ProviderRuntimeCapability,
    ProviderSandboxMode, RuntimeNetworkPosture, RuntimeToolExposureSummary, ScopeRef,
    SearchToolsRequest, SearchToolsResponse, SessionThreadId, TerminalId, TerminalThemeV1,
    ThemeColorScheme, ThemeContributionV1, ThemeKind, ToolAvailability, ToolAvailabilityResponse,
    ToolAvailabilityState, ToolCallDetail, ToolCallId, ToolCallState, ToolCallSummary,
    ToolCallsResponse, ToolDefinition, ToolDefinitionState, ToolDefinitionsResponse,
    ToolExecutionKind, ToolId, ToolInvocationMode, ToolRedactionPolicy, ToolResultEnvelope,
    ToolRiskLevel, ToolScope, ToolSearchFilters, ToolSearchResult, ToolSourceId, ToolSourceKind,
    ToolingCommandPayloadV1, ToolingCommandV1, ToolingEventPayloadV1, ToolingEventV1, UpravaRef,
    VisualRenderScope, VisualRendererContributionV1, VisualRendererFallback, VisualRendererKind,
    WorkspaceCommandHistoryItem, WorkspaceCommandHistoryResponse, WorkspaceCommandIntent,
    WorkspaceCommandRunResponse, WorkspaceTerminalListResponse, WorkspaceTerminalOpenResponse,
    WorkspaceTerminalOutputFrame, WorkspaceTerminalState, WorkspaceTerminalStreamFrame,
    WorkspaceTerminalSummary, TOOLING_CONTRACT_VERSION_V1, UPRAVA_MCP_LEASE_AUDIENCE,
};

fn main() {
    let at = Utc
        .with_ymd_and_hms(2026, 7, 10, 12, 0, 0)
        .single()
        .expect("fixture timestamp is valid");
    let placement_id = ProjectPlacementId::from("placement-fixture");
    let terminal_id = TerminalId::from("terminal-fixture");
    let terminal = WorkspaceTerminalSummary {
        placement_id: placement_id.clone(),
        terminal_id: terminal_id.clone(),
        title: "Fixture shell".to_owned(),
        cwd: "/workspace".to_owned(),
        shell: "/bin/sh".to_owned(),
        cols: 120,
        rows: 40,
        state: WorkspaceTerminalState::Running,
        exit_code: None,
        created_at: at,
        updated_at: at,
    };
    let output = WorkspaceTerminalOutputFrame {
        terminal_id: terminal_id.clone(),
        seq: 1,
        data: "ready\n".to_owned(),
        sent_at: at,
    };

    let mut fixtures = Map::new();
    insert(
        &mut fixtures,
        "command_accepted",
        CommandAcceptedResponse {
            command_id: "command-fixture".into(),
            session: None,
        },
    );
    insert(
        &mut fixtures,
        "workspace_command_run",
        WorkspaceCommandRunResponse {
            placement_id: placement_id.clone(),
            terminal_command_id: "terminal-command-fixture".to_owned(),
            command: "make".to_owned(),
            args: vec!["l".to_owned()],
            intent: WorkspaceCommandIntent::Check,
            label: Some("Light checks".to_owned()),
            exit_code: Some(0),
            success: true,
            stdout: "ok\n".to_owned(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            duration_ms: 12,
            started_at: at,
            completed_at: at,
        },
    );
    insert(
        &mut fixtures,
        "workspace_terminal_open",
        WorkspaceTerminalOpenResponse {
            placement_id: placement_id.clone(),
            terminal: terminal.clone(),
            replay: vec![output.clone()],
        },
    );
    insert(
        &mut fixtures,
        "workspace_command_history",
        WorkspaceCommandHistoryResponse {
            placement_id: placement_id.clone(),
            commands: vec![WorkspaceCommandHistoryItem {
                command_id: "command-fixture".into(),
                kind: CommandKind::RunWorkspaceCommand,
                state: CommandState::Completed,
                created_at: at,
                completed_at: Some(at),
                payload: json!({ "command": "make", "args": ["l"] }).into(),
                result_payload: Some(json!({ "success": true }).into()),
            }],
            generated_at: at,
        },
    );
    insert(&mut fixtures, "workspace_terminal_output", output.clone());
    insert(
        &mut fixtures,
        "workspace_terminal_list",
        WorkspaceTerminalListResponse {
            placement_id: placement_id.clone(),
            terminals: vec![terminal],
            generated_at: at,
        },
    );
    insert(
        &mut fixtures,
        "workspace_terminal_stream",
        WorkspaceTerminalStreamFrame::Output {
            terminal_id,
            seq: output.seq,
            data: output.data,
            sent_at: at,
        },
    );
    insert(
        &mut fixtures,
        "event_envelope",
        EventEnvelope {
            event_id: EventId::from("event-fixture"),
            command_id: None,
            correlation_id: None,
            actor_ref: ActorRef::System,
            scope_ref: ScopeRef::Unknown {
                scope: "fixture".to_owned(),
            },
            node_id: None,
            runtime_session_id: None,
            session_thread_id: None,
            turn_id: None,
            seq: 1,
            session_projection_seq: None,
            kind: EventKind::RuntimeReady,
            happened_at: at,
            source_refs: vec![],
            evidence_refs: vec![],
            cause_refs: vec![],
            result_refs: vec![],
            payload: uprava_protocol::EventPayload::from_json(
                EventKind::RuntimeReady,
                json!({ "provider": "fixture" }),
            ),
        },
    );
    let managed_policy = uprava_protocol::EffectiveRuntimePolicy {
        contract_version: 1,
        execution_profile: AgentExecutionProfile::Managed,
        provider: "codex".to_owned(),
        provider_version: Some("0.144.1".to_owned()),
        provider_capabilities: ProviderRuntimeCapability::required_for_managed_codex().to_vec(),
        sandbox_mode: ProviderSandboxMode::WorkspaceWrite,
        approval_mode: ProviderApprovalMode::Untrusted,
        workspace_root: "/workspace".to_owned(),
        additional_writable_paths: Vec::new(),
        network_posture: RuntimeNetworkPosture::Unsupported,
        tool_exposure: RuntimeToolExposureSummary {
            server_count: 1,
            tool_count: 12,
            server_names: vec!["uprava".to_owned()],
        },
        credential_profile_ref: Some("codex:default".to_owned()),
        unsafe_override: None,
        capability_metadata: BTreeMap::from([("transport".to_owned(), "app-server-v2".to_owned())]),
    };
    let managed_policy_hash = managed_policy
        .policy_hash()
        .expect("managed fixture policy hashes");
    insert(
        &mut fixtures,
        "managed_policy_event",
        EventEnvelope {
            event_id: EventId::from("event-managed-policy-fixture"),
            command_id: Some("command-managed-start-fixture".into()),
            correlation_id: Some("correlation-managed-fixture".into()),
            actor_ref: ActorRef::Node {
                node_id: "node-fixture".into(),
            },
            scope_ref: ScopeRef::Runtime {
                runtime_session_id: "runtime-managed-fixture".into(),
            },
            node_id: Some("node-fixture".into()),
            runtime_session_id: Some("runtime-managed-fixture".into()),
            session_thread_id: Some("session-managed-fixture".into()),
            turn_id: None,
            seq: 1,
            session_projection_seq: Some(1),
            kind: EventKind::RuntimePolicyEffective,
            happened_at: at,
            source_refs: vec![],
            evidence_refs: vec![],
            cause_refs: vec![],
            result_refs: vec![],
            payload: uprava_protocol::EventPayload::from_json(
                EventKind::RuntimePolicyEffective,
                json!({
                    "policy": managed_policy,
                    "policy_hash": managed_policy_hash,
                }),
            ),
        },
    );
    let artifact_id = ArtifactId::from("artifact-fixture");
    insert(
        &mut fixtures,
        "artifact_detail",
        ArtifactDetail {
            artifact: ArtifactSummary {
                artifact_id: artifact_id.clone(),
                artifact_type: "uprava.diagram".to_owned(),
                title: "Fixture diagram".to_owned(),
                scope_ref: ScopeRef::Placement {
                    project_placement_id: placement_id.clone(),
                },
                owner_plugin_id: PluginId::from("uprava.diagrams"),
                current_version: 1,
                state: ArtifactState::Active,
                created_by: ActorRef::System,
                created_at: at,
                updated_at: at,
            },
            version: ArtifactVersion {
                artifact_id,
                version: 1,
                schema_version: 1,
                payload: json!({
                    "language": "mermaid",
                    "source": "flowchart LR\nCore --> Node",
                })
                .into(),
                fallback_text: "flowchart LR\nCore --> Node".to_owned(),
                source_version: Some("fixture:v1".to_owned()),
                source_refs: vec![UpravaRef::WorkspaceDiff {
                    diff_id: "diff-fixture".to_owned(),
                    placement_id: placement_id.clone(),
                }],
                evidence_refs: Vec::new(),
                cause_refs: Vec::new(),
                trace_refs: Vec::new(),
                provenance: json!({ "kind": "fixture" }).into(),
                created_at: at,
            },
        },
    );
    insert(
        &mut fixtures,
        "tooling_contract",
        tooling_contract_fixture(at, placement_id),
    );
    insert(
        &mut fixtures,
        "plugin_contract",
        plugin_contract_fixture(at),
    );

    println!(
        "{}",
        serde_json::to_string_pretty(&Value::Object(fixtures))
            .expect("web protocol fixtures serialize")
    );
}

#[derive(Serialize)]
struct PluginContractFixture {
    plugins: PluginListResponse,
    effective_snapshot: uprava_protocol::EffectivePluginSnapshot,
}

fn plugin_contract_fixture(at: chrono::DateTime<Utc>) -> PluginContractFixture {
    let theme_contribution = PluginContribution::UiTheme {
        contribution_id: "uprava.theme-dark.theme".to_owned(),
        contract_version: 1,
        contribution: ThemeContributionV1 {
            theme_id: "uprava.dark".to_owned(),
            label: "Dark".to_owned(),
            kind: ThemeKind::Dark,
            color_scheme: ThemeColorScheme::Dark,
            semantic_tokens: BTreeMap::from([
                ("content.primary".to_owned(), "#f1f2ec".to_owned()),
                ("content.muted".to_owned(), "#a9ada3".to_owned()),
                ("content.inverse".to_owned(), "#111310".to_owned()),
                ("surface.background".to_owned(), "#111310".to_owned()),
                ("surface.muted".to_owned(), "#191c18".to_owned()),
                ("surface.raised".to_owned(), "#20241f".to_owned()),
                ("border.default".to_owned(), "#3a4038".to_owned()),
                ("border.strong".to_owned(), "#d7dbd1".to_owned()),
                ("status.risk".to_owned(), "#ff777d".to_owned()),
                ("status.notice".to_owned(), "#b7a0ff".to_owned()),
                ("focus".to_owned(), "#f1f2ec".to_owned()),
                ("selection".to_owned(), "#355343".to_owned()),
                ("editor.background".to_owned(), "#151814".to_owned()),
                ("editor.foreground".to_owned(), "#e5e9df".to_owned()),
                ("terminal.background".to_owned(), "#10130f".to_owned()),
                ("terminal.foreground".to_owned(), "#dce8dd".to_owned()),
            ]),
            monaco: MonacoThemeV1 {
                base: "vs-dark".to_owned(),
                colors: BTreeMap::from([("editor.background".to_owned(), "#151814".to_owned())]),
            },
            terminal: TerminalThemeV1 {
                colors: BTreeMap::from([("background".to_owned(), "#10130f".to_owned())]),
            },
        },
    };
    let package = PluginPackageSummary {
        plugin_id: PluginId::from("uprava.theme-dark"),
        version: "1.1.0".to_owned(),
        manifest_hash: "sha256:plugin-fixture".to_owned(),
        manifest_version: 1,
        display_name: "Dark Theme".to_owned(),
        description: "Bundled data-only dark appearance for Uprava.".to_owned(),
        publisher: "Uprava".to_owned(),
        install_source: PluginInstallSource::Bundled,
        trust_level: PluginTrustLevel::DataOnly,
        requested_permissions: vec!["ui.theme.contribute".to_owned()],
        contributions: vec![theme_contribution.clone()],
        discovered_at: at,
    };
    let theme_installation = PluginInstallationSummary {
        package,
        desired_state: PluginDesiredState::Enabled,
        effective_state: PluginEffectiveState::Active,
        compatibility: PluginCompatibility {
            state: PluginCompatibilityState::Compatible,
            diagnostics: vec![],
        },
        configuration_revision: 0,
        granted_permissions: vec!["ui.theme.contribute".to_owned()],
        installed_at: at,
        updated_at: at,
        last_error_code: None,
    };
    let renderer_contribution = PluginContribution::VisualRenderer {
        contribution_id: "uprava.markdown.chat".to_owned(),
        contract_version: 1,
        contribution: VisualRendererContributionV1 {
            renderer_id: "uprava.markdown.chat".to_owned(),
            implementation_id: "uprava.markdown.v1".to_owned(),
            renderer_kind: VisualRendererKind::Content,
            accepted_source_kinds: vec!["chat.assistant_message".to_owned()],
            render_scopes: vec![VisualRenderScope::ContentEnhancement],
            allowed_surfaces: vec!["session.timeline".to_owned()],
            fallback_strategy: VisualRendererFallback::PlainText,
            source_matcher: None,
            visual_kinds: Vec::new(),
            actions: Vec::new(),
        },
    };
    let renderer_installation = PluginInstallationSummary {
        package: PluginPackageSummary {
            plugin_id: PluginId::from("uprava.markdown"),
            version: "1.1.0".to_owned(),
            manifest_hash: "sha256:markdown-plugin-fixture".to_owned(),
            manifest_version: 1,
            display_name: "Markdown Renderer".to_owned(),
            description: "Bundled hardened Markdown rendering for agent chat messages.".to_owned(),
            publisher: "Uprava".to_owned(),
            install_source: PluginInstallSource::Bundled,
            trust_level: PluginTrustLevel::TrustedBundled,
            requested_permissions: vec!["visual.renderer.contribute".to_owned()],
            contributions: vec![renderer_contribution.clone()],
            discovered_at: at,
        },
        desired_state: PluginDesiredState::Enabled,
        effective_state: PluginEffectiveState::Active,
        compatibility: PluginCompatibility {
            state: PluginCompatibilityState::Compatible,
            diagnostics: vec![],
        },
        configuration_revision: 0,
        granted_permissions: vec!["visual.renderer.contribute".to_owned()],
        installed_at: at,
        updated_at: at,
        last_error_code: None,
    };
    let plain_text_contribution = PluginContribution::VisualRenderer {
        contribution_id: "uprava.plain-text.chat".to_owned(),
        contract_version: 1,
        contribution: VisualRendererContributionV1 {
            renderer_id: "uprava.plain-text.chat".to_owned(),
            implementation_id: "uprava.plain-text.v1".to_owned(),
            renderer_kind: VisualRendererKind::Content,
            accepted_source_kinds: vec!["chat.assistant_message".to_owned()],
            render_scopes: vec![VisualRenderScope::ContentEnhancement],
            allowed_surfaces: vec!["session.timeline".to_owned()],
            fallback_strategy: VisualRendererFallback::PlainText,
            source_matcher: None,
            visual_kinds: Vec::new(),
            actions: Vec::new(),
        },
    };
    let plain_text_installation = PluginInstallationSummary {
        package: PluginPackageSummary {
            plugin_id: PluginId::from("uprava.plain-text"),
            version: "1.0.0".to_owned(),
            manifest_hash: "sha256:plain-text-plugin-fixture".to_owned(),
            manifest_version: 1,
            display_name: "Plain Text Renderer".to_owned(),
            description: "Bundled readable fallback renderer for agent chat messages.".to_owned(),
            publisher: "Uprava".to_owned(),
            install_source: PluginInstallSource::Bundled,
            trust_level: PluginTrustLevel::TrustedBundled,
            requested_permissions: vec!["visual.renderer.contribute".to_owned()],
            contributions: vec![plain_text_contribution.clone()],
            discovered_at: at,
        },
        desired_state: PluginDesiredState::Enabled,
        effective_state: PluginEffectiveState::Active,
        compatibility: PluginCompatibility {
            state: PluginCompatibilityState::Compatible,
            diagnostics: vec![],
        },
        configuration_revision: 0,
        granted_permissions: vec!["visual.renderer.contribute".to_owned()],
        installed_at: at,
        updated_at: at,
        last_error_code: None,
    };
    let theme_effective = EffectiveContribution {
        plugin_id: PluginId::from("uprava.theme-dark"),
        plugin_version: "1.1.0".to_owned(),
        contribution_id: "uprava.theme-dark.theme".to_owned(),
        extension_point: "ui.theme".to_owned(),
        contract_version: 1,
        target: ContributionTarget::UiTheme {
            theme_id: "uprava.dark".to_owned(),
        },
        effective_state: EffectiveContributionState::Available,
        contribution: theme_contribution,
    };
    let renderer_effective = EffectiveContribution {
        plugin_id: PluginId::from("uprava.markdown"),
        plugin_version: "1.1.0".to_owned(),
        contribution_id: "uprava.markdown.chat".to_owned(),
        extension_point: "visual.renderer".to_owned(),
        contract_version: 1,
        target: ContributionTarget::VisualRenderer {
            source_kind: "chat.assistant_message".to_owned(),
            surface: "session.timeline".to_owned(),
            render_scope: VisualRenderScope::ContentEnhancement,
            selector: None,
        },
        effective_state: EffectiveContributionState::Available,
        contribution: renderer_contribution,
    };
    let plain_text_effective = EffectiveContribution {
        plugin_id: PluginId::from("uprava.plain-text"),
        plugin_version: "1.0.0".to_owned(),
        contribution_id: "uprava.plain-text.chat".to_owned(),
        extension_point: "visual.renderer".to_owned(),
        contract_version: 1,
        target: renderer_effective.target.clone(),
        effective_state: EffectiveContributionState::Available,
        contribution: plain_text_contribution,
    };
    PluginContractFixture {
        plugins: PluginListResponse {
            items: vec![
                theme_installation,
                renderer_installation,
                plain_text_installation,
            ],
        },
        effective_snapshot: uprava_protocol::EffectivePluginSnapshot {
            contributions: vec![
                theme_effective,
                renderer_effective.clone(),
                plain_text_effective.clone(),
            ],
            resolutions: vec![ContributionTargetResolution {
                target_id: "renderer-target-fixture".to_owned(),
                extension_point: "visual.renderer".to_owned(),
                mode: ContributionResolutionMode::Exclusive,
                target: renderer_effective.target.clone(),
                revision: 0,
                conflict: true,
                contributions: vec![renderer_effective, plain_text_effective],
            }],
            generated_at: at,
        },
    }
}

#[derive(Serialize)]
struct ToolingContractFixture {
    tool_definition: ToolDefinition,
    availability: ToolAvailability,
    observed_capability: ObservedCapability,
    integration: IntegrationConnectionSummary,
    dependency: McpDependencyStatus,
    search_request: SearchToolsRequest,
    search_response: SearchToolsResponse,
    inspect_request: InspectToolRequest,
    inspect_response: InspectToolResponse,
    execute_request: ExecuteToolRequest,
    execute_response: ExecuteToolResponse,
    tool_call_detail: ToolCallDetail,
    node_command: ToolingCommandV1,
    node_event: ToolingEventV1,
    lease_claims: McpAccessLeaseClaims,
    tool_definitions: ToolDefinitionsResponse,
    tool_availability: ToolAvailabilityResponse,
    observed_capabilities: ObservedCapabilitiesResponse,
    integration_connections: IntegrationConnectionsResponse,
    dependency_statuses: McpDependencyStatusesResponse,
    tool_calls: ToolCallsResponse,
    integration_connect_request: IntegrationConnectRequest,
    integration_connect_response: IntegrationConnectResponse,
    integration_disconnect_request: IntegrationDisconnectRequest,
    integration_disconnect_response: IntegrationDisconnectResponse,
}

fn tooling_contract_fixture(
    at: chrono::DateTime<Utc>,
    placement_id: ProjectPlacementId,
) -> ToolingContractFixture {
    let tool_id = ToolId::from("uprava.session.inspect");
    let source_id = ToolSourceId::from("uprava-native");
    let input_schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": { "session_thread_id": { "type": "string" } },
        "required": ["session_thread_id"],
        "additionalProperties": false
    })
    .into();
    let schema_hash =
        compute_tool_schema_hash(&input_schema, None).expect("fixture tool schema hash computes");
    let definition = ToolDefinition {
        tool_id: tool_id.clone(),
        source_id: source_id.clone(),
        source_kind: ToolSourceKind::UpravaNative,
        source_tool_name: "session.inspect".to_owned(),
        version: 1,
        display_name: "Inspect session".to_owned(),
        short_description: "Return a bounded session summary.".to_owned(),
        documentation_url: None,
        input_schema,
        output_schema: None,
        schema_hash: schema_hash.clone(),
        risk_level: ToolRiskLevel::ReadOnly,
        required_permissions: vec!["session.read".to_owned()],
        execution_kind: ToolExecutionKind::CoreNative,
        approval_policy: PolicyDecision::Allow,
        redaction: ToolRedactionPolicy {
            argument_json_pointers: vec![],
            result_json_pointers: vec![],
            redact_all_arguments: false,
            redact_all_result: false,
            max_summary_bytes: 4096,
        },
        state: ToolDefinitionState::Active,
        created_at: at,
        updated_at: at,
    };
    let scope = ToolScope {
        actor_ref: ActorRef::local_user(),
        node_id: Some("node-fixture".into()),
        project_id: Some(ProjectId::from("project-fixture")),
        project_placement_id: Some(placement_id.clone()),
        session_thread_id: Some(SessionThreadId::from("session-fixture")),
    };
    let availability = ToolAvailability {
        tool_id: tool_id.clone(),
        scope: scope.clone(),
        state: ToolAvailabilityState::Available,
        reason: None,
        backend_ref: Some("core-native".to_owned()),
        dependency_instance_id: None,
        schema_hash: schema_hash.clone(),
        policy_version: "policy-fixture-v1".to_owned(),
        observed_at: at,
    };
    let integration_id = uprava_protocol::IntegrationId::from("integration-linear-fixture");
    let dependency_instance_id = McpDependencyInstanceId::from("dependency-linear-fixture");
    let integration = IntegrationConnectionSummary {
        integration_id: integration_id.clone(),
        source_id: ToolSourceId::from("linear-remote-mcp"),
        provider: "linear".to_owned(),
        display_name: "Linear".to_owned(),
        desired_state: IntegrationDesiredState::Enabled,
        auth_state: IntegrationAuthState::Connected,
        node_id: Some("node-fixture".into()),
        authenticated_actor_label: Some("fixture-workspace".to_owned()),
        connected_at: Some(at),
        updated_at: at,
        error_code: None,
    };
    let dependency = McpDependencyStatus {
        dependency_instance_id: dependency_instance_id.clone(),
        integration_id: integration_id.clone(),
        node_id: "node-fixture".into(),
        desired_state: IntegrationDesiredState::Enabled,
        actual_state: McpDependencyActualState::Running,
        runtime_name: "toolhive".to_owned(),
        runtime_version: Some("0.40.0".to_owned()),
        upstream_identity: Some("https://mcp.linear.app/mcp".to_owned()),
        schema_set_hash: Some("sha256:schema-set-fixture".to_owned()),
        error_code: None,
        observed_at: at,
    };
    let observed_capability = ObservedCapability {
        node_id: "node-fixture".into(),
        capability_key: "git".to_owned(),
        display_name: "Git".to_owned(),
        state: ObservedCapabilityState::Available,
        version: Some("2.50.1".to_owned()),
        safe_authentication_state: None,
        observed_at: at,
    };
    let search_result = ToolSearchResult {
        tool_id: tool_id.clone(),
        display_name: definition.display_name.clone(),
        short_description: definition.short_description.clone(),
        source_kind: definition.source_kind,
        risk_level: definition.risk_level,
        availability_state: availability.state,
        unavailable_reason: None,
        schema_hash: schema_hash.clone(),
    };
    let tool_call_id = ToolCallId::from("tool-call-fixture");
    let call_summary = ToolCallSummary {
        tool_call_id: tool_call_id.clone(),
        tool_id: tool_id.clone(),
        schema_hash: schema_hash.clone(),
        actor_ref: ActorRef::local_user(),
        scope: scope.clone(),
        source_kind: ToolSourceKind::UpravaNative,
        state: ToolCallState::Completed,
        policy_decision: PolicyDecision::Allow,
        route: "core_native".to_owned(),
        requested_at: at,
        started_at: Some(at),
        completed_at: Some(at),
        correlation_id: CorrelationId::from("correlation-tool-fixture"),
    };
    let result = ToolResultEnvelope {
        content: json!({ "session_thread_id": "session-fixture", "state": "active" }).into(),
        summary: Some("Session is active.".to_owned()),
        truncated: false,
        original_size_bytes: Some(64),
        artifact_refs: vec![],
    };

    ToolingContractFixture {
        tool_definition: definition.clone(),
        availability: availability.clone(),
        observed_capability: observed_capability.clone(),
        integration: integration.clone(),
        dependency: dependency.clone(),
        search_request: SearchToolsRequest {
            scope: scope.clone(),
            query: "inspect session".to_owned(),
            filters: ToolSearchFilters::default(),
            cursor: None,
            limit: Some(10),
        },
        search_response: SearchToolsResponse {
            items: vec![search_result],
            next_cursor: None,
        },
        inspect_request: InspectToolRequest {
            scope: scope.clone(),
            tool_id: tool_id.clone(),
        },
        inspect_response: InspectToolResponse {
            definition: definition.clone(),
            availability: availability.clone(),
            invocation_mode: ToolInvocationMode::StableExecuteTool,
        },
        execute_request: ExecuteToolRequest {
            scope: scope.clone(),
            tool_id: tool_id.clone(),
            arguments: json!({ "session_thread_id": "session-fixture" }).into(),
        },
        execute_response: ExecuteToolResponse {
            tool_call_id: tool_call_id.clone(),
            state: ToolCallState::Completed,
            result: Some(result.clone()),
            error: None,
        },
        tool_call_detail: ToolCallDetail {
            summary: call_summary.clone(),
            command_id: Some("command-tool-fixture".into()),
            integration_id: None,
            dependency_instance_id: None,
            policy_version: "policy-fixture-v1".to_owned(),
            redacted_arguments_summary: Some("session_thread_id=session-fixture".to_owned()),
            redacted_result_summary: Some("Session is active.".to_owned()),
            argument_hash: Some("sha256:arguments-fixture".to_owned()),
            result_hash: Some("sha256:result-fixture".to_owned()),
            result_size_bytes: Some(64),
            trace_refs: vec![],
            result_refs: vec![],
            error: None,
        },
        node_command: ToolingCommandV1 {
            contract_version: TOOLING_CONTRACT_VERSION_V1,
            payload: ToolingCommandPayloadV1::CancelToolCall {
                tool_call_id: tool_call_id.clone(),
                reason: Some("fixture".to_owned()),
            },
        },
        node_event: ToolingEventV1 {
            contract_version: TOOLING_CONTRACT_VERSION_V1,
            payload: ToolingEventPayloadV1::ToolAvailabilityChanged {
                availability: availability.clone(),
            },
        },
        lease_claims: McpAccessLeaseClaims {
            lease_id: McpAccessLeaseId::from("lease-fixture"),
            audience: UPRAVA_MCP_LEASE_AUDIENCE.to_owned(),
            actor_ref: ActorRef::local_user(),
            session_thread_id: "session-fixture".into(),
            project_id: Some("project-fixture".into()),
            project_placement_id: placement_id,
            node_id: "node-fixture".into(),
            issued_at: at,
            expires_at: at + chrono::Duration::minutes(10),
            credential_version: 1,
            resume_command_id: None,
        },
        tool_definitions: ToolDefinitionsResponse {
            items: vec![definition],
            next_cursor: None,
        },
        tool_availability: ToolAvailabilityResponse {
            items: vec![availability],
            generated_at: at,
        },
        observed_capabilities: ObservedCapabilitiesResponse {
            items: vec![observed_capability],
            generated_at: at,
        },
        integration_connections: IntegrationConnectionsResponse {
            items: vec![integration.clone()],
        },
        dependency_statuses: McpDependencyStatusesResponse {
            items: vec![dependency],
            generated_at: at,
        },
        tool_calls: ToolCallsResponse {
            items: vec![call_summary],
            next_cursor: None,
        },
        integration_connect_request: IntegrationConnectRequest {
            integration_id: integration_id.clone(),
            project_id: Some("project-fixture".into()),
            node_id: "node-fixture".into(),
        },
        integration_connect_response: IntegrationConnectResponse {
            connection: integration.clone(),
            authorization_url: "https://core.example.test/oauth/linear".to_owned(),
            expires_at: at + chrono::Duration::minutes(5),
        },
        integration_disconnect_request: IntegrationDisconnectRequest {
            revoke_remote: true,
        },
        integration_disconnect_response: IntegrationDisconnectResponse {
            connection: integration,
            remote_revocation_confirmed: true,
        },
    }
}

fn insert<T: serde::Serialize>(fixtures: &mut Map<String, Value>, name: &str, fixture: T) {
    fixtures.insert(
        name.to_owned(),
        serde_json::to_value(fixture).expect("fixture serializes"),
    );
}
