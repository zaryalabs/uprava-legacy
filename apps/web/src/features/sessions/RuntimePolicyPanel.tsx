import {
  Activity,
  AlertTriangle,
  Clock3,
  RotateCcw,
  ShieldCheck,
} from "lucide-react";

import type { RuntimeSummary } from "../../shared/protocol/types";
import { Badge } from "../../shared/ui/badge";

export function RuntimePolicyPanel({ runtime }: { runtime: RuntimeSummary }) {
  const profile = runtime.execution_profile ?? "exec_compatibility";
  const policy = runtime.effective_policy;
  const attempt = runtime.current_attempt;
  const managed = profile === "managed";

  return (
    <section aria-labelledby="runtime-policy-title" className="space-y-2">
      <div className="flex flex-wrap items-center gap-2">
        <Badge tone={managed ? "good" : "bad"}>
          {managed ? (
            <ShieldCheck size={13} aria-hidden="true" />
          ) : (
            <AlertTriangle size={13} aria-hidden="true" />
          )}
          {profileLabel(profile)}
        </Badge>
        {policy ? (
          <Badge tone={policy.unsafe_override ? "bad" : "neutral"}>
            {policy.sandbox_mode} · {policy.approval_mode}
          </Badge>
        ) : null}
        {runtime.recovery_status ? (
          <Badge tone={recoveryTone(runtime.recovery_status)}>
            <RotateCcw size={13} aria-hidden="true" />
            {runtime.recovery_status}
          </Badge>
        ) : null}
      </div>

      {!managed ? (
        <div
          className="border-l-2 border-[var(--color-risk)] bg-[var(--color-risk-soft)] p-3 text-xs"
          role="status"
        >
          <div className="flex items-center gap-1 font-bold text-[var(--color-risk)]">
            <AlertTriangle size={14} aria-hidden="true" />
            Unrestricted compatibility runtime
          </div>
          <p className="mt-1 text-[var(--color-muted)]">
            Live approval continuation and true managed interrupt are not
            available. This profile never silently upgrades or falls back.
          </p>
        </div>
      ) : null}

      <details className="border border-[var(--color-border)] bg-[var(--color-bg-muted)] p-3 text-xs">
        <summary
          id="runtime-policy-title"
          className="cursor-pointer font-bold focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2"
        >
          Policy and runtime diagnostics
        </summary>
        <dl className="mt-3 grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
          <Diagnostic label="Driver" value={driverLabel(profile)} />
          <Diagnostic
            label="Provider"
            value={`${runtime.provider}${policy?.provider_version ? ` ${policy.provider_version}` : ""}`}
          />
          <Diagnostic
            label="Policy"
            value={
              policy
                ? `${policy.sandbox_mode} / ${policy.approval_mode}`
                : "Snapshot unavailable"
            }
          />
          <Diagnostic
            label="Attempt"
            value={
              attempt
                ? `${attempt.state} · ${shortId(attempt.runtime_attempt_id)}`
                : "No current attempt"
            }
          />
          <Diagnostic
            label="Recovery"
            value={
              attempt?.recovery_reason ??
              runtime.degraded_reason ??
              runtime.recovery_status ??
              "No recovery required"
            }
          />
          <Diagnostic
            label="Last activity"
            value={formatTimestamp(runtime.last_runtime_step_at)}
          />
          <Diagnostic
            label="Policy hash"
            value={shortId(runtime.effective_policy_hash ?? "unavailable")}
            mono
          />
          <Diagnostic
            label="Network"
            value={policy?.network_posture ?? "Unknown"}
          />
          <Diagnostic
            label="Tool exposure"
            value={
              policy
                ? `${policy.tool_exposure.tool_count} tools / ${policy.tool_exposure.server_count} servers`
                : "Unknown"
            }
          />
        </dl>
        <div className="mt-3 flex flex-wrap gap-3 text-[var(--color-muted)]">
          <span className="inline-flex items-center gap-1">
            <Activity size={13} aria-hidden="true" />
            State: {runtime.state}
          </span>
          {attempt ? (
            <span className="inline-flex items-center gap-1">
              <Clock3 size={13} aria-hidden="true" />
              Attempt started {formatTimestamp(attempt.started_at)}
            </span>
          ) : null}
        </div>
      </details>
    </section>
  );
}

function Diagnostic({
  label,
  value,
  mono = false,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div className="min-w-0">
      <dt className="text-[var(--color-muted)]">{label}</dt>
      <dd className={`mt-0.5 break-words ${mono ? "font-mono" : ""}`}>
        {value}
      </dd>
    </div>
  );
}

export function profileLabel(profile: RuntimeSummary["execution_profile"]) {
  return profile === "managed" ? "Managed" : "Exec compatibility";
}

function driverLabel(profile: RuntimeSummary["execution_profile"]) {
  return profile === "managed" ? "Codex app-server v2" : "codex exec/resume";
}

function recoveryTone(status: NonNullable<RuntimeSummary["recovery_status"]>) {
  if (status === "failed" || status === "lost") return "bad" as const;
  if (status === "degraded" || status === "reconnecting")
    return "warn" as const;
  if (status === "live" || status === "recovered") return "good" as const;
  return "neutral" as const;
}

function shortId(value: string) {
  return value.length > 18 ? `${value.slice(0, 8)}…${value.slice(-6)}` : value;
}

function formatTimestamp(value: string | null) {
  return value ? new Date(value).toLocaleString() : "Not observed";
}
