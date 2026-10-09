import { describe, expect, it } from "vitest";

import type {
  RuntimeSummary,
  SessionSummary,
} from "../../shared/protocol/types";
import { lifecycleControlStates } from "./LifecycleControls";

describe("lifecycleControlStates", () => {
  it("enables lifecycle controls from session and runtime state", () => {
    expect(
      enabledLabels(session("active", runtime("running", "managed")), [
        "session.detach",
        "runtime.interrupt",
        "runtime.stop",
      ]),
    ).toEqual(["Detach", "Interrupt", "Stop"]);
    expect(
      enabledLabels(session("detached", runtime("ready")), [
        "session.attach",
        "runtime.stop",
      ]),
    ).toEqual(["Attach", "Stop"]);
    expect(
      enabledLabels(session("active", runtime("error")), [
        "session.detach",
        "runtime.stop",
        "runtime.resume",
      ]),
    ).toEqual(["Detach", "Stop", "Resume"]);
    expect(
      enabledLabels(session("active", runtime("expired")), [
        "session.detach",
        "runtime.resume",
      ]),
    ).toEqual(["Detach", "Resume"]);
    expect(
      enabledLabels(session("stopped", runtime("stopped")), ["runtime.resume"]),
    ).toEqual(["Resume"]);
  });

  it("hides managed-only interrupt for compatibility sessions", () => {
    expect(
      enabledLabels(
        session("active", runtime("running", "exec_compatibility")),
        ["session.detach", "runtime.interrupt", "runtime.stop"],
      ),
    ).toEqual(["Detach", "Stop"]);
  });

  it("disables every lifecycle control while a lifecycle mutation is pending", () => {
    const controls = lifecycleControlStates(
      {
        session: session("active", runtime("running")),
        runtime: runtime("running"),
        availableCommands: [
          "session.detach",
          "runtime.interrupt",
          "runtime.stop",
        ],
      },
      true,
    );

    expect(controls.every((control) => !control.enabled)).toBe(true);
  });
});

function enabledLabels(
  sessionSummary: SessionSummary,
  availableCommands: import("../../shared/protocol/types").ActionCapability[],
) {
  return lifecycleControlStates({
    session: sessionSummary,
    runtime: sessionSummary.runtime,
    availableCommands,
  })
    .filter((control) => control.enabled)
    .map((control) => control.label);
}

function runtime(
  state: RuntimeSummary["state"],
  executionProfile: RuntimeSummary["execution_profile"] = "exec_compatibility",
): RuntimeSummary {
  return {
    runtime_session_id: "runtime-1",
    provider: "codex",
    execution_profile: executionProfile,
    state,
    resume_supported: true,
    degraded_reason: null,
    last_runtime_step_at: null,
  };
}

function session(
  state: SessionSummary["state"],
  runtimeSummary: RuntimeSummary,
): SessionSummary {
  return {
    session_thread_id: "session-1",
    project_placement_id: "placement-1",
    runtime_session_id: runtimeSummary.runtime_session_id,
    title: "Session",
    state,
    runtime: runtimeSummary,
    message_count: 0,
    updated_at: "2026-06-17T00:00:00Z",
  };
}
