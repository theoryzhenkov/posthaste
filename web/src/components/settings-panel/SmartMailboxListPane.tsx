/**
 * Left-pane smart mailbox list with reorder and reset-defaults controls.
 *
 * @spec docs/L1-api#smart-mailbox-crud
 */
import { FolderSearch } from "lucide-react";
import type { SmartMailboxSummary } from "../../api/types";
import { cn } from "../../lib/utils";
import { Button } from "../ui/button";

/**
 * Smart mailbox list pane: shows all saved smart mailboxes with reorder buttons.
 *
 * @spec docs/L1-api#smart-mailbox-crud
 */
export function SmartMailboxListPane({
  smartMailboxSummaries,
  selectedSmartMailboxId,
  smartMailboxActionPendingKey,
  smartMailboxActionError,
  onResetDefaults,
  onCreateMailbox,
  onSelectMailbox,
  onReorderMailbox,
}: {
  smartMailboxSummaries: SmartMailboxSummary[];
  selectedSmartMailboxId: string | "new";
  smartMailboxActionPendingKey: string | null;
  smartMailboxActionError: string | null;
  onResetDefaults: () => void;
  onCreateMailbox: () => void;
  onSelectMailbox: (smartMailboxId: string) => void;
  onReorderMailbox: (smartMailbox: SmartMailboxSummary, position: number) => void;
}) {
  return (
    <section className="border-t border-border px-4 py-4">
      <div className="flex items-center justify-between gap-3">
        <div>
          <p className="text-[10px] font-mono uppercase tracking-[0.24em] text-muted-foreground">
            smart mailboxes
          </p>
          <p className="mt-1 text-sm text-muted-foreground">
            Unified defaults and saved message queries.
          </p>
        </div>
        <div className="flex gap-2">
          <Button
            size="xs"
            variant="outline"
            type="button"
            onClick={onResetDefaults}
            disabled={smartMailboxActionPendingKey !== null}
          >
            Reset defaults
          </Button>
          <Button
            size="xs"
            variant="outline"
            type="button"
            onClick={onCreateMailbox}
          >
            New mailbox
          </Button>
        </div>
      </div>

      <div className="mt-3 space-y-3">
        {smartMailboxActionError && (
          <p className="rounded border border-destructive/20 bg-destructive/5 px-3 py-2 text-sm text-destructive">
            {smartMailboxActionError}
          </p>
        )}
        {smartMailboxSummaries.length === 0 && (
          <div className="flex flex-col items-center gap-3 rounded-lg border border-dashed border-border px-4 py-8">
            <FolderSearch size={32} strokeWidth={1.5} className="text-muted-foreground/40" />
            <div className="text-center">
              <p className="text-sm font-medium text-muted-foreground">No smart mailboxes yet</p>
              <p className="mt-1 text-xs text-muted-foreground/60">
                Create one to organize your mail
              </p>
            </div>
          </div>
        )}
        {smartMailboxSummaries.map((smartMailbox) => (
          <article
            key={smartMailbox.id}
            className={cn(
              "rounded-lg border border-border bg-background/60 px-3 py-3",
              selectedSmartMailboxId === smartMailbox.id &&
                "border-primary/60 shadow-[0_0_0_1px_rgba(37,99,235,0.25)]",
            )}
          >
            <div className="flex items-start justify-between gap-3">
              <button
                type="button"
                className="min-w-0 text-left"
                onClick={() => onSelectMailbox(smartMailbox.id)}
              >
                <div className="flex items-center gap-2">
                  <span className="truncate text-sm font-medium">{smartMailbox.name}</span>
                  {smartMailbox.kind === "default" && (
                    <span className="rounded border border-primary/40 bg-primary/10 px-1.5 py-0.5 text-[10px] font-mono uppercase tracking-wider text-primary">
                      default
                    </span>
                  )}
                </div>
                <p className="mt-1 text-xs text-muted-foreground">
                  {smartMailbox.totalMessages} messages · {smartMailbox.unreadMessages} unread
                </p>
              </button>
              <div className="flex gap-2">
                <Button
                  size="xs"
                  variant="outline"
                  type="button"
                  onClick={() =>
                    onReorderMailbox(smartMailbox, Math.max(0, smartMailbox.position - 1))
                  }
                  disabled={smartMailboxActionPendingKey !== null}
                >
                  Up
                </Button>
                <Button
                  size="xs"
                  variant="outline"
                  type="button"
                  onClick={() => onReorderMailbox(smartMailbox, smartMailbox.position + 1)}
                  disabled={smartMailboxActionPendingKey !== null}
                >
                  Down
                </Button>
              </div>
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}
