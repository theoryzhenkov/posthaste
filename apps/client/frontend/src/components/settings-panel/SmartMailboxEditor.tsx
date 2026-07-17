/**
 * Smart mailbox create/edit form with rule builder integration.
 *
 */
import type { AccountRow, SmartMailboxesResult } from '@/gen'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import type {
  AppSettings,
  SmartMailbox,
  SmartMailboxSummary,
} from '../../api/types'
import { useMailClient } from '@/data/context'
import { fetchQuery } from '@/data/queries'
import { runCommand } from '@/data/commands'
import { ASSIGNABLE_MAILBOX_ROLES } from '../../domainVocabulary'
import { Button } from '../ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../ui/select'
import { SmartMailboxAutomationFields } from './AutomationActionsEditor'
import { EMPTY_SMART_MAILBOX_FORM, formFromSmartMailbox } from './helpers'
import { RuleGroupEditor } from './RuleGroupEditor'
import { ConditionEditorContext } from './rule-group/conditionEditorContext'
import {
  FeedbackBanner,
  Field,
  SettingsFooter,
  SettingsPageHeader,
  SettingsSection,
} from './shared'
import type { SmartMailboxEditorTarget } from './types'

const NO_ROLE = '__none__'

function roleLabel(role: string): string {
  return role.charAt(0).toUpperCase() + role.slice(1)
}

function smartMailboxFieldsSignature(form: {
  name: string
  role: string | null
  rule: unknown
}): string {
  return JSON.stringify({
    name: form.name.trim(),
    role: form.role,
    rule: form.rule,
  })
}

/**
 * Smart mailbox editor form: create new or edit existing smart mailboxes.
 *
 * Embeds the recursive `RuleGroupEditor` for building filter rules.
 */
export function SmartMailboxEditor({
  editorTarget,
  editingSmartMailbox,
  accounts,
  settings,
  onSaved,
  onAutomationSettingsSaved,
  onDeleted,
}: {
  editorTarget: SmartMailboxEditorTarget
  editingSmartMailbox: SmartMailbox | SmartMailboxSummary | null
  accounts: AccountRow[]
  settings: AppSettings | null
  onSaved: (smartMailbox: SmartMailbox) => Promise<void>
  onAutomationSettingsSaved: (settings: AppSettings) => Promise<void>
  onDeleted: (smartMailboxId: string) => Promise<void>
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

  const client = useMailClient()
  const queryClient = useQueryClient()
  const saveMutation = useMutation({
    // The commands return only an acceptance; the saved row is read back from
    // the next `smartMailboxes` answer (the backend mints ids on create, so a
    // fresh creation is located by name among user mailboxes, newest first).
    mutationFn: async (currentForm: typeof form): Promise<SmartMailbox> => {
      const name = currentForm.name.trim()
      if (editorTarget === 'new') {
        await runCommand(client, queryClient, {
          createSmartMailbox: {
            name,
            role: currentForm.role,
            rule: currentForm.rule,
          },
        })
      } else {
        await runCommand(client, queryClient, {
          updateSmartMailbox: {
            smartMailboxId: editorTarget,
            name,
            // Empty string clears the role; a value sets it.
            role: currentForm.role ?? '',
            rule: currentForm.rule,
          },
        })
      }
      const { rows } = await fetchQuery<SmartMailboxesResult>(client, {
        smartMailboxes: {},
      })
      const saved =
        editorTarget === 'new'
          ? rows
              .filter((row) => row.kind === 'user' && row.name === name)
              .sort((a, b) => b.createdAt.localeCompare(a.createdAt))[0]
          : rows.find((row) => row.id === editorTarget)
      if (!saved) {
        throw new Error('The saved smart mailbox could not be read back.')
      }
      return saved
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
        <div className="grid gap-3 sm:grid-cols-[1fr_280px] sm:items-center">
          <div className="min-w-0">
            <p className="text-[13px] font-medium text-foreground">Role</p>
            <p className="mt-1 text-[12px] leading-5 text-muted-foreground">
              Give this view a built-in role's icon, color, and contextual
              actions. The rule still decides which messages appear.
            </p>
          </div>
          <Select
            value={form.role ?? NO_ROLE}
            onValueChange={(value) =>
              setForm((current) => ({
                ...current,
                role: value === NO_ROLE ? null : value,
              }))
            }
          >
            <SelectTrigger className="h-8 w-full rounded-md border-border bg-background text-[13px] shadow-none">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={NO_ROLE}>No role</SelectItem>
              {ASSIGNABLE_MAILBOX_ROLES.map((role) => (
                <SelectItem key={role} value={role}>
                  {roleLabel(role)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </SettingsSection>

      <SettingsSection title="Rules">
        <ConditionEditorContext.Provider
          value={{ accountId: '', mailboxes: null, accounts }}
        >
          <RuleGroupEditor
            group={form.rule.root}
            onChange={(root) =>
              setForm((current) => ({ ...current, rule: { root } }))
            }
          />
        </ConditionEditorContext.Provider>
      </SettingsSection>

      {editorTarget !== 'new' &&
        editingSmartMailbox &&
        'rule' in editingSmartMailbox &&
        settings && (
          <SettingsSection title="Actions">
            <SmartMailboxAutomationFields
              accounts={accounts}
              settings={settings}
              smartMailbox={editingSmartMailbox}
              disabledReason={
                hasUnsavedChanges
                  ? 'Save mailbox definition before applying actions'
                  : null
              }
              onSaved={onAutomationSettingsSaved}
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
