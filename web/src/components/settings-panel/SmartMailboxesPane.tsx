/**
 * Smart mailboxes view: slim list paired with the mailbox editor.
 *
 * @spec docs/L1-api#smart-mailbox-crud
 */
import { FolderSearch, Plus } from "lucide-react";
import type { SmartMailbox, SmartMailboxSummary } from "../../api/types";
import { cn } from "../../lib/utils";
import { Button } from "../ui/button";
import { SmartMailboxEditor } from "./SmartMailboxEditor";
import { SectionHeader } from "./shared";
import type { SmartMailboxEditorTarget } from "./types";

export function SmartMailboxesPane({
  smartMailboxes,
  selectedMailboxId,
  editingSmartMailbox,
  editorKey,
  actionPendingKey,
  actionError,
  onSelectMailbox,
  onCreateMailbox,
  onResetDefaults,
  onReorderMailbox,
  onSaved,
  onDeleted,
}: {
  smartMailboxes: SmartMailboxSummary[];
  selectedMailboxId: SmartMailboxEditorTarget;
  editingSmartMailbox: SmartMailbox | SmartMailboxSummary | null;
  editorKey: string;
  actionPendingKey: string | null;
  actionError: string | null;
  onSelectMailbox: (mailboxId: string) => void;
  onCreateMailbox: () => void;
  onResetDefaults: () => void;
  onReorderMailbox: (mailbox: SmartMailboxSummary, position: number) => void;
  onSaved: (mailbox: SmartMailbox) => Promise<void>;
  onDeleted: (mailboxId: string) => Promise<void>;
}) {
  const selectedMailbox =
    selectedMailboxId === "new"
      ? null
      : smartMailboxes.find((mailbox) => mailbox.id === selectedMailboxId) ?? null;

  return (
    <div className="grid h-full min-h-0 gap-5 px-5 py-5 lg:grid-cols-[18rem_minmax(0,1fr)] lg:px-6 lg:py-6">
      <aside className="flex min-h-0 flex-col overflow-hidden rounded-lg border border-border/80 bg-background/78 shadow-[var(--shadow-pane)]">
        <header className="border-b border-border/80 px-4 py-4">
          <SectionHeader
            eyebrow="Smart mailboxes"
            title="Saved views"
            description="Virtual mailboxes powered by message-level rules."
            actions={
              <Button
                size="sm"
                variant="outline"
                type="button"
                onClick={onCreateMailbox}
                className="h-8"
                aria-label="New smart mailbox"
              >
                <Plus size={13} strokeWidth={2} />
                New
              </Button>
            }
          />
        </header>
        <div className="ph-scroll min-h-0 flex-1 overflow-y-auto p-2">
          {smartMailboxes.length === 0 && selectedMailboxId !== "new" && (
            <p className="px-4 py-8 text-center text-xs leading-5 text-muted-foreground">
              No smart mailboxes.
            </p>
          )}
          {selectedMailboxId === "new" && (
            <MailboxListRow
              isActive
              label="New mailbox"
              sublabel="Unsaved"
              onClick={() => undefined}
            />
          )}
          {smartMailboxes.map((mailbox) => (
            <MailboxListRow
              key={mailbox.id}
              isActive={selectedMailboxId === mailbox.id}
              label={mailbox.name}
              sublabel={`${mailbox.totalMessages} · ${mailbox.unreadMessages} unread`}
              isDefault={mailbox.kind === "default"}
              onClick={() => onSelectMailbox(mailbox.id)}
            />
          ))}
        </div>
        <footer className="border-t border-border/80 p-2">
          <Button
            size="sm"
            variant="outline"
            type="button"
            className="w-full justify-start"
            onClick={onResetDefaults}
            disabled={actionPendingKey !== null}
          >
            Reset to defaults
          </Button>
        </footer>
      </aside>

      <section className="ph-scroll min-h-0 overflow-y-auto rounded-lg border border-border/80 bg-background/78 shadow-[var(--shadow-pane)]">
        <div className="px-5 py-5 sm:px-6 sm:py-6">
        {actionError && (
          <p className="mb-4 rounded-lg border border-destructive/20 bg-destructive/5 px-3.5 py-2.5 text-sm text-destructive shadow-[var(--shadow-pane)]">
            {actionError}
          </p>
        )}
        {selectedMailboxId === "new" || editingSmartMailbox ? (
          <SmartMailboxEditor
            key={editorKey}
            editorTarget={selectedMailboxId}
            editingSmartMailbox={editingSmartMailbox}
            summary={selectedMailbox}
            onSaved={onSaved}
            onDeleted={onDeleted}
            onReorder={onReorderMailbox}
            reorderPendingKey={actionPendingKey}
          />
        ) : (
          <SmartMailboxesEmptyState onCreateMailbox={onCreateMailbox} />
        )}
        </div>
      </section>
    </div>
  );
}

function MailboxListRow({
  isActive,
  label,
  sublabel,
  isDefault,
  onClick,
}: {
  isActive: boolean;
  label: string;
  sublabel?: string;
  isDefault?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex w-full items-center gap-3 rounded-lg border px-3 py-3 text-left transition-all",
        isActive
          ? "border-primary/25 bg-accent/80 text-accent-foreground shadow-[var(--shadow-pane)]"
          : "border-transparent hover:border-border/80 hover:bg-panel-muted/70",
      )}
    >
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-1.5">
          <span className="truncate text-sm font-medium">{label}</span>
          {isDefault && (
            <span
              className="shrink-0 rounded-sm bg-background/80 px-1.5 py-0.5 text-[9px] font-mono uppercase tracking-[0.18em] text-muted-foreground"
              title="Built-in smart mailbox"
            >
              default
            </span>
          )}
        </span>
        {sublabel && (
          <span className="block truncate text-xs text-muted-foreground">
            {sublabel}
          </span>
        )}
      </span>
    </button>
  );
}

function SmartMailboxesEmptyState({
  onCreateMailbox,
}: {
  onCreateMailbox: () => void;
}) {
  return (
    <div className="flex h-full min-h-[260px] flex-col items-center justify-center rounded-lg border border-dashed border-border/80 bg-panel-muted/40 px-6 text-center">
      <FolderSearch size={36} strokeWidth={1.5} className="text-muted-foreground/40" />
      <div className="mt-4">
        <p className="text-sm font-medium">No mailbox selected</p>
        <p className="mt-1 text-sm text-muted-foreground">
          Pick a mailbox on the left, or create a new one.
        </p>
      </div>
      <Button
        size="sm"
        variant="outline"
        type="button"
        onClick={onCreateMailbox}
        className="mt-4"
      >
        <Plus size={13} strokeWidth={2} />
        New mailbox
      </Button>
    </div>
  );
}
