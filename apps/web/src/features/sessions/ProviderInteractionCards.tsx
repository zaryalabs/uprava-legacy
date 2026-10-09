import { useMutation, useQueryClient } from "@tanstack/react-query";
import { CircleDot, HelpCircle, ShieldAlert } from "lucide-react";
import { useState } from "react";

import { queryKeys } from "../../shared/api/query-keys";
import type {
  ActionCapability,
  ProviderInteractionSummary,
  SessionDetail,
} from "../../shared/protocol/types";
import { Badge } from "../../shared/ui/badge";
import { Button } from "../../shared/ui/button";
import { ErrorNotice } from "../../shared/ui/error-notice";
import { Textarea } from "../../shared/ui/textarea";
import {
  canRunCommand,
  runWorkbenchCommand,
} from "../../workbench/commands/registry";

export function ProviderInteractionCards({
  detail,
  availableCommands,
}: {
  detail: SessionDetail;
  availableCommands: ActionCapability[];
}) {
  const interactions = detail.pending_interactions ?? [];
  if (interactions.length === 0) return null;

  return (
    <section
      className="space-y-3"
      aria-labelledby="provider-interactions-title"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 id="provider-interactions-title" className="text-sm font-bold">
          Agent needs your decision
        </h3>
        <Badge tone="warn">
          <CircleDot size={13} aria-hidden="true" />
          {interactions.length} blocked
        </Badge>
      </div>
      {interactions.map((interaction) => (
        <ProviderInteractionCard
          key={interaction.provider_interaction_id}
          detail={detail}
          interaction={interaction}
          availableCommands={availableCommands}
        />
      ))}
    </section>
  );
}

function ProviderInteractionCard({
  detail,
  interaction,
  availableCommands,
}: {
  detail: SessionDetail;
  interaction: ProviderInteractionSummary;
  availableCommands: ActionCapability[];
}) {
  const queryClient = useQueryClient();
  const [response, setResponse] = useState("");
  const invalidate = async () => {
    await Promise.all([
      queryClient.invalidateQueries({
        queryKey: queryKeys.session(detail.session.session_thread_id),
      }),
      queryClient.invalidateQueries({ queryKey: queryKeys.inventory }),
    ]);
  };
  const resolveApproval = useMutation({
    mutationFn: (approved: boolean) =>
      runWorkbenchCommand("providerInteraction.resolveApproval", {
        session: detail.session,
        runtime: detail.session.runtime,
        providerInteractionId: interaction.provider_interaction_id,
        interactionMessage: response,
        approved,
        availableCommands,
        afterSuccess: invalidate,
      }),
  });
  const submitInput = useMutation({
    mutationFn: () =>
      runWorkbenchCommand("providerInteraction.submitInput", {
        session: detail.session,
        runtime: detail.session.runtime,
        providerInteractionId: interaction.provider_interaction_id,
        answers: [response],
        availableCommands,
        afterSuccess: invalidate,
      }),
  });
  const requested = interaction.state === "requested";
  const isApproval = interaction.kind === "approval";
  const canApprove =
    requested &&
    canRunCommand("providerInteraction.resolveApproval", {
      session: detail.session,
      runtime: detail.session.runtime,
      providerInteractionId: interaction.provider_interaction_id,
      approved: true,
      availableCommands,
    });
  const canSubmitInput =
    requested &&
    canRunCommand("providerInteraction.submitInput", {
      session: detail.session,
      runtime: detail.session.runtime,
      providerInteractionId: interaction.provider_interaction_id,
      answers: [response],
      availableCommands,
    });
  const pending = resolveApproval.isPending || submitInput.isPending;
  const error = resolveApproval.error ?? submitInput.error;
  const policy = detail.session.runtime.effective_policy;
  const inputId = `provider-interaction-${interaction.provider_interaction_id}`;

  return (
    <article
      className="border-l-2 border-[var(--color-notice)] bg-[var(--color-notice-soft)] p-4"
      aria-live="polite"
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="flex items-center gap-2">
          {isApproval ? (
            <ShieldAlert size={17} aria-hidden="true" />
          ) : (
            <HelpCircle size={17} aria-hidden="true" />
          )}
          <div>
            <div className="text-sm font-bold">
              {isApproval ? "Provider approval" : "Provider question"}
            </div>
            <div className="text-xs text-[var(--color-muted)]">
              Codex managed runtime · {interaction.state}
            </div>
          </div>
        </div>
        <Badge tone={requested ? "warn" : "info"}>{interaction.state}</Badge>
      </div>

      <p className="mt-3 whitespace-pre-wrap break-words text-sm">
        {interaction.prompt}
      </p>

      <dl className="mt-3 grid gap-2 text-xs sm:grid-cols-3">
        <div>
          <dt className="text-[var(--color-muted)]">Scope</dt>
          <dd className="break-all">
            {policy?.workspace_root ?? "Current runtime"}
          </dd>
        </div>
        <div>
          <dt className="text-[var(--color-muted)]">Sandbox</dt>
          <dd>{policy?.sandbox_mode ?? "Unknown"}</dd>
        </div>
        <div>
          <dt className="text-[var(--color-muted)]">Approval policy</dt>
          <dd>{policy?.approval_mode ?? "Unknown"}</dd>
        </div>
      </dl>

      {requested ? (
        <div className="mt-3">
          <label htmlFor={inputId} className="text-xs font-bold">
            {isApproval ? "Optional message to provider" : "Your answer"}
          </label>
          <Textarea
            id={inputId}
            className="mt-1 min-h-20"
            value={response}
            maxLength={16_384}
            placeholder={
              isApproval
                ? "Add context for this decision (optional)"
                : "Type the information Codex requested"
            }
            disabled={pending}
            onChange={(event) => setResponse(event.target.value)}
          />
          <div className="mt-2 flex flex-wrap gap-2">
            {isApproval ? (
              <>
                <Button
                  variant="primary"
                  disabled={pending || !canApprove}
                  onClick={() => resolveApproval.mutate(true)}
                >
                  Approve
                </Button>
                <Button
                  variant="danger"
                  disabled={pending || !canApprove}
                  onClick={() => resolveApproval.mutate(false)}
                >
                  Deny
                </Button>
              </>
            ) : (
              <Button
                variant="primary"
                disabled={pending || !canSubmitInput}
                onClick={() => submitInput.mutate()}
              >
                Submit answer
              </Button>
            )}
          </div>
        </div>
      ) : (
        <p className="mt-3 text-xs text-[var(--color-muted)]" role="status">
          Decision sent. Waiting for provider confirmation; repeated submission
          is disabled.
        </p>
      )}

      {error ? (
        <div className="mt-3">
          <ErrorNotice error={error} title="Decision was not accepted" />
        </div>
      ) : null}
    </article>
  );
}
