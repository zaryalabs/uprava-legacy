import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, Check, Play, ShieldCheck } from "lucide-react";
import { useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";

import { queryKeys } from "../../shared/api/query-keys";
import { coreApi } from "../../shared/api/http-client";
import type {
  AgentExecutionProfile,
  NodeSummary,
  ProjectPlacementSummary,
} from "../../shared/protocol/types";
import { Badge } from "../../shared/ui/badge";
import { Button } from "../../shared/ui/button";
import { ErrorNotice } from "../../shared/ui/error-notice";
import {
  canRunCommand,
  runWorkbenchCommand,
} from "../../workbench/commands/registry";
import { routeWithSearch } from "../workspaces/routes";

export function StartSessionControl({
  node,
  placement,
}: {
  node: NodeSummary;
  placement: ProjectPlacementSummary;
}) {
  const navigate = useNavigate();
  const location = useLocation();
  const queryClient = useQueryClient();
  const [provider, setProvider] = useState<ProviderId>("codex");
  const [profileSelection, setProfileSelection] =
    useState<AgentExecutionProfile | null>(null);
  const [unsafeAcknowledged, setUnsafeAcknowledged] = useState(false);
  const [force, setForce] = useState(false);
  const executionProfile =
    profileSelection ?? defaultExecutionProfile(node, provider);
  const mutation = useMutation({
    mutationFn: () =>
      runWorkbenchCommand("session.start", {
        placement,
        provider,
        executionProfile,
        force,
        navigate: (path) => navigate(routeWithSearch(path, location.search)),
        afterSuccess: async () => {
          await queryClient.invalidateQueries({
            queryKey: queryKeys.inventory,
          });
        },
      }),
  });
  const providerOptions = providerChoiceOptions(node);
  const selectedProvider =
    providerOptions.find((option) => option.id === provider) ??
    providerOptions[0];
  const profileOptions = executionProfileOptions(node, provider);
  const selectedProfile = profileOptions.find(
    (option) => option.id === executionProfile,
  );
  const canStart = canRunCommand("session.start", { placement });
  const unavailableReason = startUnavailableReasonFor({
    canStart,
    node,
    placement,
    provider,
    providerAvailable:
      (selectedProvider?.available ?? false) &&
      (selectedProfile?.available ?? false),
  });
  const policyPreview = useQuery({
    queryKey: queryKeys.sessionPolicyPreview(
      placement.project_placement_id,
      executionProfile,
    ),
    queryFn: () =>
      coreApi.previewSessionPolicy({
        project_placement_id: placement.project_placement_id,
        provider,
        execution_profile: executionProfile,
      }),
    enabled:
      canStart &&
      Boolean(selectedProvider?.available) &&
      Boolean(selectedProfile?.available),
  });
  const unsafeStartBlocked =
    executionProfile === "exec_compatibility" && !unsafeAcknowledged;

  return (
    <section
      className="border-b border-[var(--color-border)] pb-4"
      aria-labelledby="start-session-title"
    >
      <div className="zarya-label">NEW RUNTIME</div>
      <h3 id="start-session-title" className="mt-1 text-sm font-bold">
        Start Agent
      </h3>
      {providerOptions.length > 1 ? (
        <div
          className="mt-3 grid grid-cols-2 border border-[var(--color-muted)] bg-[var(--color-bg)]"
          role="group"
          aria-label="Provider"
        >
          {providerOptions.map((option) => (
            <button
              key={option.id}
              type="button"
              aria-pressed={provider === option.id}
              disabled={!option.available || mutation.isPending}
              className={
                provider === option.id
                  ? "bg-[var(--color-ink)] px-2 py-2 text-xs font-medium text-[var(--color-inverse)] disabled:bg-[var(--color-muted)]"
                  : "px-2 py-2 text-xs font-medium hover:bg-[var(--color-bg-muted)] disabled:text-[var(--color-muted)]"
              }
              onClick={() => {
                setProvider(option.id);
                setProfileSelection(null);
                setUnsafeAcknowledged(false);
              }}
            >
              {option.label}
            </button>
          ))}
        </div>
      ) : null}
      <fieldset className="mt-3 space-y-2">
        <legend className="zarya-label">EXECUTION MODE</legend>
        {profileOptions.map((option) => (
          <label
            key={option.id}
            className={`block cursor-pointer border p-3 ${
              executionProfile === option.id
                ? "border-[var(--color-ink)] bg-[var(--color-bg)]"
                : "border-[var(--color-border)] bg-[var(--color-bg-muted)]"
            } ${option.available ? "" : "cursor-not-allowed opacity-60"}`}
          >
            <span className="flex items-start gap-2">
              <input
                className="mt-0.5"
                type="radio"
                name={`execution-profile-${placement.project_placement_id}`}
                value={option.id}
                checked={executionProfile === option.id}
                disabled={!option.available || mutation.isPending}
                onChange={() => {
                  setProfileSelection(option.id);
                  setUnsafeAcknowledged(false);
                }}
              />
              <span className="min-w-0 flex-1">
                <span className="flex flex-wrap items-center gap-1.5 text-xs font-bold">
                  {option.label}
                  {option.recommended ? (
                    <Badge tone="good">Recommended</Badge>
                  ) : null}
                </span>
                <span className="mt-1 block text-xs text-[var(--color-muted)]">
                  {option.description}
                </span>
                {!option.available && option.unavailableReason ? (
                  <span className="mt-1 block text-xs text-[var(--color-risk)]">
                    {managedUnavailableMessage(option.unavailableReason)}
                  </span>
                ) : null}
              </span>
            </span>
          </label>
        ))}
      </fieldset>

      <dl className="mt-3 grid gap-1 border-l-2 border-[var(--color-muted)] pl-3 text-xs">
        <div className="grid grid-cols-[4rem_minmax(0,1fr)] gap-2">
          <dt className="text-[var(--color-muted)]">Node</dt>
          <dd className="truncate font-medium">{node.display_name}</dd>
        </div>
        <div className="grid grid-cols-[4rem_minmax(0,1fr)] gap-2">
          <dt className="text-[var(--color-muted)]">Workspace</dt>
          <dd className="break-all font-mono">{placement.workspace_path}</dd>
        </div>
      </dl>

      {policyPreview.data ? (
        <PolicyPreview policy={policyPreview.data.effective_policy} />
      ) : policyPreview.isPending &&
        policyPreview.fetchStatus === "fetching" ? (
        <p className="mt-3 text-xs text-[var(--color-muted)]" role="status">
          Calculating effective policy…
        </p>
      ) : null}

      {executionProfile === "exec_compatibility" ? (
        <label className="mt-3 flex items-start gap-2 border border-[var(--color-risk)] bg-[var(--color-risk-soft)] p-3 text-xs">
          <input
            className="mt-0.5"
            type="checkbox"
            checked={unsafeAcknowledged}
            disabled={mutation.isPending}
            onChange={(event) => setUnsafeAcknowledged(event.target.checked)}
          />
          <span>
            <strong className="flex items-center gap-1">
              <AlertTriangle size={14} aria-hidden="true" />I understand this
              mode is unrestricted
            </strong>
            <span className="mt-1 block text-[var(--color-muted)]">
              It bypasses approval continuation and the managed sandbox. Use it
              only as an explicit compatibility fallback.
            </span>
          </span>
        </label>
      ) : null}
      <Button
        className="mt-3 w-full"
        variant="primary"
        disabled={
          mutation.isPending ||
          !canStart ||
          !selectedProvider?.available ||
          !selectedProfile?.available ||
          unsafeStartBlocked
        }
        onClick={() => mutation.mutate()}
      >
        <Play size={15} aria-hidden="true" />
        {mutation.isPending
          ? "Starting…"
          : `Start ${selectedProfile?.label ?? "Agent"}`}
      </Button>
      <label className="mt-3 flex items-start gap-2 text-xs text-[var(--color-muted)]">
        <input
          className="mt-0.5"
          type="checkbox"
          checked={force}
          disabled={mutation.isPending}
          onChange={(event) => setForce(event.target.checked)}
        />
        Force start at 5% or less provider quota
      </label>
      {unavailableReason ? (
        <div
          role="status"
          className="mt-3 border-l-2 border-[var(--color-muted)] bg-[var(--color-bg-muted)] p-2 text-xs"
        >
          <div className="font-bold">Start unavailable</div>
          <div className="mt-1 break-words">{unavailableReason}</div>
        </div>
      ) : null}
      {mutation.isError ? (
        <div className="mt-3">
          <ErrorNotice error={mutation.error} title="Session start failed" />
        </div>
      ) : null}
      {policyPreview.isError ? (
        <div className="mt-3">
          <ErrorNotice
            error={policyPreview.error}
            title="Policy preview failed"
          />
        </div>
      ) : null}
    </section>
  );
}

type ProviderId = "codex";

type ProviderChoiceOption = {
  id: ProviderId;
  label: string;
  available: boolean;
};

export type ExecutionProfileOption = {
  id: AgentExecutionProfile;
  label: string;
  description: string;
  recommended: boolean;
  available: boolean;
  unavailableReason: string | null;
};

export function executionProfileOptions(
  node: NodeSummary | undefined,
  provider: ProviderId,
): ExecutionProfileOption[] {
  const managed = providerProfileCapability(
    node,
    `provider.${provider}.managed`,
  );
  const compatibility =
    providerProfileCapability(node, `provider.${provider}.exec`) ??
    providerProfileCapability(node, `provider.${provider}`);
  return [
    {
      id: "managed",
      label: "Managed",
      description:
        "Workspace sandbox, live approvals and questions, true interrupt, and provider-native recovery.",
      recommended: true,
      available: Boolean(managed?.available),
      unavailableReason:
        managed?.unavailable_reason ?? "capability_not_reported",
    },
    {
      id: "exec_compatibility",
      label: "Exec compatibility",
      description:
        "Unrestricted one-shot execution with reconstructed continuation and no live approval round-trip.",
      recommended: false,
      available: Boolean(compatibility?.available),
      unavailableReason:
        compatibility?.unavailable_reason ?? "capability_not_reported",
    },
  ];
}

export function defaultExecutionProfile(
  node: NodeSummary | undefined,
  provider: ProviderId,
): AgentExecutionProfile {
  const managed = providerProfileCapability(
    node,
    `provider.${provider}.managed`,
  );
  return managed?.available ? "managed" : "exec_compatibility";
}

function providerProfileCapability(node: NodeSummary | undefined, key: string) {
  const capability = node?.capabilities.find(
    (candidate) => candidate.key === key,
  );
  return capability?.value.kind === "provider" ? capability.value : null;
}

export function managedUnavailableMessage(reason: string) {
  if (reason === "version_unsupported") {
    return "Managed mode needs a supported Codex version on this Node.";
  }
  if (reason === "binary_not_found") {
    return "Codex is not available to the Node Daemon.";
  }
  if (reason === "capability_not_reported") {
    return "This Node has not reported managed runtime capability.";
  }
  return `Managed mode is unavailable: ${reason}.`;
}

function PolicyPreview({
  policy,
}: {
  policy: import("../../shared/protocol/types").EffectiveRuntimePolicy;
}) {
  const managed = policy.execution_profile === "managed";
  return (
    <div className="mt-3 border border-[var(--color-border)] bg-[var(--color-bg-muted)] p-3 text-xs">
      <div className="flex items-center gap-2 font-bold">
        {managed ? (
          <ShieldCheck size={14} aria-hidden="true" />
        ) : (
          <AlertTriangle size={14} aria-hidden="true" />
        )}
        Effective policy
      </div>
      <div className="mt-2 grid grid-cols-2 gap-2">
        <span>
          <span className="block text-[var(--color-muted)]">Sandbox</span>
          {policy.sandbox_mode}
        </span>
        <span>
          <span className="block text-[var(--color-muted)]">Approvals</span>
          {policy.approval_mode}
        </span>
        <span>
          <span className="block text-[var(--color-muted)]">Network</span>
          {policy.network_posture}
        </span>
        <span>
          <span className="block text-[var(--color-muted)]">Recovery</span>
          {managed ? "provider-native" : "reconstructed"}
        </span>
      </div>
      <div className="mt-2 flex items-center gap-1 text-[var(--color-muted)]">
        <Check size={13} aria-hidden="true" />
        Preview is fixed into the runtime policy snapshot at start.
      </div>
    </div>
  );
}

export function providerChoiceOptions(
  node: NodeSummary | undefined,
): ProviderChoiceOption[] {
  return [
    {
      id: "codex",
      label: "Codex",
      available: providerCapabilityAvailable(node, "codex"),
    },
  ];
}

function providerCapabilityAvailable(
  node: NodeSummary | undefined,
  provider: ProviderId,
): boolean {
  if (!node) return false;
  const capability = node.capabilities.find(
    (candidate) => candidate.key === `provider.${provider}`,
  );
  return Boolean(
    capability?.value.kind === "provider" && capability.value.available,
  );
}

export function startUnavailableReasonFor({
  canStart,
  node,
  placement,
  provider,
  providerAvailable,
}: {
  canStart: boolean;
  node: NodeSummary | undefined;
  placement: {
    state: string;
    resource_badges: { label: string; severity: string }[];
  };
  provider: ProviderId;
  providerAvailable: boolean;
}): string | null {
  if (!canStart) {
    if (placement.state !== "validated") {
      return "Validate this workspace before starting Codex.";
    }
    const blockers = placement.resource_badges
      .filter((badge) => badge.severity === "hard_block")
      .map((badge) => badge.label);
    if (blockers.length > 0) {
      return `Clear workspace blockers before starting Codex: ${blockers.join(", ")}.`;
    }
  }

  if (providerAvailable) return null;
  if (!node) {
    return "Waiting for the node capability report. Refresh the workspace after the node reconnects.";
  }

  const capability = node.capabilities.find(
    (candidate) => candidate.key === `provider.${provider}`,
  );
  if (!capability || capability.value.kind !== "provider") {
    return "This node has not advertised Codex support. Verify that the Node Daemon is connected and refresh the workspace.";
  }
  if (capability.value.unavailable_reason === "binary_not_found") {
    return "Codex is not available to the Node Daemon. Install it for the daemon user, or set UPRAVA_CODEX_BINARY to its absolute path, then restart uprava-node.";
  }
  return capability.value.unavailable_reason
    ? `Codex is unavailable on this node: ${capability.value.unavailable_reason}.`
    : "Codex is unavailable on this node. Check the Node Daemon configuration and refresh the workspace.";
}
