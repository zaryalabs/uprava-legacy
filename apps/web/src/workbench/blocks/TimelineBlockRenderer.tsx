import { useEffect, useState, type ComponentType, type ReactNode } from "react";
import {
  Activity,
  AlertTriangle,
  Bot,
  CheckCircle2,
  CircleDot,
  Clock3,
  HelpCircle,
  LoaderCircle,
  User,
} from "lucide-react";

import {
  PluginBlockRenderer,
  PluginContentRenderer,
} from "../../plugins/ExtensionHost";
import { Badge } from "../../shared/ui/badge";
import { DisclosureControl } from "../../shared/ui/system";
import type { UiBlock } from "./types";

type BadgeTone = "neutral" | "good" | "warn" | "bad" | "info";

export type BlockRendererProps = {
  block: UiBlock;
  actions?: ReactNode;
};

export type BlockFallbackProps = BlockRendererProps;

export type BlockRendererRegistration = {
  type: string;
  supportedSchemaVersions: number[];
  allowedSurfaces: string[];
  render: ComponentType<BlockRendererProps>;
  fallback: ComponentType<BlockFallbackProps>;
};

const rendererRegistrations: BlockRendererRegistration[] = [
  register("core.user-message", MessageBlock),
  register("core.assistant-message", MessageBlock),
  register("core.session-activity", SessionActivityBlock),
  register("core.turn-activity", TurnActivityBlock),
  register("core.provider-output-stream", EventBlock),
  register("core.runtime-event", EventBlock),
  register("core.workspace-validation", EventBlock),
  register("core.resource-snapshot", EventBlock),
  register("core.warning", WarningBlock),
  register("core.error", ErrorBlock),
  register("core.approval-request", ApprovalBlock),
  register("core.provider-interaction", ProviderInteractionBlock),
  register("core.unknown", UnknownBlock),
];

const rendererByType = new Map(
  rendererRegistrations.map((registration) => [
    registration.type,
    registration,
  ]),
);

export function TimelineBlockRenderer({ block, actions }: BlockRendererProps) {
  const registration = rendererByType.get(block.type);
  if (
    !registration ||
    !registration.supportedSchemaVersions.includes(block.schema_version)
  ) {
    const Fallback = registration?.fallback ?? UnknownBlock;
    const fallback = <Fallback block={block} actions={actions} />;
    return (
      <PluginBlockRenderer
        block={block}
        actions={actions}
        fallback={fallback}
      />
    );
  }
  const Renderer = registration.render;
  const fallback = <Renderer block={block} actions={actions} />;
  return (
    <PluginBlockRenderer block={block} actions={actions} fallback={fallback} />
  );
}

export function getTimelineBlockRenderer(type: string) {
  return rendererByType.get(type);
}

export function registeredTimelineBlockTypes() {
  return rendererRegistrations.map((registration) => registration.type);
}

function register(
  type: string,
  render: ComponentType<BlockRendererProps>,
  fallback: ComponentType<BlockFallbackProps> = UnknownBlock,
): BlockRendererRegistration {
  return {
    type,
    supportedSchemaVersions: [1],
    allowedSurfaces: ["session.timeline"],
    render,
    fallback,
  };
}

function MessageBlock({ block, actions }: BlockRendererProps) {
  const data = blockData(block);
  const isAssistant = block.type === "core.assistant-message";
  const label = isAssistant ? "Agent" : "You";
  const Icon = isAssistant ? Bot : User;
  const content = stringField(data, "content", block.fallback_text ?? "");
  const fallback = <PlainMessageContent content={content} />;

  return (
    <article
      className={`border-l-2 py-3 pl-3 ${isAssistant ? "border-[var(--color-ink)]" : "border-[var(--color-muted)]"}`}
    >
      <div
        className={
          isAssistant
            ? "mb-1 flex items-center gap-1.5 text-xs font-bold text-[var(--color-muted)]"
            : "mb-1 flex items-center gap-1.5 text-xs font-bold text-[var(--color-muted)]"
        }
      >
        <Icon size={14} aria-hidden="true" />
        {label}
      </div>
      <PluginContentRenderer
        content={content}
        fallback={fallback}
        sourceKind={
          isAssistant ? "chat.assistant_message" : "chat.user_message"
        }
        sourceRef={block.primary_ref}
        state="complete"
        surfaceId={block.surface_id}
      />
      {isAssistant ? (
        <div className="mt-2 text-xs text-[var(--color-muted)]">
          Evidence and source are available through the reference action.
        </div>
      ) : null}
      <BlockActions actions={actions} />
    </article>
  );
}

