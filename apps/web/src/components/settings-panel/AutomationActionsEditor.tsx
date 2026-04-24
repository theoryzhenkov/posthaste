/**
 * Shared automation action editors for account and smart-mailbox settings.
 *
 * Automation rules are persisted globally in app settings. Account and smart
 * mailbox editors project their UI context into normal query conditions.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 * @spec docs/L1-search#smart-mailbox-data-model
 */
import { useMutation, useQuery } from '@tanstack/react-query'
import type React from 'react'
import { useState } from 'react'
import type {
  AccountOverview,
  AppSettings,
  AutomationAction,
  AutomationTrigger,
  Mailbox,
  MessageSummary,
  SmartMailboxRule,
  SmartMailbox,
} from '../../api/types'
import type { AutomationRuleDraft } from '../../automationRules'
import {
  accountRulePrefix,
  actionConditionFromSmartMailboxRule,
  draftToRule,
  extractAccountIdFromRule,
  isSmartMailboxLinkedRule,
  normalizeAction,
  ruleToDraft,
  smartMailboxDraftToRule,
  smartMailboxRulePrefix,
} from '../../automationRules'
import {
  fetchMailboxes,
  patchSettings,
  previewAutomationRule,
} from '../../api/client'
import { cn } from '../../lib/utils'
import { queryKeys } from '../../queryKeys'
import { formatRelativeTime } from '../../utils/relativeTime'
import { Button } from '../ui/button'
import { Checkbox } from '../ui/checkbox'
import { Input } from '../ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../ui/select'
import { RuleGroupEditor } from './RuleGroupEditor'
import { defaultEmptyRule } from './helpers'
import { FeedbackBanner, Field } from './shared'

const TRIGGER_OPTIONS: Array<{
  value: AutomationTrigger
  label: string
}> = [
  { value: 'messageArrived', label: 'Mail arrives' },
  { value: 'messageChanged', label: 'Mail changes' },
  { value: 'manual', label: 'Manual' },
]

const ACTION_KIND_OPTIONS: Array<{
  value: AutomationAction['kind']
  label: string
}> = [
  { value: 'applyTag', label: 'Apply tag' },
  { value: 'removeTag', label: 'Remove tag' },
  { value: 'markRead', label: 'Mark read' },
  { value: 'markUnread', label: 'Mark unread' },
  { value: 'flag', label: 'Flag' },
  { value: 'unflag', label: 'Unflag' },
  { value: 'moveToMailbox', label: 'Move to mailbox' },
]

function createRuleId(prefix = 'automation'): string {
  if (globalThis.crypto && 'randomUUID' in globalThis.crypto) {
    return `${prefix}:${globalThis.crypto.randomUUID()}`
  }
  return `${prefix}:${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`
}

function automationRuleSignature(rules: AutomationRuleDraft[]): string {
  return JSON.stringify(
    rules.map((rule) => ({
      ...rule,
      id: rule.id.trim(),
      accountId: rule.accountId.trim(),
      name: rule.name.trim(),
      actions: rule.actions.map(normalizeAction),
    })),
  )
}

function defaultAction(): AutomationAction {
  return { kind: 'applyTag', tag: '' }
}

function actionForKind(kind: AutomationAction['kind']): AutomationAction {
  switch (kind) {
    case 'applyTag':
      return { kind: 'applyTag', tag: '' }
    case 'removeTag':
      return { kind: 'removeTag', tag: '' }
    case 'moveToMailbox':
      return { kind: 'moveToMailbox', mailboxId: '' }
    case 'markRead':
      return { kind: 'markRead' }
    case 'markUnread':
      return { kind: 'markUnread' }
    case 'flag':
      return { kind: 'flag' }
    case 'unflag':
      return { kind: 'unflag' }
  }
}

function isActionComplete(action: AutomationAction): boolean {
  switch (action.kind) {
    case 'applyTag':
    case 'removeTag':
      return action.tag.trim().length > 0 && !action.tag.trim().startsWith('$')
    case 'moveToMailbox':
      return action.mailboxId.trim().length > 0
    case 'markRead':
    case 'markUnread':
    case 'flag':
    case 'unflag':
      return true
  }
}

