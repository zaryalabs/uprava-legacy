import { describe, expect, it } from "vitest";

import type { NodeSummary } from "../../shared/protocol/types";
import {
  defaultExecutionProfile,
  executionProfileOptions,
  managedUnavailableMessage,
  providerChoiceOptions,
  startUnavailableReasonFor,
} from "../sessions/StartSessionControl";

describe("providerChoiceOptions", () => {
  it("enables Codex only when the node advertises an available provider capability", () => {
    expect(providerChoiceOptions(undefined)).toEqual([
      { id: "codex", label: "Codex", available: false },
    ]);

    expect(
      providerChoiceOptions(
        nodeWithCapabilities([
          { key: "provider.codex", value: providerCapability(false) },
        ]),
      ),
    ).toEqual([{ id: "codex", label: "Codex", available: false }]);

    expect(
      providerChoiceOptions(
        nodeWithCapabilities([
          { key: "provider.codex", value: providerCapability(true) },
        ]),
      ),
    ).toEqual([{ id: "codex", label: "Codex", available: true }]);
  });
});

describe("executionProfileOptions", () => {
  it("recommends managed only when the Node reports the capability", () => {
    const options = executionProfileOptions(
      nodeWithCapabilities([
        { key: "provider.codex", value: providerCapability(true) },
        {
          key: "provider.codex.exec",
          value: { ...providerCapability(true), mode: "exec_compatibility" },
        },
        {
          key: "provider.codex.managed",
          value: { ...providerCapability(true), mode: "managed" },
        },
      ]),
      "codex",
    );

    expect(
      options.map(({ id, available, recommended }) => ({
        id,
        available,
        recommended,
      })),
    ).toEqual([
      { id: "managed", available: true, recommended: true },
      { id: "exec_compatibility", available: true, recommended: false },
    ]);
  });

  it("turns typed managed capability failures into actionable copy", () => {
    expect(managedUnavailableMessage("version_unsupported")).toContain(
      "supported Codex version",
    );
  });

  it("defaults capable Nodes to Managed and keeps incapable Nodes explicit", () => {
    const capable = nodeWithCapabilities([
      {
        key: "provider.codex.managed",
        value: { ...providerCapability(true), mode: "managed" },
      },
    ]);

    expect(defaultExecutionProfile(capable, "codex")).toBe("managed");
    expect(defaultExecutionProfile(undefined, "codex")).toBe(
      "exec_compatibility",
    );
  });
});

describe("startUnavailableReasonFor", () => {
  it("explains how to restore the unavailable Codex provider on a server node", () => {
    expect(
      startUnavailableReasonFor({
        canStart: true,
        node: nodeWithCapabilities([
          { key: "provider.codex", value: providerCapability(false) },
        ]),
        placement: validatedPlacement(),
        provider: "codex",
        providerAvailable: false,
      }),
    ).toBe(
      "Codex is not available to the Node Daemon. Install it for the daemon user, or set UPRAVA_CODEX_BINARY to its absolute path, then restart uprava-node.",
    );
  });

  it("explains placement blockers before trying to start a session", () => {
    expect(
      startUnavailableReasonFor({
        canStart: false,
        node: nodeWithCapabilities([
          { key: "provider.codex", value: providerCapability(true) },
        ]),
        placement: {
          state: "validated",
          resource_badges: [
            { label: "Low disk space", severity: "hard_block" },
          ],
        },
        provider: "codex",
        providerAvailable: true,
      }),
    ).toBe("Clear workspace blockers before starting Codex: Low disk space.");
  });
});

function providerCapability(available: boolean) {
  return {
    kind: "provider" as const,
    available,
    configured: true,
    mode: "exec",
    timeout_seconds: 120,
    unavailable_reason: available ? null : "binary_not_found",
  };
}

function nodeWithCapabilities(
  capabilities: NodeSummary["capabilities"],
): NodeSummary {
  return {
    node_id: "node-1",
    display_name: "Node",
    presence: "reachable",
    sleep_hint: "awake",
    heartbeat_age_seconds: 1,
    active_runtime_count: 0,
    capabilities,
    diagnostics: "",
  };
}

function validatedPlacement() {
  return { state: "validated", resource_badges: [] };
}
