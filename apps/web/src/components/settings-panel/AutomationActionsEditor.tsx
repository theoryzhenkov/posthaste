/**
 * Shared automation action editors for account and smart-mailbox settings.
 *
 * Automation rules are persisted globally in app settings. Account and smart
 * mailbox editors project their UI context into normal query conditions.
 *
 * @spec docs/L1-api#application-settings
 * @spec docs/L1-search#smart-mailbox-data-model
 */
import { useMutation, useQuery } from '@tanstack/react-query'
import type React from 'react'
import { useState } from 'react'
import { createPortal } from 'react-dom'
import type {
  AccountOverview,
  AppSettings,
  AutomationAction,
  AutomationRule,
  Mailbox,
  MessageSummary,
  SmartMailboxRule,
  SmartMailbox,
} from '../../api/types'
import type { AutomationRuleDraft } from '../../automationRules'
import {
  actionConditionFromSourceMailboxRule,
  actionConditionFromSmartMailboxRule,
  extractAccountIdFromRule,
  isSourceMailboxLinkedRule,
  isSmartMailboxLinkedRule,
  ruleToDraft,
  sourceMailboxDraftToRule,
  sourceMailboxRulePrefix,
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
import { FeedbackBanner, Field, SettingsBackButton } from './shared'
import {
  ACTION_KIND_OPTIONS,
  TRIGGER_OPTIONS,
  accountName,
  actionForKind,
  actionListDescription,
  defaultAction,
  defaultDraft,
  isDraftComplete,
  parseActionKind,
  parseTrigger,
  removeRule,
  ruleActionSummary,
  triggerLabel,
  upsertRule,
  type AutomationRuleItem,
  type AutomationRuleState,
} from './automationRuleHelpers'

function linkedAutomationRuleItems({
  rules,
  drafts,
  isLinkedRule,
  accountIdForRule,
  conditionForRule,
}: {
  rules: AutomationRule[]
  drafts: AutomationRule[]
  isLinkedRule: (rule: AutomationRule) => boolean
  accountIdForRule: (rule: AutomationRule) => string
  conditionForRule: (
    rule: AutomationRule,
    accountId: string,
  ) => SmartMailboxRule
}): AutomationRuleItem[] {
  const mapItem =
    (state: AutomationRuleState) =>
    (rule: AutomationRule): AutomationRuleItem => {
      const accountId = accountIdForRule(rule)
      return {
        state,
        draft: {
          ...ruleToDraft(accountId, rule),
          condition: conditionForRule(rule, accountId),
        },
      }
    }

  return [
    ...rules.filter(isLinkedRule).map(mapItem('active')),
    ...drafts.filter(isLinkedRule).map(mapItem('draft')),
  ]
}

function LinkedAutomationRuleFields({
  accounts,
  mailboxesByAccount,
  settings,
  canEditAccount,
  addLabel,
  emptyText,
  addDisabled = false,
  disabledReason = null,
  onSaved,
  itemsFromSettings,
  createDraft,
  draftToRule,
  previewConditionForDraft,
}: {
  accounts: AccountOverview[]
  mailboxesByAccount?: Record<string, Mailbox[]>
  settings: AppSettings
  canEditAccount: boolean
  addLabel: string
  emptyText: string
  addDisabled?: boolean
  disabledReason?: string | null
  onSaved: (settings: AppSettings) => Promise<void>
  itemsFromSettings: (settings: AppSettings) => AutomationRuleItem[]
  createDraft: () => AutomationRuleDraft | null
  draftToRule: (draft: AutomationRuleDraft) => AutomationRule
  previewConditionForDraft: (draft: AutomationRuleDraft) => SmartMailboxRule
}) {
  const [items, setItems] = useState<AutomationRuleItem[]>(() =>
    itemsFromSettings(settings),
  )
  const persistMutation = useMutation({
    mutationFn: (input: Partial<AppSettings>) => patchSettings(input),
    onSuccess: async (savedSettings) => {
      setItems(itemsFromSettings(savedSettings))
      await onSaved(savedSettings)
    },
  })

  function persistItem(draft: AutomationRuleDraft) {
    const rule = draftToRule(draft)
    const complete = isDraftComplete(draft)
    persistMutation.mutate({
      automationRules: complete
        ? upsertRule(settings.automationRules ?? [], rule)
        : removeRule(settings.automationRules ?? [], rule.id),
      automationDrafts: complete
        ? removeRule(settings.automationDrafts ?? [], rule.id)
        : upsertRule(settings.automationDrafts ?? [], rule),
    })
  }

  function removeItem(draft: AutomationRuleDraft) {
    const ruleId = draft.id.trim()
    setItems((current) =>
      current.filter((item) => item.draft.id.trim() !== ruleId),
    )
    persistMutation.mutate({
      automationRules: removeRule(settings.automationRules ?? [], ruleId),
      automationDrafts: removeRule(settings.automationDrafts ?? [], ruleId),
    })
  }

  return (
    <AutomationRuleList
      title="Actions"
      items={items}
      accounts={accounts}
      mailboxesByAccount={mailboxesByAccount}
      canEditAccount={canEditAccount}
      addLabel={addLabel}
      emptyText={emptyText}
      savePending={persistMutation.isPending}
      addDisabled={addDisabled}
      disabledReason={disabledReason}
      errors={[persistMutation.error?.message ?? null]}
      onAdd={() => {
        const draft = createDraft()
        if (!draft) {
          return null
        }
        const rule = draftToRule(draft)
        setItems((current) => [...current, { state: 'draft', draft }])
        persistMutation.mutate({
          automationDrafts: upsertRule(settings.automationDrafts ?? [], rule),
        })
        return draft.id
      }}
      onChange={(ruleId, patch) =>
        setItems((current) =>
          current.map((item) =>
            item.draft.id === ruleId
              ? { ...item, draft: { ...item.draft, ...patch } }
              : item,
          ),
        )
      }
      onSaveItem={persistItem}
      onRemoveItem={removeItem}
      previewConditionForDraft={previewConditionForDraft}
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

  return (
    <LinkedAutomationRuleFields
      accounts={accounts}
      canEditAccount
      settings={settings}
      addLabel="Add action rule"
      emptyText="No smart mailbox actions configured."
      addDisabled={Boolean(disabledReason)}
      disabledReason={disabledReason}
      onSaved={onSaved}
      itemsFromSettings={(sourceSettings) =>
        linkedAutomationRuleItems({
          rules: sourceSettings.automationRules ?? [],
          drafts: sourceSettings.automationDrafts ?? [],
          isLinkedRule: (rule) =>
            isSmartMailboxLinkedRule(rule, smartMailbox.id),
          accountIdForRule: (rule) =>
            extractAccountIdFromRule(rule, fallbackAccountId),
          conditionForRule: actionConditionFromSmartMailboxRule,
        })
      }
      createDraft={() => {
        const account = accounts[0]
        if (!account) {
          return null
        }
        const draft = defaultDraft({
          accountId: account.id,
          name: `${smartMailbox.name} action`,
          idPrefix: smartMailboxRulePrefix(smartMailbox.id),
        })
        return draft
      }}
      draftToRule={(draft) => smartMailboxDraftToRule(draft, smartMailbox)}
      previewConditionForDraft={(draft) =>
        smartMailboxDraftToRule(draft, smartMailbox).condition
      }
    />
  )
}

export function SourceMailboxAutomationFields({
  account,
  mailbox,
  mailboxes,
  settings,
  onSaved,
}: {
  account: AccountOverview
  mailbox: Mailbox
  mailboxes: Mailbox[]
  settings: AppSettings
  onSaved: (settings: AppSettings) => Promise<void>
}) {
  const linkedPrefix = sourceMailboxRulePrefix(account.id, mailbox.id)

  return (
    <LinkedAutomationRuleFields
      accounts={[account]}
      mailboxesByAccount={{ [account.id]: mailboxes }}
      canEditAccount={false}
      settings={settings}
      addLabel="Add action rule"
      emptyText="No mailbox actions configured."
      onSaved={onSaved}
      itemsFromSettings={(sourceSettings) =>
        linkedAutomationRuleItems({
          rules: sourceSettings.automationRules ?? [],
          drafts: sourceSettings.automationDrafts ?? [],
          isLinkedRule: (rule) =>
            isSourceMailboxLinkedRule(rule, account.id, mailbox.id),
          accountIdForRule: () => account.id,
          conditionForRule: (rule) =>
            actionConditionFromSourceMailboxRule(rule, account.id, mailbox.id),
        })
      }
      createDraft={() => {
        const draft = defaultDraft({
          accountId: account.id,
          name: `${mailbox.name} action`,
          idPrefix: linkedPrefix,
        })
        return draft
      }}
      draftToRule={(draft) => sourceMailboxDraftToRule(draft, mailbox.id)}
      previewConditionForDraft={(draft) =>
        sourceMailboxDraftToRule(draft, mailbox.id).condition
      }
    />
  )
}

function AutomationRuleList({
  title,
  items,
  accounts,
  mailboxesByAccount = {},
  canEditAccount,
  addLabel,
  emptyText,
  savePending,
  addDisabled = false,
  disabledReason = null,
  errors,
  onAdd,
  onChange,
  onSaveItem,
  onRemoveItem,
  previewConditionForDraft,
}: {
  title: string
  items: AutomationRuleItem[]
  accounts: AccountOverview[]
  mailboxesByAccount?: Record<string, Mailbox[]>
  canEditAccount: boolean
  addLabel: string
  emptyText: string
  savePending: boolean
  addDisabled?: boolean
  disabledReason?: string | null
  errors: Array<string | null>
  onAdd: () => string | null
  onChange: (ruleId: string, patch: Partial<AutomationRuleDraft>) => void
  onSaveItem: (draft: AutomationRuleDraft) => void
  onRemoveItem: (draft: AutomationRuleDraft) => void
  previewConditionForDraft: (draft: AutomationRuleDraft) => SmartMailboxRule
}) {
  const [editingRuleId, setEditingRuleId] = useState<string | null>(null)

  function updateDraft(ruleId: string, patch: Partial<AutomationRuleDraft>) {
    onChange(ruleId, patch)
  }

  const editingItem =
    items.find((item) => item.draft.id === editingRuleId) ?? null

  return (
    <div className="mt-8 space-y-6">
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div>
          <p className="text-[13px] font-medium text-foreground">{title}</p>
          <p className="mt-1 text-[12px] text-muted-foreground">
            {disabledReason ?? actionListDescription(items)}
          </p>
        </div>
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="rounded-md border-border bg-background"
          disabled={accounts.length === 0 || addDisabled || savePending}
          onClick={() => {
            const newRuleId = onAdd()
            if (newRuleId) {
              setEditingRuleId(newRuleId)
            }
          }}
        >
          {addLabel}
        </Button>
      </div>

      {items.length === 0 ? (
        <p className="text-[12px] text-muted-foreground">{emptyText}</p>
      ) : (
        <div className="overflow-hidden rounded-lg border border-border-soft bg-bg-elev/35">
          {items.map((item) => (
            <AutomationRuleListRow
              key={item.draft.id}
              item={item}
              accounts={accounts}
              isComplete={isDraftComplete(item.draft)}
              onSelect={() => setEditingRuleId(item.draft.id)}
            />
          ))}
        </div>
      )}

      {errors.filter(Boolean).map((message) => (
        <FeedbackBanner key={message} tone="error">
          {message}
        </FeedbackBanner>
      ))}

      {editingItem && (
        <AutomationRuleEditorPortal>
          <AutomationRuleEditor
            draft={editingItem.draft}
            state={editingItem.state}
            accounts={accounts}
            staticMailboxes={
              mailboxesByAccount[editingItem.draft.accountId] ?? null
            }
            canEditAccount={canEditAccount}
            previewCondition={previewConditionForDraft(editingItem.draft)}
            savePending={savePending}
            onBack={() => setEditingRuleId(null)}
            onSave={() => onSaveItem(editingItem.draft)}
            onChange={(patch) => updateDraft(editingItem.draft.id, patch)}
            onRemove={() => {
              onRemoveItem(editingItem.draft)
              setEditingRuleId(null)
            }}
          />
        </AutomationRuleEditorPortal>
      )}
    </div>
  )
}

function AutomationRuleEditorPortal({
  children,
}: {
  children: React.ReactNode
}) {
  if (typeof document === 'undefined') {
    return null
  }

  return createPortal(
    <div className="fixed inset-0 z-[2150] bg-background text-card-foreground">
      <div className="ph-scroll h-full min-h-0 overflow-y-auto px-4 py-6 sm:px-6 sm:py-8">
        <div className="mx-auto flex max-w-[1040px] flex-col">{children}</div>
      </div>
    </div>,
    document.body,
  )
}

function AutomationRuleListRow({
  item,
  accounts,
  isComplete,
  onSelect,
}: {
  item: AutomationRuleItem
  accounts: AccountOverview[]
  isComplete: boolean
  onSelect: () => void
}) {
  const { draft, state } = item
  return (
    <button
      type="button"
      onClick={onSelect}
      className="group flex min-h-[58px] w-full items-center gap-3 border-b border-border-soft px-4 text-left transition-colors last:border-b-0 hover:bg-[var(--list-hover)]"
    >
      <span
        aria-hidden
        className={cn(
          'size-2 shrink-0 rounded-full',
          state === 'active' && draft.enabled
            ? 'bg-emerald-500'
            : 'bg-zinc-400',
          (state === 'draft' || !isComplete) && 'bg-amber-500',
        )}
      />
      <span className="min-w-0 flex-1">
        <span className="flex min-w-0 items-center gap-2">
          <span className="truncate text-[13px] font-medium text-foreground">
            {draft.name.trim() || 'Untitled rule'}
          </span>
          <span
            className={cn(
              'shrink-0 rounded-sm px-1.5 py-0.5 text-[10px] font-medium',
              state === 'active' && isComplete
                ? 'bg-emerald-500/10 text-emerald-700'
                : 'bg-amber-500/10 text-amber-700',
            )}
          >
            {state === 'active' && isComplete ? 'active' : 'draft'}
          </span>
        </span>
        <span className="mt-0.5 block truncate text-[12px] text-muted-foreground">
          {accountName(accounts, draft.accountId)} ·{' '}
          {triggerLabel(draft.triggers[0] ?? 'messageArrived')} ·{' '}
          {ruleActionSummary(draft)}
        </span>
      </span>
      <span className="shrink-0 text-[12px] text-muted-foreground/70">
        Edit
      </span>
    </button>
  )
}

function AutomationRuleEditor({
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
}: {
  draft: AutomationRuleDraft
  state: AutomationRuleState
  accounts: AccountOverview[]
  staticMailboxes: Mailbox[] | null
  canEditAccount: boolean
  previewCondition: SmartMailboxRule
  savePending: boolean
  onBack: () => void
  onSave: () => void
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

      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <p className="truncate text-[14px] font-semibold text-foreground">
            {draft.name.trim() || 'Untitled rule'}
          </p>
          <p className="mt-1 text-[12px] text-muted-foreground">{saveStatus}</p>
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
            variant="ghost"
            className="h-8 px-2 text-muted-foreground hover:text-destructive"
            onClick={onRemove}
            disabled={savePending}
          >
            Remove
          </Button>
        </div>
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
        <RuleGroupEditor
          group={draft.condition.root}
          onChange={(root) => onChange({ condition: { root } })}
        />
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
    <div className="space-y-3">
      <div className="flex justify-end">
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
      <div className="space-y-2">
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
