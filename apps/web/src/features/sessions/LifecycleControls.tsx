import { useMutation, useQueryClient } from "@tanstack/react-query";
import { LogIn, LogOut, Pause, Play, RotateCcw, Square } from "lucide-react";

import { queryKeys } from "../../shared/api/query-keys";
import type {
  ActionCapability,
  RuntimeSummary,
  SessionSummary,
} from "../../shared/protocol/types";
import { Button } from "../../shared/ui/button";
import { ErrorNotice } from "../../shared/ui/error-notice";
import {
  canRunCommand,
  runWorkbenchCommand,
  type WorkbenchCommandId,
  type WorkbenchCommandContext,
} from "../../workbench/commands/registry";

type Props = {
  session: SessionSummary;
  runtime: RuntimeSummary;
  availableCommands: ActionCapability[];
};

export function LifecycleControls({
  session,
  runtime,
  availableCommands,
}: Props) {
  const queryClient = useQueryClient();
  const invalidate = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: queryKeys.inventory }),
      queryClient.invalidateQueries({
        queryKey: queryKeys.session(session.session_thread_id),
      }),
    ]);
  };
  const context = {
    session,
    runtime,
    availableCommands,
    afterSuccess: invalidate,
  };
  const command = useMutation({
    mutationFn: (id: WorkbenchCommandId) => runWorkbenchCommand(id, context),
  });
  const controls = lifecycleControlStates(context, command.isPending);

  const resume = controls.find((control) => control.id === "runtime.resume");

  return (
    <div className="grid justify-items-start gap-2 md:justify-items-end">
      <div className="flex flex-wrap gap-2">
        {controls.map((control) => {
          const Icon = control.icon;
          return (
            <Button
              key={control.id}
              variant={control.variant}
              disabled={!control.enabled}
              onClick={() => command.mutate(control.id)}
            >
              <Icon size={15} aria-hidden="true" />
              {control.label}
            </Button>
          );
        })}
      </div>
      {resume?.enabled ? (
        <p className="max-w-sm text-xs text-[var(--color-muted)]">
          Resume uses policy {shortPolicyHash(runtime.effective_policy_hash)}.
          {runtime.current_attempt?.recovery_reason || runtime.degraded_reason
            ? ` Recovery: ${runtime.current_attempt?.recovery_reason ?? runtime.degraded_reason}.`
            : " No policy drift is reported."}
        </p>
      ) : null}
      {command.isError ? (
        <ErrorNotice error={command.error} title="Lifecycle command failed" />
      ) : null}
    </div>
  );
}

export type LifecycleControlState = {
  id: Extract<
    WorkbenchCommandId,
    | "session.attach"
    | "session.detach"
    | "runtime.interrupt"
    | "runtime.stop"
    | "runtime.resume"
  >;
  label: string;
  enabled: boolean;
  icon: typeof LogIn;
  variant?: "primary" | "secondary" | "ghost" | "danger";
};

export function lifecycleControlStates(
  context: WorkbenchCommandContext,
  pending = false,
): LifecycleControlState[] {
  const controls: LifecycleControlState[] = [
    {
      id: "session.attach",
      label: "Attach",
      enabled: !pending && canRunCommand("session.attach", context),
      icon: LogIn,
    },
    {
      id: "session.detach",
      label: "Detach",
      enabled: !pending && canRunCommand("session.detach", context),
      icon: LogOut,
    },
    {
      id: "runtime.interrupt",
      label: "Interrupt",
      enabled: !pending && canRunCommand("runtime.interrupt", context),
      icon: Pause,
      variant: "danger",
    },
    {
      id: "runtime.stop",
      label: "Stop",
      enabled: !pending && canRunCommand("runtime.stop", context),
      icon: Square,
      variant: "danger",
    },
    {
      id: "runtime.resume",
      label: "Resume",
      enabled: !pending && canRunCommand("runtime.resume", context),
      icon: context.runtime?.resume_supported ? RotateCcw : Play,
    },
  ];
  return controls.filter(
    (control) =>
      control.id !== "runtime.interrupt" ||
      context.runtime?.execution_profile === "managed",
  );
}

function shortPolicyHash(value: string | null | undefined) {
  if (!value) return "snapshot unavailable";
  return value.length > 12 ? `${value.slice(0, 12)}…` : value;
}
