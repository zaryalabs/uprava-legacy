#!/usr/bin/env python3
"""Check tracked Rust/Web protocol literal drift.

This intentionally avoids network/codegen dependencies. It verifies that
Web-facing TypeScript literal unions still match the Rust serde wire literals
for protocol enums that are manually represented in the web client.
"""

from __future__ import annotations

import os
import re
import shlex
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RUST_PROTOCOL_ROOT = ROOT / "crates/uprava-protocol/src"
RUST_PROTOCOL = RUST_PROTOCOL_ROOT / "lib.rs"
WEB_TYPES = ROOT / "apps/web/src/shared/protocol/types.ts"
WEB_LITERALS = ROOT / "apps/web/src/shared/protocol/literals.ts"
WEB_FIXTURES = ROOT / "apps/web/src/shared/protocol/fixtures.json"


ENUM_SPECS: list[tuple[str, str, str, str | None, str]] = [
    (
        "AgentExecutionProfile",
        "AgentExecutionProfile",
        "snake_case",
        None,
        "AGENT_EXECUTION_PROFILE_VALUES",
    ),
    (
        "RuntimeAttemptState",
        "RuntimeAttemptState",
        "snake_case",
        None,
        "RUNTIME_ATTEMPT_STATE_VALUES",
    ),
    (
        "ProviderRuntimeCapability",
        "ProviderRuntimeCapability",
        "snake_case",
        None,
        "PROVIDER_RUNTIME_CAPABILITY_VALUES",
    ),
    (
        "ProviderInteractionKind",
        "ProviderInteractionKind",
        "snake_case",
        None,
        "PROVIDER_INTERACTION_KIND_VALUES",
    ),
    (
        "ProviderInteractionState",
        "ProviderInteractionState",
        "snake_case",
        None,
        "PROVIDER_INTERACTION_STATE_VALUES",
    ),
    (
        "RuntimeRecoveryStatus",
        "RuntimeRecoveryStatus",
        "snake_case",
        None,
        "RUNTIME_RECOVERY_STATUS_VALUES",
    ),
    (
        "ActionCapability",
        "ActionCapability",
        "snake_case",
        None,
        "ACTION_CAPABILITY_VALUES",
    ),
    (
        "DeploymentProfile",
        "DeploymentProfile",
        "snake_case",
        None,
        "DEPLOYMENT_PROFILE_VALUES",
    ),
    ("NodePresence", "NodePresence", "snake_case", None, "NODE_PRESENCE_VALUES"),
    (
        "RuntimeSessionState",
        "RuntimeSessionState",
        "snake_case",
        None,
        "RUNTIME_SESSION_STATE_VALUES",
    ),
    (
        "SessionThreadState",
        "SessionThreadState",
        "snake_case",
        None,
        "SESSION_THREAD_STATE_VALUES",
    ),
    ("PlacementState", "PlacementState", "snake_case", None, "PLACEMENT_STATE_VALUES"),
    (
        "WarningSeverity",
        "WarningSeverity",
        "snake_case",
        None,
        "WARNING_SEVERITY_VALUES",
    ),
    ("ClientLogLevel", "ClientLogLevel", "snake_case", None, "CLIENT_LOG_LEVEL_VALUES"),
    ("CommandState", "CommandState", "snake_case", None, "COMMAND_STATE_VALUES"),
    ("CommandKind", "CommandKind", "PascalCase", None, "COMMAND_KIND_VALUES"),
    ("EventKind", "EventKind", "snake_case", None, "EVENT_KIND_VALUES"),
    ("MessageRole", "MessageRole", "snake_case", None, "MESSAGE_ROLE_VALUES"),
    (
        "ScheduledMessageState",
        "ScheduledMessageState",
        "snake_case",
        None,
        "SCHEDULED_MESSAGE_STATE_VALUES",
    ),
    (
        "WorkspaceEntryKind",
        "WorkspaceEntryKind",
        "snake_case",
        None,
        "WORKSPACE_ENTRY_KIND_VALUES",
    ),
    (
        "WorkspaceEntryStatus",
        "WorkspaceEntryStatus",
        "snake_case",
        None,
        "WORKSPACE_ENTRY_STATUS_VALUES",
    ),
    (
        "WorkspaceEntryClassification",
        "WorkspaceEntryClassification",
        "snake_case",
        None,
        "WORKSPACE_ENTRY_CLASSIFICATION_VALUES",
    ),
    (
        "WorkspaceCommandIntent",
        "WorkspaceCommandIntent",
        "snake_case",
        None,
        "WORKSPACE_COMMAND_INTENT_VALUES",
    ),
    (
        "WorkspaceTerminalState",
        "WorkspaceTerminalState",
        "snake_case",
        None,
        "WORKSPACE_TERMINAL_STATE_VALUES",
    ),
    (
        "WorkspaceTerminalClientFrame",
        "WorkspaceTerminalClientFrame",
        "snake_case",
        "kind",
        "WORKSPACE_TERMINAL_CLIENT_FRAME_KIND_VALUES",
    ),
    (
        "WorkspaceTerminalStreamFrame",
        "WorkspaceTerminalStreamFrame",
        "snake_case",
        "kind",
        "WORKSPACE_TERMINAL_STREAM_FRAME_KIND_VALUES",
    ),
    ("ToolSourceKind", "ToolSourceKind", "snake_case", None, "TOOL_SOURCE_KIND_VALUES"),
    (
        "ToolExecutionKind",
        "ToolExecutionKind",
        "snake_case",
        None,
        "TOOL_EXECUTION_KIND_VALUES",
    ),
    ("ToolRiskLevel", "ToolRiskLevel", "snake_case", None, "TOOL_RISK_LEVEL_VALUES"),
    (
        "ToolDefinitionState",
        "ToolDefinitionState",
        "snake_case",
        None,
        "TOOL_DEFINITION_STATE_VALUES",
    ),
    (
        "ToolAvailabilityState",
        "ToolAvailabilityState",
        "snake_case",
        None,
        "TOOL_AVAILABILITY_STATE_VALUES",
    ),
    (
        "ToolUnavailableReason",
        "ToolUnavailableReason",
        "snake_case",
        None,
        "TOOL_UNAVAILABLE_REASON_VALUES",
    ),
    (
        "ObservedCapabilityState",
        "ObservedCapabilityState",
        "snake_case",
        None,
        "OBSERVED_CAPABILITY_STATE_VALUES",
    ),
    (
        "IntegrationDesiredState",
        "IntegrationDesiredState",
        "snake_case",
        None,
        "INTEGRATION_DESIRED_STATE_VALUES",
    ),
    (
        "IntegrationAuthState",
        "IntegrationAuthState",
        "snake_case",
        None,
        "INTEGRATION_AUTH_STATE_VALUES",
    ),
    (
        "McpDependencyActualState",
        "McpDependencyActualState",
        "snake_case",
        None,
        "MCP_DEPENDENCY_ACTUAL_STATE_VALUES",
    ),
    ("PolicyDecision", "PolicyDecision", "snake_case", None, "POLICY_DECISION_VALUES"),
    ("ToolCallState", "ToolCallState", "snake_case", None, "TOOL_CALL_STATE_VALUES"),
    (
        "ToolInvocationMode",
        "ToolInvocationMode",
        "snake_case",
        None,
        "TOOL_INVOCATION_MODE_VALUES",
    ),
    (
        "ToolExecutionErrorCode",
        "ToolExecutionErrorCode",
        "snake_case",
        None,
        "TOOL_EXECUTION_ERROR_CODE_VALUES",
    ),
]