function isDraftComplete(draft: AutomationRuleDraft): boolean {
  return (
    draft.accountId.trim().length > 0 &&
    draft.name.trim().length > 0 &&
    draft.actions.length > 0 &&
    draft.actions.every(isActionComplete)
  )
}

function defaultDraft({
  accountId,
  name,
  idPrefix,
}: {
  accountId: string
  name: string
  idPrefix?: string
}): AutomationRuleDraft {
  return {
    id: createRuleId(idPrefix),
    accountId,
    name,
    enabled: true,
    triggers: ['messageArrived'],
    condition: defaultEmptyRule(),
    actions: [defaultAction()],
    backfill: true,
  }
}

function actionListDescription(drafts: AutomationRuleDraft[]): string {
  if (drafts.length === 0) {
    return 'No actions configured.'
  }
  const actionCount = drafts.reduce(
    (count, rule) => count + rule.actions.length,
    0,
  )
  return `${actionCount} ${actionCount === 1 ? 'action' : 'actions'} in ${drafts.length} ${
    drafts.length === 1 ? 'rule' : 'rules'
  }.`
}

function triggerLabel(trigger: AutomationTrigger): string {
  return (
    TRIGGER_OPTIONS.find((option) => option.value === trigger)?.label ?? trigger
  )
}

function actionSummary(action: AutomationAction): string {
  switch (action.kind) {
    case 'applyTag':
      return action.tag.trim() ? `Tag ${action.tag.trim()}` : 'Apply tag'
    case 'removeTag':
      return action.tag.trim() ? `Remove ${action.tag.trim()}` : 'Remove tag'
    case 'markRead':
      return 'Mark read'
    case 'markUnread':
      return 'Mark unread'
    case 'flag':
      return 'Flag'
    case 'unflag':
      return 'Unflag'
    case 'moveToMailbox':
      return action.mailboxId.trim()
        ? `Move to ${action.mailboxId.trim()}`
        : 'Move to mailbox'
  }
}

function ruleActionSummary(draft: AutomationRuleDraft): string {
  if (draft.actions.length === 0) {
    return 'No actions'
  }
  const [firstAction] = draft.actions
  const first = actionSummary(firstAction)
  if (draft.actions.length === 1) {
    return first
  }
  return `${first} +${draft.actions.length - 1}`
}

function accountName(accounts: AccountOverview[], accountId: string): string {
  return (
    accounts.find((account) => account.id === accountId)?.name ||
    accountId.trim() ||
    'Account'
  )
}

export function AccountAutomationFields({
  account,
  settings,
  onSaved,
}: {
  account: AccountOverview
  settings: AppSettings
  onSaved: (settings: AppSettings) => Promise<void>
}) {
  const rulePrefix = accountRulePrefix(account.id)
  const [drafts, setDrafts] = useState<AutomationRuleDraft[]>(() =>
    (settings.automationRules ?? [])
      .filter((rule) => rule.id.startsWith(rulePrefix))
      .map((rule) => ruleToDraft(account.id, rule)),
  )
  const [savedSignature, setSavedSignature] = useState(() =>
    automationRuleSignature(drafts),
  )
  const mailboxesQuery = useQuery({
    queryKey: queryKeys.mailboxes(account.id),
    queryFn: () => fetchMailboxes(account.id),
  })
  const saveMutation = useMutation({
    mutationFn: () => {
      const nextRules = [
        ...(settings.automationRules ?? []).filter(
          (rule) => !rule.id.startsWith(rulePrefix),
        ),
        ...drafts.map(draftToRule),
      ]
      return patchSettings({ automationRules: nextRules })
    },
    onSuccess: async (savedSettings) => {
      const nextDrafts = (savedSettings.automationRules ?? [])
        .filter((rule) => rule.id.startsWith(rulePrefix))
        .map((rule) => ruleToDraft(account.id, rule))
      setDrafts(nextDrafts)
      setSavedSignature(automationRuleSignature(nextDrafts))
      await onSaved(savedSettings)
    },
  })

  const hasInvalidRule = drafts.some((draft) => !isDraftComplete(draft))
  const hasUnsavedChanges = automationRuleSignature(drafts) !== savedSignature

  return (
    <AutomationRuleList
      title="Actions"
      description="Run backend actions when matching mail arrives or changes."
      drafts={drafts}
      accounts={[account]}
      mailboxesByAccount={{ [account.id]: mailboxesQuery.data ?? [] }}
      canEditAccount={false}
      addLabel="Add action rule"
      emptyText="No account actions configured."
      saveLabel="Apply actions"
      statusText={
        hasInvalidRule
          ? 'Complete all actions before applying'
          : hasUnsavedChanges
            ? 'Unsaved action changes'
            : 'Actions saved'
      }
      savePending={saveMutation.isPending}
      saveDisabled={!hasUnsavedChanges || hasInvalidRule}
      errors={[
        mailboxesQuery.error?.message ?? null,
        saveMutation.error?.message ?? null,
      ]}
      onAdd={() =>
        setDrafts((current) => [
          ...current,
          defaultDraft({
            accountId: account.id,
            name: 'New action rule',
            idPrefix: `account:${account.id}`,
          }),
        ])
      }
      onChange={(nextDrafts) => setDrafts(nextDrafts)}
      onSave={() => saveMutation.mutate()}
      previewConditionForDraft={(draft) => draftToRule(draft).condition}
    />
  )
}

