import { useMutation } from '@tanstack/react-query'
import type React from 'react'
import type {
  AccountOverview,
  Mailbox,
  MailQueryRule,
} from '../../../api/types'
import type { AutomationRuleDraft } from '../../../automationRules'
import { runtimeMutations } from '../../../runtime/mutations'
import { Button } from '../../ui/button'
import { Checkbox } from '../../ui/checkbox'
import { SelectItem } from '../../ui/select'
import {
  isDraftComplete,
  parseTrigger,
  TRIGGER_OPTIONS,
  type AutomationRuleState,
} from '../automationRuleHelpers'
import { RuleGroupEditor } from '../RuleGroupEditor'
import { ConditionEditorContext } from '../rule-group/conditionEditorContext'
import { Field, SettingsBackButton } from '../shared'
import { ActionListEditor, LabeledSelect } from './ActionListEditor'
import { AutomationRulePreview } from './AutomationRulePreview'

export function AutomationRuleEditor({
  draft,
  state,
  accounts,
  staticMailboxes,
  canEditAccount,
  previewCondition,
  savePending,
  onBack,
  onSave,
  onChange,
  onRemove,
  onBackfill,
  backfillNoticeFor,
}: {
  draft: AutomationRuleDraft
  state: AutomationRuleState
  accounts: AccountOverview[]
  staticMailboxes: Mailbox[] | null
  canEditAccount: boolean
  previewCondition: MailQueryRule
  savePending: boolean
  onBack: () => void
  onSave: () => void
  onChange: (patch: Partial<AutomationRuleDraft>) => void
  onRemove: () => void
  onBackfill: () => void
  /**
   * Rule id whose backfill request was accepted by the server, or null. The
   * list owns the mutation, so this reflects actual success (not the click) and
   * is keyed by rule id so the note only shows for the relevant rule.
   */
  backfillNoticeFor: string | null
}) {
  const previewKey = JSON.stringify(previewCondition)
  const previewMutation = useMutation({
    mutationFn: async (input: { key: string; condition: MailQueryRule }) => ({
      key: input.key,
      preview: await runtimeMutations.settings.previewAutomationRule({
        condition: input.condition,
        limit: 5,
      }),
    }),
  })
  const activePreview =
    previewMutation.data?.key === previewKey
      ? previewMutation.data.preview
      : null
  const activePreviewError =
    previewMutation.variables?.key === previewKey
      ? previewMutation.error?.message
      : null

  function runPreview() {
    previewMutation.mutate({
      key: previewKey,
      condition: previewCondition,
    })
  }

  const isComplete = isDraftComplete(draft)
  const saveStatus = isComplete
    ? state === 'active'
      ? 'Saves as active'
      : 'Moves to active'
    : 'Saves as draft'

  return (
    <div className="space-y-12 pt-1">
      <SettingsBackButton ariaLabel="Back to actions" onClick={onBack}>
        Actions
      </SettingsBackButton>

      <div className="space-y-2">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-0">
            <p className="truncate text-[14px] font-semibold text-foreground">
              {draft.name.trim() || 'Untitled rule'}
            </p>
            <p className="mt-1 text-[12px] text-muted-foreground">
              {saveStatus}
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <Button
              type="button"
              size="sm"
              onClick={onSave}
              disabled={savePending}
              className="bg-brand-coral text-white hover:bg-brand-coral/90"
            >
              {savePending ? 'Saving' : 'Save action'}
            </Button>
            <Button
              type="button"
              size="sm"
              variant="outline"
              className="rounded-md border-border bg-background"
              onClick={() => {
                // Backfilling makes this a backfill rule; keep the checkbox in
                // sync with what gets persisted.
                if (!draft.backfill) {
                  onChange({ backfill: true })
                }
                onBackfill()
              }}
              disabled={!isComplete || savePending}
              title="Apply this rule's actions to all existing matching messages"
            >
              Backfill
            </Button>
            <Button
              type="button"
              size="sm"
              variant="ghost"
              className="h-8 px-2 text-muted-foreground hover:text-destructive"
              onClick={onRemove}
              disabled={savePending}
            >
              Remove
            </Button>
          </div>
        </div>

        {backfillNoticeFor === draft.id && (
          <p className="text-[12px] text-muted-foreground">
            Backfill started — this rule is being applied to existing matching
            messages in the background.
          </p>
        )}
      </div>

      <RuleEditorSection title="Basics">
        <div className="grid gap-3 lg:grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)_minmax(0,1fr)] lg:items-end">
          <Field
            label="Rule name"
            value={draft.name}
            placeholder="Newsletter tags"
            onChange={(name) => onChange({ name })}
          />

          {canEditAccount && (
            <LabeledSelect
              label="Account"
              value={draft.accountId}
              onValueChange={(accountId) => onChange({ accountId })}
            >
              {accounts.map((account) => (
                <SelectItem key={account.id} value={account.id}>
                  {account.name}
                </SelectItem>
              ))}
            </LabeledSelect>
          )}

          <LabeledSelect
            label="Trigger"
            value={draft.triggers[0] ?? 'messageArrived'}
            onValueChange={(value) =>
              onChange({
                triggers: [
                  parseTrigger(value, draft.triggers[0] ?? 'messageArrived'),
                ],
              })
            }
          >
            {TRIGGER_OPTIONS.map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </LabeledSelect>
        </div>

        <div className="flex flex-wrap items-center gap-4 pt-1 text-[13px] text-muted-foreground">
          <label className="flex items-center gap-2">
            <Checkbox
              checked={draft.enabled}
              onCheckedChange={(checked) =>
                onChange({ enabled: checked === true })
              }
            />
            Enabled
          </label>
          <label className="flex items-center gap-2">
            <Checkbox
              checked={draft.backfill}
              onCheckedChange={(checked) =>
                onChange({ backfill: checked === true })
              }
            />
            Backfill existing messages
          </label>
        </div>
      </RuleEditorSection>

      <RuleEditorSection title="Conditions">
        <ConditionEditorContext.Provider
          value={{
            accountId: draft.accountId,
            mailboxes: staticMailboxes,
            accounts,
          }}
        >
          <RuleGroupEditor
            group={draft.condition.root}
            onChange={(root) => onChange({ condition: { root } })}
          />
        </ConditionEditorContext.Provider>
      </RuleEditorSection>

      <RuleEditorSection title="Preview">
        <AutomationRulePreview
          accountId={draft.accountId}
          preview={activePreview}
          error={activePreviewError ?? null}
          isPending={
            previewMutation.isPending &&
            previewMutation.variables?.key === previewKey
          }
          onPreview={runPreview}
        />
      </RuleEditorSection>

      <RuleEditorSection title="Actions">
        <ActionListEditor
          accountId={draft.accountId}
          actions={draft.actions}
          staticMailboxes={staticMailboxes}
          onChange={(actions) => onChange({ actions })}
        />
      </RuleEditorSection>
    </div>
  )
}

function RuleEditorSection({
  title,
  children,
}: {
  title: string
  children: React.ReactNode
}) {
  return (
    <section className="grid gap-5 md:grid-cols-[104px_1fr]">
      <div>
        <h4 className="text-[12px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">
          {title}
        </h4>
      </div>
      <div className="min-w-0 space-y-4">{children}</div>
    </section>
  )
}