function PlainMessageContent({ content }: { content: string }) {
  return <p className="whitespace-pre-wrap break-words text-sm">{content}</p>;
}

function SessionActivityBlock({ block, actions }: BlockRendererProps) {
  const data = blockData(block);
  const rows = arrayField(data, "rows").filter(isRecord);
  const completed = booleanField(data, "completed", false);
  const [expanded, setExpanded] = useState(false);
  const durationMs = durationBetween(
    stringField(data, "startedAt", ""),
    stringField(data, "completedAt", ""),
  );

  return (
    <article className="border-l-2 border-[var(--color-muted)] py-2 pl-3 text-sm">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <DisclosureControl
            expanded={expanded}
            label="session initialization"
            onClick={() => setExpanded((current) => !current)}
          />
          <div className="min-w-0">
            <div className="font-bold">
              {completed ? "Session initialized" : "Initializing session…"}
            </div>
            <div className="text-xs text-[var(--color-muted)]">
              {completed
                ? "Runtime is ready for agent turns."
                : "Waiting for the runtime to become ready."}
            </div>
          </div>
        </div>
        <div className="flex flex-wrap gap-1.5">
          <ActivityCounter label="events" value={rows.length} />
          {durationMs > 0 ? (
            <ActivityCounter label="time" value={formatDuration(durationMs)} />
          ) : null}
        </div>
      </div>
      {expanded ? <ActivityRows rows={rows} className="mt-3" /> : null}
      <BlockActions actions={actions} />
    </article>
  );
}

function ApprovalBlock({ block, actions }: BlockRendererProps) {
  const data = blockData(block);

  return (
    <article className="border-l-2 border-[var(--color-muted)] py-3 pl-3">
      <div className="mb-2 flex items-center gap-2">
        <Badge tone="warn">
          <CircleDot size={13} />
          Approval
        </Badge>
        <span className="font-mono text-xs text-[var(--color-muted)]">
          {stringField(data, "approvalId", "pending")}
        </span>
      </div>
      <p className="whitespace-pre-wrap break-words text-sm">
        {stringField(
          data,
          "prompt",
          block.fallback_text ?? "Approval requested",
        )}
      </p>
      <dl className="mt-3 grid gap-2 text-xs text-[var(--color-muted)] sm:grid-cols-3">
        <div>
          <dt>Affected Scope</dt>
          <dd className="text-[var(--color-ink)]">Current runtime</dd>
        </div>
        <div>
          <dt>Risk</dt>
          <dd className="text-[var(--color-ink)]">Command-dependent</dd>
        </div>
        <div>
          <dt>Reversibility</dt>
          <dd className="text-[var(--color-ink)]">Review before approval</dd>
        </div>
      </dl>
      <BlockActions actions={actions} />
    </article>
  );
}

