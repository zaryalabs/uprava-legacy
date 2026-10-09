import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FolderOpen, ListTree, MessageSquareText } from "lucide-react";
import { useEffect } from "react";
import { Link, useLocation, useSearchParams } from "react-router-dom";

import { coreApi } from "../../shared/api/http-client";
import { queryKeys } from "../../shared/api/query-keys";
import { openSessionStream } from "../../shared/api/sse-client";
import { ErrorNotice } from "../../shared/ui/error-notice";
import { LoadingState } from "../../shared/ui/system";
import { StatusIndicator } from "../../shared/ui/status-indicator";
import {
  canRunCommand,
  runWorkbenchCommand,
} from "../../workbench/commands/registry";
import { sessionEventCursor } from "../../workbench/projection/apply-session-event";
import { applySessionStreamEventToCache } from "../../workbench/projection/session-stream-cache";
import { ReferenceActions } from "../../workbench/references/ReferenceActions";
import { AgentProjectionPanel } from "../agent-projection/AgentProjectionPanel";
import { EvidenceProjection } from "../artifacts/EvidenceProjection";
import { routeWithSearch, workspaceAgentRoute } from "../workspaces/routes";
import { ChatComposer } from "./ChatComposer";
import { CausalityPanel } from "./CausalityPanel";
import { LifecycleControls } from "./LifecycleControls";
import { RuntimePolicyPanel } from "./RuntimePolicyPanel";
import { ScheduledMessagesPanel } from "./ScheduledMessagesPanel";
import { sessionAttention } from "./session-attention";
import { SessionTimeline } from "./SessionTimeline";