export function SmartMailboxAutomationFields({
  accounts,
  settings,
  smartMailbox,
  disabledReason,
  onSaved,
}: {
  accounts: AccountOverview[]
  settings: AppSettings
  smartMailbox: SmartMailbox
  disabledReason?: string | null
  onSaved: (settings: AppSettings) => Promise<void>
}) {
  const fallbackAccountId = accounts[0]?.id ?? ''
  const linkedDrafts = () =>
    (settings.automationRules ?? [])
      .filter((rule) => isSmartMailboxLinkedRule(rule, smartMailbox.id))
      .map((rule) => {
        const accountId = extractAccountIdFromRule(rule, fallbackAccountId)
        return {
          ...ruleToDraft(accountId, rule),
          condition: actionConditionFromSmartMailboxRule(rule, accountId),
        }
      })
  const [drafts, setDrafts] = useState<AutomationRuleDraft[]>(linkedDrafts)
  const [savedSignature, setSavedSignature] = useState(() =>
    automationRuleSignature(drafts),
  )
  const saveMutation = useMutation({
    mutationFn: () => {
      const linkedPrefix = smartMailboxRulePrefix(smartMailbox.id)
      const nextRules = [
        ...(settings.automationRules ?? []).filter(
          (rule) => !rule.id.startsWith(linkedPrefix),
        ),
        ...drafts.map((draft) => smartMailboxDraftToRule(draft, smartMailbox)),
      ]
      return patchSettings({ automationRules: nextRules })
    },
    onSuccess: async (savedSettings) => {
      const nextDrafts = (savedSettings.automationRules ?? [])
        .filter((rule) => isSmartMailboxLinkedRule(rule, smartMailbox.id))
        .map((rule) => {
          const accountId = extractAccountIdFromRule(rule, fallbackAccountId)
          return {
            ...ruleToDraft(accountId, rule),
            condition: actionConditionFromSmartMailboxRule(rule, accountId),
          }
        })
      setDrafts(nextDrafts)
      setSavedSignature(automationRuleSignature(nextDrafts))
      await onSaved(savedSettings)
    },
  })

  const hasInvalidRule = drafts.some((draft) => !isDraftComplete(draft))
  const hasUnsavedChanges = automationRuleSignature(drafts) !== savedSignature

  return (
    <AutomationRuleList
      title="Actions"
      description="Run backend actions for messages that match this smart mailbox, with optional per-action filters."
      drafts={drafts}
      accounts={accounts}
      canEditAccount
      addLabel="Add action rule"
      emptyText="No smart mailbox actions configured."
      saveLabel="Apply actions"
      statusText={
        disabledReason
          ? disabledReason
          : hasInvalidRule
            ? 'Complete all actions before applying'
            : hasUnsavedChanges
              ? 'Unsaved action changes'
              : 'Actions saved'
      }
      savePending={saveMutation.isPending}
      saveDisabled={
        Boolean(disabledReason) ||
        !hasUnsavedChanges ||
        hasInvalidRule ||
        accounts.length === 0
      }
      addDisabled={Boolean(disabledReason)}
      errors={[saveMutation.error?.message ?? null]}
      onAdd={() => {
        const account = accounts[0]
        if (!account) {
          return
        }
        setDrafts((current) => [
          ...current,
          defaultDraft({
            accountId: account.id,
            name: `${smartMailbox.name} action`,
            idPrefix: smartMailboxRulePrefix(smartMailbox.id),
          }),
        ])
      }}
      onChange={(nextDrafts) => setDrafts(nextDrafts)}
      onSave={() => saveMutation.mutate()}
      previewConditionForDraft={(draft) =>
        smartMailboxDraftToRule(draft, smartMailbox).condition
      }
    />
  )
}