function ProviderInteractionBlock({ block, actions }: BlockRendererProps) {
  const data = blockData(block);
  const kind = stringField(data, "interactionKind", "approval");
  const state = stringField(data, "state", "requested");
  const question = kind === "user_input";
  const resolved = state === "resolved";
  const approved = booleanField(data, "approved", false);
  const answered = booleanField(data, "answered", false);
  const terminalLabel = question
    ? answered
      ? "Answer received"
      : "Question closed"
    : approved
      ? "Approved"
      : "Denied";

  return (
    <article className="border-l-2 border-[var(--color-notice)] py-3 pl-3">
      <div className="mb-2 flex flex-wrap items-center gap-2">
        <Badge tone={resolved ? "neutral" : "warn"}>
          {question ? (
            <HelpCircle size={13} aria-hidden="true" />
          ) : (
            <CircleDot size={13} aria-hidden="true" />
          )}
          {question ? "Provider question" : "Provider approval"}
        </Badge>
        <span className="text-xs text-[var(--color-muted)]">
          {resolved ? terminalLabel : "Waiting for your decision"}
        </span>
      </div>
      {stringField(data, "prompt", "") ? (
        <p className="whitespace-pre-wrap break-words text-sm">
          {stringField(data, "prompt", "")}
        </p>
      ) : null}
      <BlockActions actions={actions} />
    </article>
  );
}

function TurnActivityBlock({ block, actions }: BlockRendererProps) {
  const data = blockData(block);
  const rows = arrayField(data, "rows").filter(isRecord);
  const completed = booleanField(data, "completed", false);
  const [manualExpanded, setManualExpanded] = useState<boolean | null>(null);
  const expanded = manualExpanded ?? !completed;
  const durationMs = numberField(data, "durationMs", 0);
  const providerEventCount = numberField(data, "providerEventCount", 0);
  const lastObservedAt = stringField(data, "lastObservedAt", "");
  const now = useCurrentTime(!completed);
  const silentForMs = lastObservedAt
    ? Math.max(0, now - new Date(lastObservedAt).getTime())
    : 0;
  const stalled = !completed && silentForMs >= 30_000;
  const terminalKind = stringField(data, "terminalKind", "");
  const failed = terminalKind === "runtime.error";
  const stateLabel = completed
    ? failed
      ? "Agent failed"
      : terminalKind === "turn.interrupted"
        ? "Interrupted"
        : terminalKind === "runtime.blocked"
          ? "Blocked"
          : "Completed"
    : stalled
      ? "No recent activity"
      : "Agent is working";

  return (
    <article
      className={`border-l-2 py-3 pl-3 ${stalled || failed ? "border-[var(--color-notice)] bg-[var(--color-notice-soft)]" : "border-[var(--color-muted)]"}`}
      aria-live="polite"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <DisclosureControl
            expanded={expanded}
            label="activity"
            onClick={() => setManualExpanded(!expanded)}
          />
          <div className="min-w-0">
            <div className="flex items-center gap-1.5 text-xs font-bold text-[var(--color-muted)]">
              {failed ? (
                <AlertTriangle size={14} aria-hidden="true" />
              ) : completed ? (
                <CheckCircle2 size={14} aria-hidden="true" />
              ) : stalled ? (
                <Clock3 size={14} aria-hidden="true" />
              ) : (
                <LoaderCircle size={14} aria-hidden="true" />
              )}
              {stateLabel}
            </div>
            <div className="truncate text-xs text-[var(--color-muted)]">
              {completed
                ? "Observed work for this turn"
                : stalled
                  ? `No observed event for ${formatDuration(silentForMs)}`
                  : "Observed events appear here as they arrive"}
            </div>
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-1.5">
          <ActivityCounter label="activity" value={providerEventCount} />
          <ActivityCounter
            label="commands"
            value={numberField(data, "commandCount", 0)}
          />
          <ActivityCounter
            label="files"
            value={numberField(data, "fileChangeCount", 0)}
          />
          <ActivityCounter
            label="warnings"
            value={numberField(data, "warningErrorCount", 0)}
          />
          {durationMs > 0 ? (
            <ActivityCounter label="time" value={formatDuration(durationMs)} />
          ) : null}
        </div>
      </div>
      {expanded ? (
        <>
          {providerEventCount === 0 && !completed ? (
            <div className="mt-3 border-t border-[var(--color-muted)] pt-3 text-sm text-[var(--color-muted)]">
              The provider process started, but no agent activity has been
              observed yet.
            </div>
          ) : null}
          <ActivityRows rows={rows} className="mt-3" />
        </>
      ) : null}
      <BlockActions actions={actions} />
    </article>
  );
}

