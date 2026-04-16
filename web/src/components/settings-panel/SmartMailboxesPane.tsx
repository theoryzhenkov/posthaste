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
    <div className="grid h-full min-h-0 grid-cols-[220px_minmax(0,1fr)]">
      <aside className="flex min-h-0 flex-col border-r border-border bg-background/30">
        <header className="flex items-center justify-between gap-2 border-b border-border px-3 py-2">
          <span className="text-[10px] font-mono uppercase tracking-[0.2em] text-muted-foreground">
            mailboxes
          </span>
          <Button
            size="xs"
            variant="ghost"
            type="button"
            onClick={onCreateMailbox}
            className="-mr-1 h-6 gap-1 px-1.5 text-xs"
            aria-label="New smart mailbox"
          >
            <Plus size={13} strokeWidth={2} />
            New
          </Button>
        </header>
        <div className="min-h-0 flex-1 overflow-y-auto py-1">
          {smartMailboxes.length === 0 && selectedMailboxId !== "new" && (
            <p className="px-3 py-6 text-center text-xs text-muted-foreground">
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
        <footer className="border-t border-border p-2">
          <Button
            size="xs"
            variant="ghost"
            type="button"
            className="w-full justify-start text-xs text-muted-foreground"
            onClick={onResetDefaults}
            disabled={actionPendingKey !== null}
          >
            Reset to defaults
          </Button>
        </footer>
      </aside>

      <div className="min-h-0 overflow-y-auto px-6 py-6">
        {actionError && (
          <p className="mb-4 rounded border border-destructive/20 bg-destructive/5 px-3 py-2 text-sm text-destructive">
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
        "flex w-full items-center gap-2.5 px-3 py-2 text-left transition-colors",
        isActive
          ? "bg-accent text-accent-foreground"
          : "hover:bg-accent/50",
      )}
    >
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-1.5">
          <span className="truncate text-sm font-medium">{label}</span>
          {isDefault && (
            <span
              className="shrink-0 text-[9px] font-mono uppercase tracking-wider text-muted-foreground"
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
    <div className="flex h-full min-h-[200px] flex-col items-center justify-center gap-4 text-center">
      <FolderSearch size={36} strokeWidth={1.5} className="text-muted-foreground/40" />
      <div>
        <p className="text-sm font-medium">No mailbox selected</p>
        <p className="mt-1 text-xs text-muted-foreground">
          Pick a mailbox on the left, or create a new one.
        </p>
      </div>
      <Button size="sm" variant="outline" type="button" onClick={onCreateMailbox}>
        <Plus size={13} strokeWidth={2} />
        New mailbox
      </Button>
    </div>
  );
}