export function SessionSurface({
  sessionThreadId,
}: {
  sessionThreadId: string;
}) {
  const location = useLocation();
  const [searchParams, setSearchParams] = useSearchParams();
  const sessionView =
    searchParams.get("agentView") === "trace" ? "trace" : "conversation";
  const queryClient = useQueryClient();
  const session = useQuery({
    queryKey: queryKeys.session(sessionThreadId),
    queryFn: () => coreApi.session(sessionThreadId),
    enabled: Boolean(sessionThreadId),
  });
  const agentProjection = useQuery({
    queryKey: queryKeys.agentProjection(sessionThreadId),
    queryFn: () => coreApi.agentProjection(sessionThreadId),
    enabled: Boolean(sessionThreadId),
  });
  const invalidateSession = async () => {
    await Promise.all([
      queryClient.invalidateQueries({
        queryKey: queryKeys.session(sessionThreadId),
      }),
      queryClient.invalidateQueries({ queryKey: queryKeys.inventory }),
    ]);
  };
  const sendTurn = useMutation({
    mutationFn: (request: {
      content: string;
      collaborationMode: "default" | "plan";
    }) =>
      runWorkbenchCommand("session.sendTurn", {
        session: session.data?.session,
        runtime: session.data?.session.runtime,
        turnContent: request.content,
        turnCollaborationMode: request.collaborationMode,
        availableCommands: agentProjection.data?.available_commands,
        afterSuccess: invalidateSession,
      }),
  });
  const loadedSessionThreadId = session.data?.session.session_thread_id;

  useEffect(() => {
    const sessionData = session.data;
    if (
      !sessionData ||
      !loadedSessionThreadId ||
      loadedSessionThreadId !== sessionThreadId
    ) {
      return;
    }
    const afterSeq = sessionData.events.reduce(
      (max, event) => Math.max(max, sessionEventCursor(event)),
      0,
    );
    return openSessionStream(
      sessionThreadId,
      afterSeq,
      (event) => {
        void applySessionStreamEventToCache(
          queryClient,
          sessionThreadId,
          event,
        );
      },
      () => {
        void queryClient.invalidateQueries({
          queryKey: queryKeys.session(sessionThreadId),
        });
      },
    );
  }, [loadedSessionThreadId, queryClient, sessionThreadId]);

  if (session.isError) {
    return <ErrorNotice error={session.error} title="Session load failed" />;
  }
  if (!session.data) return <LoadingState stage="Loading session" />;

  const canSendTurn = canRunCommand("session.sendTurn", {
    session: session.data.session,
    runtime: session.data.session.runtime,
    turnContent: "ready",
    availableCommands: agentProjection.data?.available_commands,
  });
  const workspaceRoute = routeWithSearch(
    workspaceAgentRoute(session.data.placement.project_placement_id),
    location.search,
  );
  const selectSessionView = (view: "conversation" | "trace") => {
    const next = new URLSearchParams(searchParams);
    if (view === "conversation") next.delete("agentView");
    else next.set("agentView", view);
    setSearchParams(next, { replace: true });
  };

  return (
    <article className="min-w-0" aria-labelledby="session-surface-title">
      <header className="uprava-session-header grid gap-4 border-b border-[var(--color-border)] pb-5 md:grid-cols-[minmax(0,1fr)_auto] md:items-end">
        <div className="min-w-0">
          <div className="zarya-caption">
            SESSION /{" "}
            {session.data.session.runtime.execution_profile ??
              "exec_compatibility"}{" "}
            / {session.data.session.runtime.state}
          </div>
          <h2
            id="session-surface-title"
            className="mt-2 truncate text-xl font-bold"
          >
            {session.data.session.title}
          </h2>
          <p className="mt-1 truncate text-xs text-[var(--color-muted)]">
            {session.data.placement.workspace_path}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Link
            to={workspaceRoute}
            className="inline-flex h-9 items-center justify-center gap-2 border border-[var(--color-muted)] bg-[var(--color-bg)] px-3 text-sm font-medium hover:border-[var(--color-ink)] hover:bg-[var(--color-bg-muted)]"
          >
            <FolderOpen size={16} aria-hidden="true" />
            Workspace
          </Link>
          <ReferenceActions
            reference={{
              kind: "session",
              session_thread_id: session.data.session.session_thread_id,
            }}
            showCopy={false}
          />
          <ReferenceActions
            reference={{
              kind: "runtime",
              runtime_session_id:
                session.data.session.runtime.runtime_session_id,
            }}
            showCopy={false}
          />
          <StatusIndicator
            showDimension
            dimension="lifecycle"
            value={session.data.session.state}
          />
          <StatusIndicator
            showDimension
            dimension="attention"
            value={sessionAttention(
              session.data.session.state,
              session.data.session.runtime.state,
              session.data.session.runtime.degraded_reason,
              session.data.session.runtime.last_runtime_step_at,
            )}
          />
        </div>
      </header>

      <section className="uprava-session-runtime grid gap-4 border-l-2 border-[var(--color-ink)] py-4 pl-4 md:grid-cols-[minmax(0,1fr)_auto] md:items-center">
        <div>
          <div className="zarya-label">Runtime Context</div>
          <div className="mt-1 text-sm">
            Phase <strong>{session.data.session.runtime.state}</strong> ·
            Session <strong>{session.data.session.state}</strong> · Resume{" "}
            {session.data.session.runtime.resume_supported
              ? "supported"
              : "unavailable"}
          </div>
          <div className="mt-1 text-xs text-[var(--color-muted)]">
            Detach closes this surface without stopping the provider runtime.
            Stop preserves the session history.
          </div>
        </div>
        <LifecycleControls
          session={session.data.session}
          runtime={session.data.session.runtime}
          availableCommands={agentProjection.data?.available_commands ?? []}
        />
        <div className="md:col-span-2">
          <RuntimePolicyPanel runtime={session.data.session.runtime} />
        </div>
      </section>

      <div className="space-y-4 pt-4">
        <nav
          aria-label="Session view"
          className="inline-flex border border-[var(--color-muted)] bg-[var(--color-bg)] p-0.5"
        >
          <button
            type="button"
            aria-pressed={sessionView === "conversation"}
            className={`inline-flex h-8 items-center gap-2 px-3 text-sm font-medium ${sessionView === "conversation" ? "bg-[var(--color-ink)] text-[var(--color-bg)]" : "hover:bg-[var(--color-bg-muted)]"}`}
            onClick={() => selectSessionView("conversation")}
          >
            <MessageSquareText size={15} aria-hidden="true" />
            Conversation
          </button>
          <button
            type="button"
            aria-pressed={sessionView === "trace"}
            className={`inline-flex h-8 items-center gap-2 px-3 text-sm font-medium ${sessionView === "trace" ? "bg-[var(--color-ink)] text-[var(--color-bg)]" : "hover:bg-[var(--color-bg-muted)]"}`}
            onClick={() => selectSessionView("trace")}
          >
            <ListTree size={15} aria-hidden="true" />
            Trace
          </button>
        </nav>

        {sessionView === "conversation" ? (
          <>
            <SessionTimeline
              detail={session.data}
              availableCommands={agentProjection.data?.available_commands ?? []}
            />
            {sendTurn.isError ? (
              <ErrorNotice error={sendTurn.error} title="Send failed" />
            ) : null}
            <ChatComposer
              pending={sendTurn.isPending}
              disabled={!canSendTurn}
              supportsPlanMode={
                session.data.session.runtime.execution_profile === "managed"
              }
              onSend={(content, collaborationMode) =>
                sendTurn
                  .mutateAsync({ content, collaborationMode })
                  .then(() => {})
              }
            />
            <ScheduledMessagesPanel
              sessionThreadId={session.data.session.session_thread_id}
              messages={session.data.scheduled_messages ?? []}
              onChanged={invalidateSession}
            />
          </>
        ) : (
          <CausalityPanel
            sessionThreadId={session.data.session.session_thread_id}
          />
        )}
        <details className="border-t border-[var(--color-border)] pt-4">
          <summary className="cursor-pointer text-sm font-bold">
            Session details
            <span className="ml-2 font-normal text-[var(--color-muted)]">
              Evidence and Agent Projection
            </span>
          </summary>
          <div className="mt-4 grid gap-6 border-l border-[var(--color-border)] pl-4 xl:grid-cols-2">
            <EvidenceProjection
              sessionThreadId={session.data.session.session_thread_id}
            />
            <AgentProjectionPanel
              sessionThreadId={session.data.session.session_thread_id}
            />
          </div>
        </details>
      </div>
    </article>
  );
}