function ActivityRows({
  rows,
  className = "",
}: {
  rows: Record<string, unknown>[];
  className?: string;
}) {
  return (
    <div
      className={`${className} max-h-80 overflow-y-auto border-t border-[var(--color-muted)] pt-2`}
    >
      <div className="space-y-2">
        {rows.map((row, index) => (
          <ActivityRow
            key={stringField(row, "eventId", `activity-row-${index}`)}
            row={row}
          />
        ))}
      </div>
    </div>
  );
}

function ActivityCounter({
  label,
  value,
}: {
  label: string;
  value: number | string;
}) {
  return (
    <span className="border border-[var(--color-muted)] bg-[var(--color-bg)] px-1.5 py-0.5 text-xs text-[var(--color-muted)]">
      <span className="font-mono">{value}</span> {label}
    </span>
  );
}

function ActivityRow({ row }: { row: Record<string, unknown> }) {
  const phase = stringField(row, "phase", "observed");
  const status = stringField(row, "status", phase);
  const rawText = rawActivityText(row);

  return (
    <div className="grid gap-1 border-b border-[var(--color-muted)] pb-2 last:border-b-0">
      <div className="flex flex-wrap items-center gap-2 text-xs">
        <span className="font-mono text-[var(--color-muted)]">
          seq {numberField(row, "seq", 0)}
        </span>
        <Badge tone={activityTone(status)}>
          {stringField(row, "providerEventType", "provider.activity")}
        </Badge>
        <span className="text-[var(--color-muted)]">{status}</span>
        {stringField(row, "providerItemType", "") ? (
          <span className="font-mono text-[var(--color-muted)]">
            {stringField(row, "providerItemType", "")}
          </span>
        ) : null}
      </div>
      <div className="break-words text-sm text-[var(--color-ink)]">
        {stringField(row, "summary", "Provider activity")}
      </div>
      {rawText ? (
        <details className="text-xs text-[var(--color-muted)]">
          <summary className="cursor-pointer select-none">Raw</summary>
          <pre className="mt-1 max-h-56 overflow-auto border-l border-[var(--color-muted)] bg-[var(--color-bg-muted)] p-2 font-mono text-[11px] leading-4 text-[var(--color-ink)]">
            {rawText}
          </pre>
        </details>
      ) : null}
    </div>
  );
}

function EventBlock({ block, actions }: BlockRendererProps) {
  const data = blockData(block);
  const eventKind = stringField(data, "eventKind", block.type);

  return (
    <article className="border-l-2 border-[var(--color-muted)] py-3 pl-3">
      <div className="mb-1 flex flex-wrap items-center gap-2">
        <Badge tone={eventTone(block.type)}>
          <CheckCircle2 size={13} />
          {eventKind}
        </Badge>
        <span className="font-mono text-xs text-[var(--color-muted)]">
          seq {numberField(data, "seq", 0)}
        </span>
      </div>
      <div className="break-words text-sm text-[var(--color-muted)]">
        {stringField(data, "summary", block.fallback_text ?? eventKind)}
      </div>
      <BlockActions actions={actions} />
    </article>
  );
}

function WarningBlock({ block, actions }: BlockRendererProps) {
  const data = blockData(block);

  return (
    <article className="border-l-2 border-[var(--color-muted)] py-3 pl-3">
      <div className="mb-1 flex items-center gap-2 text-xs font-semibold uppercase tracking-normal text-[var(--color-muted)]">
        <AlertTriangle size={14} />
        {stringField(data, "eventKind", "Warning")}
      </div>
      <div className="break-words text-sm text-[var(--color-muted)]">
        {stringField(data, "summary", block.fallback_text ?? "Warning")}
      </div>
      <BlockActions actions={actions} />
    </article>
  );
}