function AutomationRuleList({
  title,
  description,
  drafts,
  accounts,
  mailboxesByAccount = {},
  canEditAccount,
  addLabel,
  emptyText,
  saveLabel,
  statusText,
  savePending,
  saveDisabled,
  addDisabled = false,
  errors,
  onAdd,
  onChange,
  onSave,
  previewConditionForDraft,
}: {
  title: string
  description: string
  drafts: AutomationRuleDraft[]
  accounts: AccountOverview[]
  mailboxesByAccount?: Record<string, Mailbox[]>
  canEditAccount: boolean
  addLabel: string
  emptyText: string
  saveLabel: string
  statusText: string
  savePending: boolean
  saveDisabled: boolean
  addDisabled?: boolean
  errors: Array<string | null>
  onAdd: () => void
  onChange: (drafts: AutomationRuleDraft[]) => void
  onSave: () => void
  previewConditionForDraft: (draft: AutomationRuleDraft) => SmartMailboxRule
}) {
  const [selectedRuleId, setSelectedRuleId] = useState<string | null>(
    () => drafts[0]?.id ?? null,
  )

  function updateDraft(ruleId: string, patch: Partial<AutomationRuleDraft>) {
    onChange(
      drafts.map((draft) =>
        draft.id === ruleId
          ? {
              ...draft,
              ...patch,
            }
          : draft,
      ),
    )
  }

  const effectiveSelectedRuleId = drafts.some(
    (draft) => draft.id === selectedRuleId,
  )
    ? selectedRuleId
    : (drafts[0]?.id ?? null)
  const selectedDraft =
    drafts.find((draft) => draft.id === effectiveSelectedRuleId) ?? null

  return (
    <div className="mt-8 space-y-4">
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div>
          <p className="text-[13px] font-medium text-foreground">{title}</p>
          <p className="text-[12px] text-muted-foreground">{description}</p>
          <p className="mt-1 text-[12px] text-muted-foreground">
            {actionListDescription(drafts)}
          </p>
        </div>
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="rounded-md border-border bg-background"
          disabled={accounts.length === 0 || addDisabled}
          onClick={onAdd}
        >
          {addLabel}
        </Button>
      </div>

      {drafts.length === 0 ? (
        <p className="text-[12px] text-muted-foreground">{emptyText}</p>
      ) : (
        <div className="overflow-hidden rounded-lg border border-border-soft bg-bg-elev/35">
          {drafts.map((draft) => (
            <AutomationRuleListRow
              key={draft.id}
              draft={draft}
              accounts={accounts}
              isSelected={draft.id === effectiveSelectedRuleId}
              isComplete={isDraftComplete(draft)}
              onSelect={() => setSelectedRuleId(draft.id)}
            />
          ))}
        </div>
      )}

      {selectedDraft && (
        <AutomationRuleEditor
          draft={selectedDraft}
          accounts={accounts}
          staticMailboxes={mailboxesByAccount[selectedDraft.accountId] ?? null}
          canEditAccount={canEditAccount}
          previewCondition={previewConditionForDraft(selectedDraft)}
          onChange={(patch) => updateDraft(selectedDraft.id, patch)}
          onRemove={() => {
            const nextDrafts = drafts.filter(
              (candidate) => candidate.id !== selectedDraft.id,
            )
            onChange(nextDrafts)
            setSelectedRuleId(nextDrafts[0]?.id ?? null)
          }}
        />
      )}

      {errors.filter(Boolean).map((message) => (
        <FeedbackBanner key={message} tone="error">
          {message}
        </FeedbackBanner>
      ))}

      <div className="flex flex-wrap items-center gap-2">
        <Button
          type="button"
          size="sm"
          onClick={onSave}
          disabled={savePending || saveDisabled}
          className="bg-brand-coral text-white hover:bg-brand-coral/90"
        >
          {saveLabel}
        </Button>
        <span className="text-[12px] text-muted-foreground">{statusText}</span>
      </div>
    </div>
  )
}

