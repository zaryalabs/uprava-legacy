//! Shared Uprava protocol and domain contracts for the V01 control plane.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Wire protocol version advertised by Core and Node control frames.
pub const CURRENT_PROTOCOL_VERSION: &str = "v2";
/// Versions accepted during a rolling upgrade. Keep this list explicit so a
/// peer cannot silently downgrade to an unknown shape.
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &[CURRENT_PROTOCOL_VERSION];

#[must_use]
pub fn is_supported_protocol_version(version: &str) -> bool {
    SUPPORTED_PROTOCOL_VERSIONS.contains(&version)
}

pub mod api;
pub mod artifacts;
pub mod control;
pub mod domain;
pub mod dynamic_ui;
pub mod managed_runtime;
pub mod plugins;
pub mod projection;
pub mod reference;
pub mod serde_json_value;
pub mod task;
pub mod tooling;
pub mod workspace;

pub use api::*;
pub use artifacts::*;
pub use control::*;
pub use domain::*;
pub use dynamic_ui::*;
pub use managed_runtime::*;
pub use plugins::*;
pub use projection::*;
pub use reference::*;
pub use serde_json_value::JsonValue;
pub use task::*;
pub use tooling::*;
pub use workspace::*;

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    #[test]
    fn protocol_v2_is_the_only_supported_breaking_release_version() {
        assert_eq!(CURRENT_PROTOCOL_VERSION, "v2");
        assert!(is_supported_protocol_version("v2"));
        assert!(!is_supported_protocol_version("v1"));
    }

    #[test]
    fn action_capability_uses_the_authoritative_web_wire_literal() {
        let encoded = serde_json::to_string(&ActionCapability::SessionSendTurn)
            .expect("action capability serializes");

        assert_eq!(encoded, "\"session.sendTurn\"");
    }

    #[test]
    fn known_command_payload_cannot_match_a_different_command_kind() {
        let payload = CommandPayload::SendTurn {
            content: "hello".to_owned(),
            turn_id: TurnId::from("turn-1"),
            collaboration_mode: None,
        };

        assert!(payload.matches_kind(CommandKind::SendTurn));
        assert!(!payload.matches_kind(CommandKind::ResolveApproval));
        assert!(!CommandPayload::Extension {
            name: "vendor.example".to_owned(),
            value: json_payload(),
        }
        .matches_kind(CommandKind::SendTurn));
    }

    #[test]
    fn known_event_payload_cannot_match_a_different_event_kind() {
        let payload = EventPayload::from_json(
            EventKind::RuntimeError,
            serde_json::json!({ "code": "failed", "message": "failure" }),
        );

        assert!(payload.matches_kind(EventKind::RuntimeError));
        assert!(!payload.matches_kind(EventKind::RuntimeReady));
    }

    fn json_payload() -> serde_json_value::JsonValue {
        serde_json_value::JsonValue(serde_json::json!({ "sample": true }))
    }

    #[test]
    fn command_envelope_round_trips_through_json() {
        let command = CommandEnvelope {
            command_id: CommandId::from("command-1"),
            kind: CommandKind::SendTurn,
            target: CommandTarget::SessionRuntime {
                node_id: NodeId::from("node-1"),
                project_placement_id: ProjectPlacementId::from("placement-1"),
                session_thread_id: SessionThreadId::from("session-1"),
                runtime_session_id: RuntimeSessionId::from("runtime-1"),
            },
            actor_ref: ActorRef::local_user(),
            source_refs: vec![],
            cause_refs: vec![UpravaRef::Session {
                session_thread_id: SessionThreadId::from("session-1"),
            }],
            issued_at: Utc::now(),
            correlation_id: CorrelationId::from("corr-1"),
            payload: CommandPayload::SendTurn {
                content: "fixture".to_owned(),
                turn_id: TurnId::from("turn-1"),
                collaboration_mode: None,
            },
        };

        let encoded = serde_json::to_string(&command).expect("command serializes");
        let decoded: CommandEnvelope =
            serde_json::from_str(&encoded).expect("command deserializes");
        let wire: serde_json::Value =
            serde_json::from_str(&encoded).expect("command wire json parses");

        assert_eq!(decoded.kind, CommandKind::SendTurn);
        assert_eq!(wire["target"]["kind"], "session_runtime");
        assert!(wire.get("target_node_id").is_none());
        assert!(wire.get("session_thread_id").is_none());
    }

    #[test]
    fn event_envelope_round_trips_through_json() {
        let event = EventEnvelope {
            event_id: EventId::from("event-1"),
            command_id: Some(CommandId::from("command-1")),
            correlation_id: Some(CorrelationId::from("corr-1")),
            actor_ref: ActorRef::Provider {
                provider: "codex".to_owned(),
            },
            scope_ref: ScopeRef::Runtime {
                runtime_session_id: RuntimeSessionId::from("runtime-1"),
            },
            node_id: Some(NodeId::from("node-1")),
            runtime_session_id: Some(RuntimeSessionId::from("runtime-1")),
            session_thread_id: Some(SessionThreadId::from("session-1")),
            turn_id: Some(TurnId::from("turn-1")),
            seq: 1,
            session_projection_seq: Some(1),
            kind: EventKind::ProviderMessageCompleted,
            happened_at: Utc::now(),
            source_refs: vec![],
            evidence_refs: vec![],
            cause_refs: vec![],
            result_refs: vec![],
            payload: EventPayload::from_json(
                EventKind::ProviderMessageCompleted,
                serde_json::json!({ "content": "fixture" }),
            ),
        };

        let encoded = serde_json::to_string(&event).expect("event serializes");
        let decoded: EventEnvelope = serde_json::from_str(&encoded).expect("event deserializes");

        assert_eq!(decoded.seq, 1);
        assert_eq!(decoded.correlation_id, Some(CorrelationId::from("corr-1")));
    }

    #[test]
    fn event_envelope_defaults_missing_correlation_id() {
        let encoded = serde_json::json!({
            "event_id": "event-1",
            "command_id": "command-1",
            "actor_ref": { "kind": "system" },
            "scope_ref": { "kind": "unknown", "scope": "test" },
            "node_id": null,
            "runtime_session_id": null,
            "session_thread_id": null,
            "turn_id": null,
            "seq": 1,
            "kind": "runtime.ready",
            "happened_at": Utc::now(),
            "payload": { "type": "runtime_ready", "provider": "fixture" }
        });

        let decoded: EventEnvelope =
            serde_json::from_value(encoded).expect("legacy event deserializes");

        assert_eq!(decoded.correlation_id, None);
    }

    #[test]
    fn event_envelope_preserves_actor_scope_and_causality_refs() {
        let event = EventEnvelope {
            event_id: EventId::from("event-causality-1"),
            command_id: Some(CommandId::from("command-1")),
            correlation_id: Some(CorrelationId::from("corr-2")),
            actor_ref: ActorRef::Node {
                node_id: NodeId::from("node-1"),
            },
            scope_ref: ScopeRef::Runtime {
                runtime_session_id: RuntimeSessionId::from("runtime-1"),
            },
            node_id: Some(NodeId::from("node-1")),
            runtime_session_id: Some(RuntimeSessionId::from("runtime-1")),
            session_thread_id: Some(SessionThreadId::from("session-1")),
            turn_id: Some(TurnId::from("turn-1")),
            seq: 2,
            session_projection_seq: Some(2),
            kind: EventKind::ProviderOutputDelta,
            happened_at: Utc::now(),
            source_refs: vec![UpravaRef::Command {
                command_id: CommandId::from("command-1"),
            }],
            evidence_refs: vec![UpravaRef::FileRange {
                placement_id: ProjectPlacementId::from("placement-1"),
                path: "src/main.rs".to_owned(),
                range: TextRange {
                    start_line: Some(10),
                    end_line: Some(12),
                    start_offset: None,
                    end_offset: None,
                },
                version: Some("git:abc123".to_owned()),
            }],
            cause_refs: vec![UpravaRef::Approval {
                approval_id: ApprovalId::from("approval-1"),
            }],
            result_refs: vec![UpravaRef::Message {
                message_id: MessageId::from("message-1"),
            }],
            payload: EventPayload::from_json(
                EventKind::ProviderOutputDelta,
                serde_json::json!({ "content": "fixture" }),
            ),
        };

        let encoded = serde_json::to_string(&event).expect("event serializes");
        let decoded: EventEnvelope = serde_json::from_str(&encoded).expect("event deserializes");

        assert_eq!(decoded.actor_ref, event.actor_ref);
        assert_eq!(decoded.scope_ref, event.scope_ref);
        assert_eq!(decoded.source_refs, event.source_refs);
        assert_eq!(decoded.evidence_refs, event.evidence_refs);
        assert_eq!(decoded.cause_refs, event.cause_refs);
        assert_eq!(decoded.result_refs, event.result_refs);
    }

    #[test]
    fn control_frame_round_trips_through_json() {
        let frame = ControlFrame::Ping {
            frame_id: "frame-1".to_owned(),
            protocol_version: CURRENT_PROTOCOL_VERSION.to_owned(),
            sent_at: Utc::now(),
        };

        let encoded = serde_json::to_string(&frame).expect("frame serializes");
        let decoded: ControlFrame = serde_json::from_str(&encoded).expect("frame deserializes");

        assert!(matches!(decoded, ControlFrame::Ping { .. }));
    }

    #[test]
    fn error_envelope_round_trips_through_json() {
        let error = ApiError {
            error_code: "validation.invalid".to_owned(),
            message: "Invalid request".to_owned(),
            details: json_payload(),
            retryable: false,
            correlation_id: CorrelationId::from("corr-1"),
        };

        let encoded = serde_json::to_string(&error).expect("error serializes");
        let decoded: ApiError = serde_json::from_str(&encoded).expect("error deserializes");

        assert_eq!(decoded.error_code, "validation.invalid");
    }

    #[test]
    fn uprava_ref_variants_round_trip_through_json() {
        let refs = vec![
            UpravaRef::Node {
                node_id: NodeId::from("node-1"),
            },
            UpravaRef::Project {
                project_id: ProjectId::from("project-1"),
            },
            UpravaRef::Placement {
                placement_id: ProjectPlacementId::from("placement-1"),
            },
            UpravaRef::Workspace {
                placement_id: ProjectPlacementId::from("placement-1"),
            },
            UpravaRef::Session {
                session_thread_id: SessionThreadId::from("session-1"),
            },
            UpravaRef::Runtime {
                runtime_session_id: RuntimeSessionId::from("runtime-1"),
            },
            UpravaRef::Turn {
                turn_id: TurnId::from("turn-1"),
            },
            UpravaRef::Message {
                message_id: MessageId::from("message-1"),
            },
            UpravaRef::Block {
                block_id: BlockId::from("block-1"),
            },
            UpravaRef::Artifact {
                artifact_id: ArtifactId::from("artifact-1"),
            },
            UpravaRef::Event {
                event_id: EventId::from("event-1"),
                scope_ref: Box::new(ScopeRef::Session {
                    session_thread_id: SessionThreadId::from("session-1"),
                }),
                seq: 1,
            },
            UpravaRef::Command {
                command_id: CommandId::from("command-1"),
            },
            UpravaRef::Approval {
                approval_id: ApprovalId::from("approval-1"),
            },
            UpravaRef::Warning {
                warning_kind: "node_offline".to_owned(),
                command_id: Some(CommandId::from("command-1")),
            },
            UpravaRef::ToolCall {
                tool_call_id: "tool-call-1".to_owned(),
            },
            UpravaRef::File {
                placement_id: ProjectPlacementId::from("placement-1"),
                path: "src/main.rs".to_owned(),
                version: Some("git:abc123".to_owned()),
            },
            UpravaRef::FileRange {
                placement_id: ProjectPlacementId::from("placement-1"),
                path: "src/main.rs".to_owned(),
                range: TextRange {
                    start_line: Some(1),
                    end_line: Some(3),
                    start_offset: None,
                    end_offset: None,
                },
                version: None,
            },
            UpravaRef::Terminal {
                terminal_id: "terminal-1".to_owned(),
                placement_id: ProjectPlacementId::from("placement-1"),
            },
            UpravaRef::TerminalCommand {
                terminal_command_id: "terminal-command-1".to_owned(),
                terminal_id: Some("terminal-1".to_owned()),
            },
            UpravaRef::TerminalOutputRange {
                terminal_command_id: "terminal-command-1".to_owned(),
                range: TextRange {
                    start_line: Some(5),
                    end_line: Some(7),
                    start_offset: None,
                    end_offset: None,
                },
            },
            UpravaRef::DiffHunk {
                diff_id: "diff-1".to_owned(),
                hunk_id: "hunk-1".to_owned(),
            },
            UpravaRef::CheckResult {
                check_run_id: "check-1".to_owned(),
                failure_id: Some("failure-1".to_owned()),
            },
            UpravaRef::WorkspaceEdit {
                edit_id: "edit-1".to_owned(),
                placement_id: Some(ProjectPlacementId::from("placement-1")),
                path: Some("src/main.rs".to_owned()),
            },
            UpravaRef::TraceEvent {
                trace_event_id: "trace-event-1".to_owned(),
            },
            UpravaRef::ExternalEntity {
                integration_kind: "github".to_owned(),
                external_id: "pull-1".to_owned(),
            },
            UpravaRef::Unknown {
                ref_type: "future.ref".to_owned(),
                locator: json_payload(),
            },
        ];

        let encoded = serde_json::to_string(&refs).expect("refs serialize");
        let decoded: Vec<UpravaRef> = serde_json::from_str(&encoded).expect("refs deserialize");
        let kinds = serde_json::to_value(&decoded)
            .expect("refs convert to JSON value")
            .as_array()
            .expect("refs encode as array")
            .iter()
            .map(|value| {
                value
                    .get("kind")
                    .and_then(serde_json::Value::as_str)
                    .expect("ref kind is encoded")
                    .to_owned()
            })
            .collect::<Vec<_>>();

        assert_eq!(decoded, refs);
        assert_eq!(
            kinds,
            vec![
                "node",
                "project",
                "placement",
                "workspace",
                "session",
                "runtime",
                "turn",
                "message",
                "block",
                "artifact",
                "event",
                "command",
                "approval",
                "warning",
                "tool_call",
                "file",
                "file_range",
                "terminal",
                "terminal_command",
                "terminal_output_range",
                "diff_hunk",
                "check_result",
                "workspace_edit",
                "trace_event",
                "external_entity",
                "unknown",
            ]
        );
    }
}