def main() -> int:
    rust_source = read_rust_protocol_source()
    web_source = WEB_TYPES.read_text()
    literal_source = WEB_LITERALS.read_text()
    rust_by_constant: dict[str, list[str]] = {}
    failures: list[str] = []
    for rust_enum, ts_type, rename_all, discriminator, constant_name in ENUM_SPECS:
        rust_values = rust_enum_wire_values(rust_source, rust_enum, rename_all)
        rust_by_constant[constant_name] = rust_values
        ts_values = ts_type_literals(
            web_source,
            literal_source,
            ts_type,
            discriminator,
            constant_name,
        )
        if rust_values != ts_values:
            failures.append(format_failure(rust_enum, ts_type, rust_values, ts_values))

        literal_values = ts_const_literals(literal_source, constant_name)
        if rust_values != literal_values:
            failures.append(
                format_failure(
                    rust_enum,
                    f"{WEB_LITERALS.relative_to(ROOT)}:{constant_name}",
                    rust_values,
                    literal_values,
                )
            )

    expected_literals = render_web_literals(rust_by_constant)
    expected_fixtures = render_web_fixtures()
    if len(sys.argv) == 2 and sys.argv[1] == "--write":
        WEB_LITERALS.write_text(expected_literals)
        WEB_FIXTURES.write_text(expected_fixtures)
        print(f"Wrote {WEB_LITERALS.relative_to(ROOT)}")
        print(f"Wrote {WEB_FIXTURES.relative_to(ROOT)}")
        return 0
    if literal_source != expected_literals:
        failures.append(
            "Generated Web protocol literals are stale. "
            "Run `python3 scripts/protocol_check.py --write` and commit the result."
        )
    if not WEB_FIXTURES.exists() or WEB_FIXTURES.read_text() != expected_fixtures:
        failures.append(
            "Generated Web protocol fixtures are stale. "
            "Run `python3 scripts/protocol_check.py --write` and commit the result."
        )

    if failures:
        print("Protocol drift detected:\n", file=sys.stderr)
        print("\n\n".join(failures), file=sys.stderr)
        return 1

    print(
        f"Protocol drift check passed for {len(ENUM_SPECS)} Web-facing enums "
        "and canonical Rust fixtures"
    )
    return 0