function AutomationRuleListRow({
  draft,
  accounts,
  isSelected,
  isComplete,
  onSelect,
}: {
  draft: AutomationRuleDraft
  accounts: AccountOverview[]
  isSelected: boolean
  isComplete: boolean
  onSelect: () => void
}) {
  return (
    <button
      type="button"
      onClick={onSelect}
      className={cn(
        'group flex min-h-[58px] w-full items-center gap-3 border-b border-border-soft px-4 text-left transition-colors last:border-b-0 hover:bg-[var(--list-hover)]',
        isSelected && 'bg-[var(--list-hover)]',
      )}
    >
      <span
        aria-hidden
        className={cn(
          'size-2 shrink-0 rounded-full',
          draft.enabled ? 'bg-emerald-500' : 'bg-zinc-400',
          !isComplete && 'bg-amber-500',
        )}
      />
      <span className="min-w-0 flex-1">
        <span className="flex min-w-0 items-center gap-2">
          <span className="truncate text-[13px] font-medium text-foreground">
            {draft.name.trim() || 'Untitled rule'}
          </span>
          {!isComplete && (
            <span className="shrink-0 rounded-sm bg-amber-500/10 px-1.5 py-0.5 text-[10px] font-medium text-amber-700">
              incomplete
            </span>
          )}
        </span>
        <span className="mt-0.5 block truncate text-[12px] text-muted-foreground">
          {accountName(accounts, draft.accountId)} ·{' '}
          {triggerLabel(draft.triggers[0] ?? 'messageArrived')} ·{' '}
          {ruleActionSummary(draft)}
        </span>
      </span>
      <span className="shrink-0 text-[12px] text-muted-foreground/70">
        {draft.backfill ? 'Backfill' : 'New mail'}
      </span>
    </button>
  )
}

