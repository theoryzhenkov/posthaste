/**
 * Smart mailbox create/edit form with rule builder integration.
 *
 * @spec docs/L1-api#smart-mailbox-crud
 * @spec docs/L1-search#smart-mailbox-data-model
 */
import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
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
import {
  EMPTY_SMART_MAILBOX_FORM,
  formFromSmartMailbox,
} from "./helpers";
import { RuleGroupEditor } from "./RuleGroupEditor";
import { Field } from "./shared";
import type { SmartMailboxEditorTarget } from "./types";
import { Button } from "../ui/button";
import { Input } from "../ui/input";

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
  onSaved,
  onDeleted,
}: {
  editorTarget: SmartMailboxEditorTarget;
  editingSmartMailbox: SmartMailbox | SmartMailboxSummary | null;
  onSaved: (smartMailbox: SmartMailbox) => Promise<void>;
  onDeleted: (smartMailboxId: string) => Promise<void>;
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

  return (
    <section className="rounded-xl border border-border bg-background/70 p-4">
      <p className="text-[10px] font-mono uppercase tracking-[0.24em] text-muted-foreground">
        {editorTarget === "new" ? "new smart mailbox" : "smart mailbox editor"}
      </p>
      <div className="mt-2 flex items-center justify-between gap-3">
        <div>
          <h3 className="text-base font-semibold tracking-tight">
            {editorTarget === "new"
              ? "Create smart mailbox"
              : editingSmartMailbox?.name ?? "Edit smart mailbox"}
          </h3>
          <p className="text-sm text-muted-foreground">
            Saved message queries power unified mailboxes and custom filtered views.
          </p>
        </div>
        {editorTarget !== "new" && (
          <Button
            size="sm"
            variant="destructive"
            type="button"
            onClick={() => onDeleted(editorTarget)}
          >
            Delete mailbox
          </Button>
        )}
      </div>

      <div className="mt-4 grid gap-4">
        <div className="grid grid-cols-2 gap-4">
          <Field
            label="Mailbox name"
            value={form.name}
            placeholder="Important"
            onChange={(value) => setForm((current) => ({ ...current, name: value }))}
          />
          <label className="grid gap-1.5 text-sm">
            <span className="text-muted-foreground">Position</span>
            <Input
              type="number"
              className="h-9 bg-card"
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

        <div className="rounded-lg border border-border bg-card/60 p-3">
          <div className="flex items-center justify-between gap-3">
            <div>
              <p className="text-sm font-medium">Rule builder</p>
              <p className="text-xs text-muted-foreground">
                Smart mailboxes match individual messages, not whole threads.
              </p>
            </div>
            <Button
              size="xs"
              variant="outline"
              type="button"
              onClick={() => setForm(EMPTY_SMART_MAILBOX_FORM)}
            >
              Reset rule
            </Button>
          </div>

          <div className="mt-3">
            <RuleGroupEditor
              group={form.rule.root}
              onChange={(root) => setForm((current) => ({ ...current, rule: { root } }))}
            />
          </div>
        </div>

        {feedback && (
          <p className="rounded border border-emerald-500/20 bg-emerald-500/5 px-3 py-2 text-sm text-emerald-700">
            {feedback}
          </p>
        )}
        {errorMessage && (
          <p className="rounded border border-destructive/20 bg-destructive/5 px-3 py-2 text-sm text-destructive">
            {errorMessage}
          </p>
        )}

        <div className="flex flex-wrap gap-2">
          <Button type="button" onClick={() => saveMutation.mutate(form)} disabled={saveMutation.isPending}>
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
      </div>
    </section>
  );
}