function ErrorBlock({ block, actions }: BlockRendererProps) {
  const data = blockData(block);

  return (
    <article className="border-l-2 border-[var(--color-risk)] bg-[var(--color-risk-soft)] py-3 pl-3">
      <div className="mb-1 flex items-center gap-2 text-xs font-semibold uppercase tracking-normal text-[var(--color-risk)]">
        <AlertTriangle size={14} />
        {stringField(data, "eventKind", "Error")}
      </div>
      <div className="break-words text-sm text-[var(--color-muted)]">
        {stringField(data, "summary", block.fallback_text ?? "Runtime error")}
      </div>
      <div className="mt-2 text-xs text-[var(--color-risk)]">
        Affected scope: current runtime. Next safe step: inspect the source
        event, then retry or stop.
      </div>
      <BlockActions actions={actions} />
    </article>
  );
}

function UnknownBlock({ block, actions }: BlockFallbackProps) {
  return (
    <article className="border-l-2 border-dashed border-[var(--color-muted)] py-3 pl-3">
      <div className="mb-1 flex items-center gap-2 text-xs font-semibold uppercase tracking-normal text-[var(--color-muted)]">
        <HelpCircle size={14} />
        Unknown
      </div>
      <div className="break-words text-sm">
        {block.fallback_text || block.type}
      </div>
      <BlockActions actions={actions} />
    </article>
  );
}

function BlockActions({ actions }: { actions?: ReactNode }) {
  return actions ? (
    <div className="mt-3 flex flex-wrap items-center gap-2">{actions}</div>
  ) : null;
}

function eventTone(type: string): BadgeTone {
  if (
    type === "core.workspace-validation" ||
    type === "core.resource-snapshot"
  ) {
    return "good";
  }
  if (type === "core.provider-output-stream") return "info";
  return "warn";
}

function blockData(block: UiBlock): Record<string, unknown> {
  return typeof block.data === "object" && block.data !== null
    ? (block.data as Record<string, unknown>)
    : {};
}

function stringField(
  data: Record<string, unknown>,
  field: string,
  fallback: string,
) {
  const value = data[field];
  return typeof value === "string" && value.length > 0 ? value : fallback;
}

function numberField(
  data: Record<string, unknown>,
  field: string,
  fallback: number,
) {
  const value = data[field];
  return typeof value === "number" ? value : fallback;
}

function booleanField(
  data: Record<string, unknown>,
  field: string,
  fallback: boolean,
) {
  const value = data[field];
  return typeof value === "boolean" ? value : fallback;
}

function arrayField(data: Record<string, unknown>, field: string) {
  const value = data[field];
  return Array.isArray(value) ? value : [];
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function activityTone(status: string): BadgeTone {
  const normalized = status.toLowerCase();
  if (
    normalized.includes("error") ||
    normalized.includes("failed") ||
    normalized.includes("warning")
  ) {
    return normalized.includes("warning") ? "warn" : "bad";
  }
  if (normalized.includes("completed")) return "good";
  return "info";
}

function rawActivityText(row: Record<string, unknown>) {
  const rawEvent = row.rawEvent;
  if (rawEvent !== undefined) return formatRawValue(rawEvent);
  const preview = row.rawEventPreview;
  return typeof preview === "string" ? preview : null;
}

function formatRawValue(value: unknown) {
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function formatDuration(durationMs: number) {
  if (durationMs < 1000) return `${durationMs}ms`;
  const seconds = durationMs / 1000;
  if (seconds < 60) return `${seconds.toFixed(seconds < 10 ? 1 : 0)}s`;
  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = Math.round(seconds % 60);
  return `${minutes}m ${remainingSeconds}s`;
}

function durationBetween(startedAt: string, completedAt: string) {
  if (!startedAt || !completedAt) return 0;
  return Math.max(
    0,
    new Date(completedAt).getTime() - new Date(startedAt).getTime(),
  );
}

function useCurrentTime(active: boolean) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [active]);
  return now;
}
