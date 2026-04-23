/**
 * Smart mailbox create/edit form with rule builder integration.
 *
 * @spec docs/L1-api#smart-mailbox-crud
 * @spec docs/L1-search#smart-mailbox-data-model
 */
import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { ArrowDown, ArrowUp } from "lucide-react";
import {
  createSmartMailbox,
  updateSmartMailbox,
} from "../../api/client";
import type {
  CreateSmartMailboxInput,
  SmartMailbox,
  SmartMailboxSummary,
  UpdateSmartMailboxInput,
} from "../../api/types";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import {
  EMPTY_SMART_MAILBOX_FORM,
  formFromSmartMailbox,
} from "./helpers";
import { RuleGroupEditor } from "./RuleGroupEditor";
import {
  FeedbackBanner,
  Field,
  MetaStat,
  SectionCard,
  SectionHeader,
} from "./shared";
import type { SmartMailboxEditorTarget } from "./types";

/**
 * Smart mailbox editor form: create new or edit existing smart mailboxes.
 *
 * Embeds the recursive `RuleGroupEditor` for building filter rules.
 *
 * @spec docs/L1-api#smart-mailbox-crud
 * @spec docs/L1-search#smart-mailbox-data-model
 */
export function SmartMailboxEditor({
  editorTarget,
  editingSmartMailbox,
  summary,
  onSaved,
  onDeleted,
  onReorder,
  reorderPendingKey,
}: {
  editorTarget: SmartMailboxEditorTarget;
  editingSmartMailbox: SmartMailbox | SmartMailboxSummary | null;
  summary: SmartMailboxSummary | null;
  onSaved: (smartMailbox: SmartMailbox) => Promise<void>;
  onDeleted: (smartMailboxId: string) => Promise<void>;
  onReorder: (mailbox: SmartMailboxSummary, position: number) => void;
  reorderPendingKey: string | null;
}) {
  const [form, setForm] = useState(() =>
    editingSmartMailbox ? formFromSmartMailbox(editingSmartMailbox) : EMPTY_SMART_MAILBOX_FORM,
  );
  const [feedback, setFeedback] = useState<string | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  const saveMutation = useMutation({
    mutationFn: async (currentForm: typeof form) => {
      if (editorTarget === "new") {
        const payload: CreateSmartMailboxInput = {
          name: currentForm.name.trim(),
          position: currentForm.position,
          rule: currentForm.rule,
        };
        return createSmartMailbox(payload);
      }

      const payload: UpdateSmartMailboxInput = {
        name: currentForm.name.trim(),
        position: currentForm.position,
        rule: currentForm.rule,
      };
      return updateSmartMailbox(editorTarget, payload);
    },
    onSuccess: async (smartMailbox) => {
      setFeedback(`Saved ${smartMailbox.name}.`);
      setErrorMessage(null);
      await onSaved(smartMailbox);
    },
    onError: (error: Error) => {
      setFeedback(null);
      setErrorMessage(error.message);
    },
  });

  const isEditing = editorTarget !== "new";

  return (
    <div className="space-y-5">
      <SectionCard className="space-y-4">
        <SectionHeader
          eyebrow="Mailbox editor"
          title={
            editorTarget === "new"
              ? "New smart mailbox"
              : editingSmartMailbox?.name ?? "Smart mailbox"
          }
          description={
            editorTarget === "new"
              ? "A saved message query that powers a virtual mailbox."
              : "Saved queries power unified mailboxes and custom filtered views."
          }
          actions={
            isEditing ? (
              <div className="flex flex-wrap items-center gap-2">
                {summary && (
                  <>
                    <Button
                      size="sm"
                      variant="outline"
                      type="button"
                      onClick={() => onReorder(summary, Math.max(0, summary.position - 1))}
                      disabled={reorderPendingKey !== null}
                      aria-label="Move up"
                    >
                      <ArrowUp size={14} strokeWidth={1.75} />
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      type="button"
                      onClick={() => onReorder(summary, summary.position + 1)}
                      disabled={reorderPendingKey !== null}
                      aria-label="Move down"
                    >
                      <ArrowDown size={14} strokeWidth={1.75} />
                    </Button>
                  </>
                )}
                <Button
                  size="sm"
                  variant="destructive"
                  type="button"
                  onClick={() => onDeleted(editorTarget)}
                >
                  Delete
                </Button>
              </div>
            ) : null
          }
        />
      </SectionCard>

      <div className="grid gap-4 xl:grid-cols-[minmax(0,1.35fr)_17rem]">
        <div className="space-y-4">
          <SectionCard className="space-y-4">
            <SectionHeader
              eyebrow="Definition"
              title="Mailbox metadata"
              description="Name the mailbox and choose where it lands in the saved-view order."
            />

            <div className="grid gap-4 sm:grid-cols-[minmax(0,1fr)_8.5rem]">
              <Field
                label="Mailbox name"
                value={form.name}
                placeholder="Important"
                onChange={(value) => setForm((current) => ({ ...current, name: value }))}
              />
              <label className="grid gap-2 text-sm">
                <span className="text-[11px] font-medium text-muted-foreground">
                  Position
                </span>
                <Input
                  type="number"
                  className="h-9 border-border/80 bg-panel shadow-none"
                  value={form.position}
                  onChange={(event) =>
                    setForm((current) => ({
                      ...current,
                      position: Number(event.target.value) || 0,
                    }))
                  }
                />
              </label>
            </div>
          </SectionCard>

          <SectionCard className="space-y-4">
            <SectionHeader
              eyebrow="Rules"
              title="Rule builder"
              description="Smart mailboxes match individual messages, not whole threads."
              actions={
                <Button
                  size="sm"
                  variant="outline"
                  type="button"
                  onClick={() => setForm(EMPTY_SMART_MAILBOX_FORM)}
                >
                  Reset rule
                </Button>
              }
            />

            <RuleGroupEditor
              group={form.rule.root}
              onChange={(root) => setForm((current) => ({ ...current, rule: { root } }))}
            />
          </SectionCard>
        </div>

        <div className="space-y-4">
          <SectionCard className="space-y-4">
            <SectionHeader
              eyebrow="State"
              title="Mailbox summary"
              description="Counts and placement for the currently selected saved view."
            />

            {summary ? (
              <dl className="grid grid-cols-2 gap-4 rounded-lg border border-border/70 bg-panel-muted/45 px-4 py-4">
                <MetaStat label="Unread" value={String(summary.unreadMessages)} />
                <MetaStat label="Total" value={String(summary.totalMessages)} />
                <MetaStat label="Position" value={String(summary.position)} />
                <MetaStat
                  label="Kind"
                  value={summary.kind === "default" ? "default" : "custom"}
                />
              </dl>
            ) : (
              <p className="rounded-lg border border-dashed border-border/80 bg-panel-muted/35 px-4 py-4 text-sm text-muted-foreground">
                This mailbox will appear here once it has been saved.
              </p>
            )}
          </SectionCard>

          <SectionCard className="space-y-4">
            <SectionHeader
              eyebrow="Changes"
              title="Apply updates"
              description="Save the current smart mailbox or reset the form back to its loaded state."
            />

            {feedback && <FeedbackBanner tone="success">{feedback}</FeedbackBanner>}
            {errorMessage && <FeedbackBanner tone="error">{errorMessage}</FeedbackBanner>}

            <div className="flex flex-wrap gap-2">
              <Button
                type="button"
                onClick={() => saveMutation.mutate(form)}
                disabled={saveMutation.isPending}
              >
                {editorTarget === "new" ? "Create mailbox" : "Save mailbox"}
              </Button>
              <Button
                type="button"
                variant="outline"
                onClick={() =>
                  setForm(
                    editingSmartMailbox
                      ? formFromSmartMailbox(editingSmartMailbox)
                      : EMPTY_SMART_MAILBOX_FORM,
                  )
                }
              >
                Reset form
              </Button>
            </div>
          </SectionCard>
        </div>
      </div>
    </div>
  );
}