function AutomationRuleEditor({
  draft,
  accounts,
  staticMailboxes,
  canEditAccount,
  previewCondition,
  onChange,
  onRemove,
}: {
  draft: AutomationRuleDraft
  accounts: AccountOverview[]
  staticMailboxes: Mailbox[] | null
  canEditAccount: boolean
  previewCondition: SmartMailboxRule
  onChange: (patch: Partial<AutomationRuleDraft>) => void
  onRemove: () => void
}) {
  const previewKey = JSON.stringify(previewCondition)
  const previewMutation = useMutation({
    mutationFn: async (input: {
      key: string
      condition: SmartMailboxRule
    }) => ({
      key: input.key,
      preview: await previewAutomationRule({
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

  return (
    <div className="space-y-5 pt-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <p className="text-[12px] font-medium text-foreground">
            Edit selected rule
          </p>
          <p className="text-[12px] text-muted-foreground">
            Account and trigger decide when the conditions below are evaluated.
          </p>
        </div>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          className="h-8 px-2 text-muted-foreground hover:text-destructive"
          onClick={onRemove}
        >
          Remove
        </Button>
      </div>

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

      <div className="flex flex-wrap items-center gap-4 text-[13px] text-muted-foreground">
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

      <div className="space-y-2">
        <p className="text-[12px] font-medium text-muted-foreground">
          Conditions
        </p>
        <RuleGroupEditor
          group={draft.condition.root}
          onChange={(root) => onChange({ condition: { root } })}
        />
      </div>

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

      <ActionListEditor
        accountId={draft.accountId}
        actions={draft.actions}
        staticMailboxes={staticMailboxes}
        onChange={(actions) => onChange({ actions })}
      />
    </div>
  )
}

function AutomationRulePreview({
  accountId,
  preview,
  error,
  isPending,
  onPreview,
}: {
  accountId: string
  preview: { total: number; items: MessageSummary[] } | null
  error: string | null
  isPending: boolean
  onPreview: () => void
}) {
  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <p className="text-[12px] font-medium text-muted-foreground">
            Matching messages
          </p>
          {preview && (
            <p className="text-[12px] text-muted-foreground">
              {preview.total} {preview.total === 1 ? 'message' : 'messages'}{' '}
              match this rule.
            </p>
          )}
        </div>
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="h-7 rounded-md border-border bg-background px-2 text-[12px]"
          disabled={isPending || accountId.trim().length === 0}
          onClick={onPreview}
        >
          {isPending ? 'Checking' : 'Preview'}
        </Button>
      </div>

      {error && <FeedbackBanner tone="error">{error}</FeedbackBanner>}

      {preview && (
        <div className="overflow-hidden rounded-lg border border-border-soft bg-bg-elev/25">
          {preview.items.length === 0 ? (
            <p className="px-3 py-2 text-[12px] text-muted-foreground">
              No synced messages match this rule.
            </p>
          ) : (
            preview.items.map((message) => (
              <AutomationRulePreviewRow key={message.id} message={message} />
            ))
          )}
        </div>
      )}
    </div>
  )
}

function AutomationRulePreviewRow({ message }: { message: MessageSummary }) {
  const sender = message.fromName ?? message.fromEmail ?? 'Unknown sender'
  return (
    <div className="grid min-h-11 grid-cols-[minmax(0,1fr)_auto] items-center gap-3 border-b border-border-soft px-3 py-2 last:border-b-0">
      <div className="min-w-0">
        <p className="truncate text-[12px] font-medium text-foreground">
          {message.subject?.trim() || '(no subject)'}
        </p>
        <p className="truncate text-[12px] text-muted-foreground">{sender}</p>
      </div>
      <p className="shrink-0 text-[11px] text-muted-foreground">
        {formatRelativeTime(message.receivedAt)}
      </p>
    </div>
  )
}

function LabeledSelect({
  label,
  value,
  onValueChange,
  children,
}: {
  label: string
  value: string
  onValueChange: (value: string) => void
  children: React.ReactNode
}) {
  return (
    <label className="grid gap-1.5 text-[13px]">
      <span className="text-[12px] font-medium text-muted-foreground">
        {label}
      </span>
      <Select value={value} onValueChange={onValueChange}>
        <SelectTrigger className="h-8 w-full rounded-md border-border bg-background text-[13px] shadow-none">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>{children}</SelectContent>
      </Select>
    </label>
  )
}

function ActionListEditor({
  accountId,
  actions,
  staticMailboxes,
  onChange,
}: {
  accountId: string
  actions: AutomationAction[]
  staticMailboxes: Mailbox[] | null
  onChange: (actions: AutomationAction[]) => void
}) {
  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="text-[12px] font-medium text-muted-foreground">Actions</p>
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="h-7 rounded-md border-border bg-background px-2 text-[12px]"
          onClick={() => onChange([...actions, defaultAction()])}
        >
          Add action
        </Button>
      </div>
      {actions.map((action, index) => (
        <ActionRow
          key={index}
          accountId={accountId}
          action={action}
          staticMailboxes={staticMailboxes}
          onChange={(nextAction) =>
            onChange(
              actions.map((candidate, candidateIndex) =>
                candidateIndex === index ? nextAction : candidate,
              ),
            )
          }
          onRemove={() =>
            onChange(
              actions.filter((_, candidateIndex) => candidateIndex !== index),
            )
          }
        />
      ))}
    </div>
  )
}

function ActionRow({
  accountId,
  action,
  staticMailboxes,
  onChange,
  onRemove,
}: {
  accountId: string
  action: AutomationAction
  staticMailboxes: Mailbox[] | null
  onChange: (action: AutomationAction) => void
  onRemove: () => void
}) {
  return (
    <div className="grid gap-2 sm:grid-cols-[minmax(150px,0.9fr)_minmax(160px,1fr)_auto] sm:items-end">
      <LabeledSelect
        label="Action"
        value={action.kind}
        onValueChange={(value) =>
          onChange(actionForKind(parseActionKind(value, action.kind)))
        }
      >
        {ACTION_KIND_OPTIONS.map((option) => (
          <SelectItem key={option.value} value={option.value}>
            {option.label}
          </SelectItem>
        ))}
      </LabeledSelect>
      <ActionValueEditor
        accountId={accountId}
        action={action}
        staticMailboxes={staticMailboxes}
        onChange={onChange}
      />
      <Button
        type="button"
        size="sm"
        variant="ghost"
        className="h-8 justify-self-start px-2 text-muted-foreground hover:text-destructive sm:justify-self-end"
        onClick={onRemove}
      >
        Remove
      </Button>
    </div>
  )
}

function ActionValueEditor({
  accountId,
  action,
  staticMailboxes,
  onChange,
}: {
  accountId: string
  action: AutomationAction
  staticMailboxes: Mailbox[] | null
  onChange: (action: AutomationAction) => void
}) {
  if (action.kind === 'applyTag' || action.kind === 'removeTag') {
    return (
      <Field
        label="Tag"
        value={action.tag}
        placeholder="newsletter"
        onChange={(tag) => onChange({ ...action, tag })}
      />
    )
  }
  if (action.kind === 'moveToMailbox') {
    return (
      <MailboxSelect
        accountId={accountId}
        label="Target mailbox"
        mailboxId={action.mailboxId}
        staticMailboxes={staticMailboxes}
        onChange={(mailboxId) => onChange({ ...action, mailboxId })}
      />
    )
  }
  return (
    <label className="grid gap-1.5 text-[13px]">
      <span className="text-[12px] font-medium text-muted-foreground">
        Value
      </span>
      <Input
        className="h-8 rounded-md border-border bg-background text-[13px] shadow-none"
        value="No value"
        disabled
      />
    </label>
  )
}

function MailboxSelect({
  accountId,
  label,
  mailboxId,
  staticMailboxes,
  onChange,
}: {
  accountId: string
  label: string
  mailboxId: string
  staticMailboxes: Mailbox[] | null
  onChange: (mailboxId: string) => void
}) {
  const mailboxesQuery = useQuery({
    queryKey: queryKeys.mailboxes(accountId),
    queryFn: () => fetchMailboxes(accountId),
    enabled: staticMailboxes === null && accountId.trim().length > 0,
  })
  const mailboxes = staticMailboxes ?? mailboxesQuery.data ?? []
  const value = mailboxId.trim().length > 0 ? mailboxId : '__unset__'

  return (
    <LabeledSelect
      label={label}
      value={value}
      onValueChange={(value) =>
        onChange(value.startsWith('__unset__') ? '' : value)
      }
    >
      <SelectItem value="__unset__">Choose mailbox</SelectItem>
      {mailboxes.map((mailbox) => (
        <SelectItem key={mailbox.id} value={mailbox.id}>
          {mailbox.name}
        </SelectItem>
      ))}
      {mailboxId.trim().length > 0 &&
        !mailboxes.some((mailbox) => mailbox.id === mailboxId) && (
          <SelectItem value={mailboxId}>{mailboxId}</SelectItem>
        )}
    </LabeledSelect>
  )
}

function parseTrigger(
  value: string,
  fallback: AutomationTrigger,
): AutomationTrigger {
  return (
    TRIGGER_OPTIONS.find((option) => option.value === value)?.value ?? fallback
  )
}

function parseActionKind(
  value: string,
  fallback: AutomationAction['kind'],
): AutomationAction['kind'] {
  return (
    ACTION_KIND_OPTIONS.find((option) => option.value === value)?.value ??
    fallback
  )
}
