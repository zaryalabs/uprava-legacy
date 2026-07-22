import { Send } from "lucide-react";
import { useEffect, useState } from "react";

import { Button } from "../../shared/ui/button";
import { Textarea } from "../../shared/ui/textarea";

type Props = {
  pending: boolean;
  disabled?: boolean;
  supportsPlanMode?: boolean;
  onSend: (
    content: string,
    collaborationMode: "default" | "plan",
  ) => Promise<void> | void;
};

export function ChatComposer({
  pending,
  disabled = false,
  supportsPlanMode = false,
  onSend,
}: Props) {
  const [content, setContent] = useState("");
  const [collaborationMode, setCollaborationMode] = useState<
    "default" | "plan"
  >("default");

  useEffect(() => {
    if (!content.trim()) return;
    const warnBeforeUnload = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", warnBeforeUnload);
    return () => window.removeEventListener("beforeunload", warnBeforeUnload);
  }, [content]);

  return (
    <form
      className="border-t border-[var(--color-border)] pt-4"
      onSubmit={(event) => {
        event.preventDefault();
        const trimmed = content.trim();
        if (disabled || !trimmed) return;
        void Promise.resolve(onSend(trimmed, collaborationMode))
          .then(() => {
            setContent("");
          })
          .catch(() => {});
      }}
    >
      <div className="mb-2 flex flex-wrap items-baseline justify-between gap-2">
        <label htmlFor="session-turn" className="text-sm font-bold">
          Next Agent Turn
        </label>
        <span className="text-xs text-[var(--color-muted)]">
          {disabled
            ? "Runtime cannot accept a turn in its current state."
            : "Draft stays until the turn is accepted."}
        </span>
      </div>
      <Textarea
        id="session-turn"
        name="session-turn"
        autoComplete="off"
        value={content}
        onChange={(event) => setContent(event.target.value)}
        placeholder="Send a turn"
        disabled={disabled}
      />
      <div className="mt-2 flex flex-wrap items-center justify-between gap-3">
        <span
          className="text-xs text-[var(--color-muted)]"
          role="status"
          aria-live="polite"
        >
          {pending
            ? "Sending turn…"
            : content.trim()
              ? "Draft not sent"
              : "Ready"}
        </span>
        <div className="flex items-center gap-2">
          {supportsPlanMode ? (
            <label className="flex items-center gap-2 text-xs font-medium">
              Mode
              <select
                aria-label="Turn collaboration mode"
                className="h-9 border border-[var(--color-muted)] bg-[var(--color-bg)] px-2 text-sm"
                value={collaborationMode}
                disabled={disabled || pending}
                onChange={(event) =>
                  setCollaborationMode(
                    event.target.value === "plan" ? "plan" : "default",
                  )
                }
              >
                <option value="default">Default</option>
                <option value="plan">Plan (can ask questions)</option>
              </select>
            </label>
          ) : null}
          <Button
            type="submit"
            variant="primary"
            disabled={disabled || pending || !content.trim()}
          >
            <Send size={15} aria-hidden="true" />
            {pending ? "Sending…" : "Send Turn"}
          </Button>
        </div>
      </div>
    </form>
  );
}
