/**
 * Smart mailbox create/edit form with rule builder integration.
 *
 * @spec docs/L1-api#smart-mailbox-crud
 * @spec docs/L1-search#smart-mailbox-data-model
 */
import { useMutation } from '@tanstack/react-query'
import { useState } from 'react'
import { ArrowDown, ArrowUp } from 'lucide-react'
import { createSmartMailbox, updateSmartMailbox } from '../../api/client'
import type {
  CreateSmartMailboxInput,
  SmartMailbox,
  SmartMailboxSummary,
  UpdateSmartMailboxInput,
} from '../../api/types'
import { Button } from '../ui/button'
import { EMPTY_SMART_MAILBOX_FORM, formFromSmartMailbox } from './helpers'
import { RuleGroupEditor } from './RuleGroupEditor'
import { FeedbackBanner, Field, SectionCard, SectionHeader } from './shared'
import type { SmartMailboxEditorTarget } from './types'

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
  editorTarget: SmartMailboxEditorTarget
  editingSmartMailbox: SmartMailbox | SmartMailboxSummary | null
  summary: SmartMailboxSummary | null
  onSaved: (smartMailbox: SmartMailbox) => Promise<void>
  onDeleted: (smartMailboxId: string) => Promise<void>
  onReorder: (mailbox: SmartMailboxSummary, position: number) => void
  reorderPendingKey: string | null
}) {
  const [form, setForm] = useState(() =>
    editingSmartMailbox
      ? formFromSmartMailbox(editingSmartMailbox)
      : EMPTY_SMART_MAILBOX_FORM,
  )
  const [feedback, setFeedback] = useState<string | null>(null)
  const [errorMessage, setErrorMessage] = useState<string | null>(null)

  const saveMutation = useMutation({
    mutationFn: async (currentForm: typeof form) => {
      if (editorTarget === 'new') {
        const payload: CreateSmartMailboxInput = {
          name: currentForm.name.trim(),
          position: currentForm.position,
          rule: currentForm.rule,
        }
        return createSmartMailbox(payload)
      }

      const payload: UpdateSmartMailboxInput = {
        name: currentForm.name.trim(),
        position: currentForm.position,
        rule: currentForm.rule,
      }
      return updateSmartMailbox(editorTarget, payload)
    },
    onSuccess: async (smartMailbox) => {
      setFeedback(`Saved ${smartMailbox.name}.`)
      setErrorMessage(null)
      await onSaved(smartMailbox)
    },
    onError: (error: Error) => {
      setFeedback(null)
      setErrorMessage(error.message)
    },
  })

  const isEditing = editorTarget !== 'new'

  return (
    <div>
      <SectionCard>
        <SectionHeader
          eyebrow="Mailbox editor"
          title={
            editorTarget === 'new'
              ? 'New smart mailbox'
              : (editingSmartMailbox?.name ?? 'Smart mailbox')
          }
          description={
            editorTarget === 'new'
              ? 'A saved message query that powers a virtual mailbox.'
              : 'Saved queries power unified mailboxes and custom filtered views.'
          }
          actions={
            isEditing ? (
              <div className="flex flex-wrap items-center gap-1.5">
                {summary && (
                  <>
                    <Button
                      size="sm"
                      variant="outline"
                      type="button"
                      onClick={() =>
                        onReorder(summary, Math.max(0, summary.position - 1))
                      }
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

      <div>
        <div>
          <SectionCard>
            <SectionHeader eyebrow="Definition" title="Mailbox name" />

            <Field
              label="Name"
              value={form.name}
              placeholder="Important"
              onChange={(value) =>
                setForm((current) => ({ ...current, name: value }))
              }
            />
          </SectionCard>

          <SectionCard>
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
              onChange={(root) =>
                setForm((current) => ({ ...current, rule: { root } }))
              }
            />
          </SectionCard>
        </div>

        <div>
          <SectionCard>
            <SectionHeader
              eyebrow="Changes"
              title="Apply updates"
              description="Save the current smart mailbox or reset the form back to its loaded state."
            />

            {feedback && (
              <FeedbackBanner tone="success">{feedback}</FeedbackBanner>
            )}
            {errorMessage && (
              <FeedbackBanner tone="error">{errorMessage}</FeedbackBanner>
            )}

            <div className="flex flex-wrap gap-1.5">
              <Button
                type="button"
                onClick={() => saveMutation.mutate(form)}
                disabled={saveMutation.isPending}
                className="bg-brand-coral text-white hover:bg-brand-coral/90"
              >
                {editorTarget === 'new' ? 'Create mailbox' : 'Save mailbox'}
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
  )
}