def render_web_fixtures() -> str:
    cargo = shlex.split(os.environ.get("CARGO", "cargo"))
    result = subprocess.run(
        [*cargo, "run", "-q", "-p", "uprava-protocol", "--example", "web_fixtures"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout


def read_rust_protocol_source() -> str:
    return "\n".join(
        path.read_text()
        for path in sorted(RUST_PROTOCOL_ROOT.glob("*.rs"))
    )


def rust_enum_wire_values(source: str, enum_name: str, rename_all: str) -> list[str]:
    body = extract_rust_enum_body(source, enum_name)
    values: list[str] = []
    explicit_rename: str | None = None
    for raw_line in body.splitlines():
        line = raw_line.split("//", 1)[0].strip()
        if not line:
            continue
        rename_match = re.search(r'#\[serde\(rename\s*=\s*"([^"]+)"\)\]', line)
        if rename_match:
            explicit_rename = rename_match.group(1)
            continue
        if line.startswith("#["):
            continue
        variant_match = re.match(r"([A-Z][A-Za-z0-9_]*)\b", line)
        if not variant_match:
            continue
        variant = variant_match.group(1)
        values.append(explicit_rename or rename_variant(variant, rename_all))
        explicit_rename = None
    return values


def extract_rust_enum_body(source: str, enum_name: str) -> str:
    match = re.search(rf"pub enum {re.escape(enum_name)}\s*\{{", source)
    if not match:
        raise SystemExit(f"Rust enum {enum_name} not found in {RUST_PROTOCOL}")
    start = match.end()
    depth = 1
    index = start
    while index < len(source):
        character = source[index]
        if character == "{":
            depth += 1
        elif character == "}":
            depth -= 1
            if depth == 0:
                return source[start:index]
        index += 1
    raise SystemExit(f"Rust enum {enum_name} body is unterminated")


def ts_type_literals(
    source: str,
    literal_source: str,
    type_name: str,
    discriminator: str | None,
    constant_name: str,
) -> list[str]:
    match = re.search(rf"export type {re.escape(type_name)}\s*=", source)
    if not match:
        raise SystemExit(f"TypeScript type {type_name} not found in {WEB_TYPES}")
    body = extract_ts_type_body(source, match.end(), type_name)
    if discriminator is not None:
        return re.findall(rf"\b{re.escape(discriminator)}\s*:\s*\"([^\"]+)\"", body)
    literals = re.findall(r'"([^"]+)"', body)
    if literals:
        return literals
    if re.search(rf"\btypeof\s+{re.escape(constant_name)}\)\[number\]", body):
        return ts_const_literals(literal_source, constant_name)
    raise SystemExit(
        f"TypeScript type {type_name} does not expose literals or {constant_name}"
    )


def extract_ts_type_body(source: str, start: int, type_name: str) -> str:
    depth = 0
    in_string = False
    escaped = False
    index = start
    while index < len(source):
        character = source[index]
        if in_string:
            if escaped:
                escaped = False
            elif character == "\\":
                escaped = True
            elif character == '"':
                in_string = False
        else:
            if character == '"':
                in_string = True
            elif character in "{[(":
                depth += 1
            elif character in "}])":
                depth -= 1
            elif character == ";" and depth == 0:
                return source[start:index]
        index += 1
    raise SystemExit(f"TypeScript type {type_name} is unterminated")


def ts_const_literals(source: str, constant_name: str) -> list[str]:
    match = re.search(rf"export const {re.escape(constant_name)}\s*=\s*\[", source)
    if not match:
        raise SystemExit(f"TypeScript constant {constant_name} not found in {WEB_LITERALS}")
    body = extract_ts_array_body(source, match.end(), constant_name)
    return re.findall(r'"([^"]+)"', body)


def extract_ts_array_body(source: str, start: int, constant_name: str) -> str:
    depth = 1
    in_string = False
    escaped = False
    index = start
    while index < len(source):
        character = source[index]
        if in_string:
            if escaped:
                escaped = False
            elif character == "\\":
                escaped = True
            elif character == '"':
                in_string = False
        else:
            if character == '"':
                in_string = True
            elif character == "[":
                depth += 1
            elif character == "]":
                depth -= 1
                if depth == 0:
                    return source[start:index]
        index += 1
    raise SystemExit(f"TypeScript constant {constant_name} array is unterminated")


def rename_variant(variant: str, rename_all: str) -> str:
    if rename_all == "PascalCase":
        return variant
    if rename_all == "snake_case":
        return pascal_to_snake(variant)
    raise SystemExit(f"Unsupported serde rename_all style: {rename_all}")


def pascal_to_snake(value: str) -> str:
    first_pass = re.sub("(.)([A-Z][a-z]+)", r"\1_\2", value)
    return re.sub("([a-z0-9])([A-Z])", r"\1_\2", first_pass).lower()


def format_failure(
    rust_enum: str,
    ts_type: str,
    rust_values: list[str],
    ts_values: list[str],
) -> str:
    rust_set = set(rust_values)
    ts_set = set(ts_values)
    missing = [value for value in rust_values if value not in ts_set]
    extra = [value for value in ts_values if value not in rust_set]
    details = [
        f"- Rust enum: {rust_enum}",
        f"- TypeScript type: {ts_type}",
        f"- Rust values: {rust_values}",
        f"- TypeScript values: {ts_values}",
    ]
    if missing:
        details.append(f"- Missing in TypeScript: {missing}")
    if extra:
        details.append(f"- Extra in TypeScript: {extra}")
    if not missing and not extra:
        details.append("- Same values but different order")
    return "\n".join(details)


def render_web_literals(values_by_constant: dict[str, list[str]]) -> str:
    sections = [
        "// @generated by scripts/protocol_check.py --write",
        "// Rust uprava-protocol enums are the source of truth for these wire literals.",
        "",
    ]
    for _rust_enum, _ts_type, _rename_all, _discriminator, constant_name in ENUM_SPECS:
        sections.append(render_ts_const_array(constant_name, values_by_constant[constant_name]))
    return "\n".join(sections).rstrip() + "\n"


def render_ts_const_array(name: str, values: list[str]) -> str:
    inline = f"export const {name} = [{render_ts_strings(values)}] as const;"
    if len(values) <= 2 and len(inline) <= 80:
        return f"{inline}\n"
    lines = [f"export const {name} = ["]
    lines.extend(f'  "{value}",' for value in values)
    lines.append("] as const;\n")
    return "\n".join(lines)


def render_ts_strings(values: list[str]) -> str:
    return ", ".join(f'"{value}"' for value in values)


if __name__ == "__main__":
    raise SystemExit(main())
