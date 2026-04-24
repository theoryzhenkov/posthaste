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
  AccountOverview,
  CreateSmartMailboxInput,
  SmartMailbox,
  SmartMailboxSummary,
  UpdateSmartMailboxInput,
} from '../../api/types'
import { Button } from '../ui/button'
import { SmartMailboxAutomationFields } from './AutomationActionsEditor'
import { EMPTY_SMART_MAILBOX_FORM, formFromSmartMailbox } from './helpers'
import { RuleGroupEditor } from './RuleGroupEditor'
import {
  FeedbackBanner,
  Field,
  SettingsFooter,
  SettingsPageHeader,
  SettingsSection,
} from './shared'
import type { SmartMailboxEditorTarget } from './types'

function smartMailboxFieldsSignature(form: {
  name: string
  position: number
  rule: unknown
}): string {
  return JSON.stringify({
    name: form.name.trim(),
    position: form.position,
    rule: form.rule,
  })
}

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
  accounts,
  onSaved,
  onAutomationAccountsSaved,
  onDeleted,
  onReorder,
  reorderPendingKey,
}: {
  editorTarget: SmartMailboxEditorTarget
  editingSmartMailbox: SmartMailbox | SmartMailboxSummary | null
  summary: SmartMailboxSummary | null
  accounts: AccountOverview[]
  onSaved: (smartMailbox: SmartMailbox) => Promise<void>
  onAutomationAccountsSaved: (accounts: AccountOverview[]) => Promise<void>
  onDeleted: (smartMailboxId: string) => Promise<void>
  onReorder: (mailbox: SmartMailboxSummary, position: number) => void
  reorderPendingKey: string | null
}) {
  const [form, setForm] = useState(() =>
    editingSmartMailbox
      ? formFromSmartMailbox(editingSmartMailbox)
      : EMPTY_SMART_MAILBOX_FORM,
  )
  const [errorMessage, setErrorMessage] = useState<string | null>(null)
  const [
    savedSmartMailboxFieldsSignature,
    setSavedSmartMailboxFieldsSignature,
  ] = useState(() => smartMailboxFieldsSignature(form))

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
      setErrorMessage(null)
      const savedForm = formFromSmartMailbox(smartMailbox)
      setSavedSmartMailboxFieldsSignature(
        smartMailboxFieldsSignature(savedForm),
      )
      setForm(savedForm)
      await onSaved(smartMailbox)
    },
    onError: (error: Error) => {
      setErrorMessage(error.message)
    },
  })

  const isEditing = editorTarget !== 'new'
  const hasUnsavedChanges =
    smartMailboxFieldsSignature(form) !== savedSmartMailboxFieldsSignature

  return (
    <div className="pb-8">
      <SettingsPageHeader
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
          isEditing && summary ? (
            <>
              <Button
                size="sm"
                variant="ghost"
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
                variant="ghost"
                type="button"
                onClick={() => onReorder(summary, summary.position + 1)}
                disabled={reorderPendingKey !== null}
                aria-label="Move down"
              >
                <ArrowDown size={14} strokeWidth={1.75} />
              </Button>
            </>
          ) : null
        }
      />

      <SettingsSection title="Definition">
        <Field
          label="Name"
          value={form.name}
          placeholder="Important"
          onChange={(value) =>
            setForm((current) => ({ ...current, name: value }))
          }
        />
      </SettingsSection>

      <SettingsSection
        title="Rules"
        actions={
          <Button
            size="sm"
            variant="outline"
            type="button"
            onClick={() =>
              setForm((current) => ({
                ...current,
                rule: EMPTY_SMART_MAILBOX_FORM.rule,
              }))
            }
          >
            Reset rule
          </Button>
        }
      >
        <RuleGroupEditor
          group={form.rule.root}
          onChange={(root) =>
            setForm((current) => ({ ...current, rule: { root } }))
          }
        />
      </SettingsSection>

      {editorTarget !== 'new' &&
        editingSmartMailbox &&
        'rule' in editingSmartMailbox && (
          <SettingsSection title="Actions">
            <SmartMailboxAutomationFields
              accounts={accounts}
              smartMailbox={editingSmartMailbox}
              disabledReason={
                hasUnsavedChanges
                  ? 'Save mailbox definition before applying actions'
                  : null
              }
              onSaved={onAutomationAccountsSaved}
            />
          </SettingsSection>
        )}

      <SettingsFooter>
        {errorMessage && (
          <FeedbackBanner tone="error">{errorMessage}</FeedbackBanner>
        )}

        <div className="flex flex-wrap items-center gap-2">
          <Button
            type="button"
            onClick={() => saveMutation.mutate(form)}
            disabled={saveMutation.isPending || !hasUnsavedChanges}
            className="bg-brand-coral text-white hover:bg-brand-coral/90"
          >
            {editorTarget === 'new' ? 'Create mailbox' : 'Save mailbox'}
          </Button>
          <span className="text-[12px] text-muted-foreground">
            {hasUnsavedChanges ? 'Unsaved changes' : 'Saved'}
          </span>
        </div>
      </SettingsFooter>

      {isEditing && (
        <SettingsSection title="Danger" tone="danger" className="pt-16">
          <p className="mb-3 text-[12px] text-muted-foreground">
            Delete this smart mailbox. Messages remain in their source accounts.
          </p>
          <Button
            size="sm"
            variant="destructive"
            type="button"
            onClick={() => onDeleted(editorTarget)}
          >
            Delete
          </Button>
        </SettingsSection>
      )}
    </div>
  )
}
