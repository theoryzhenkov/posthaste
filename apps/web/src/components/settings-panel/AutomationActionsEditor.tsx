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
  AutomationRule,
  AutomationTrigger,
  Mailbox,
  SmartMailbox,
  SmartMailboxGroup,
  SmartMailboxRule,
} from '../../api/types'
import { fetchMailboxes, patchSettings } from '../../api/client'
import { queryKeys } from '../../queryKeys'
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

const SMART_MAILBOX_RULE_PREFIX = 'smart-mailbox'

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

interface AutomationRuleDraft {
  id: string
  accountId: string
  name: string
  enabled: boolean
  triggers: AutomationTrigger[]
  condition: SmartMailboxRule
  actions: AutomationAction[]
  backfill: boolean
}

function createRuleId(prefix = 'automation'): string {
  if (globalThis.crypto && 'randomUUID' in globalThis.crypto) {
    return `${prefix}:${globalThis.crypto.randomUUID()}`
  }
  return `${prefix}:${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`
}

function cloneRule(rule: SmartMailboxRule): SmartMailboxRule {
  return JSON.parse(JSON.stringify(rule)) as SmartMailboxRule
}

function groupNode(group: SmartMailboxGroup) {
  return {
    type: 'group' as const,
    operator: group.operator,
    negated: group.negated,
    nodes: group.nodes,
  }
}

function sourceConditionNode(accountId: string) {
  return {
    type: 'condition' as const,
    field: 'sourceId' as const,
    operator: 'equals' as const,
    negated: false,
    value: accountId,
  }
}

function isSourceConditionForAccount(
  node: SmartMailboxGroup['nodes'][number] | undefined,
  accountId: string,
): boolean {
  return (
    node?.type === 'condition' &&
    node.field === 'sourceId' &&
    node.operator === 'equals' &&
    !node.negated &&
    typeof node.value === 'string' &&
    node.value === accountId
  )
}

function extractAccountIdFromRule(
  rule: AutomationRule,
  fallbackAccountId: string,
): string {
  const sourceNode = rule.condition.root.nodes.find(
    (node) =>
      node.type === 'condition' &&
      node.field === 'sourceId' &&
      node.operator === 'equals' &&
      !node.negated &&
      typeof node.value === 'string' &&
      node.value.trim().length > 0,
  )
  return sourceNode?.type === 'condition' &&
    typeof sourceNode.value === 'string'
    ? sourceNode.value
    : fallbackAccountId
}

function accountScopedCondition(
  rule: SmartMailboxRule,
  accountId: string,
): SmartMailboxRule {
  return {
    root: {
      operator: 'all',
      negated: false,
      nodes: [sourceConditionNode(accountId), groupNode(cloneRule(rule).root)],
    },
  }
}

function actionConditionFromAccountRule(
  rule: AutomationRule,
  accountId: string,
): SmartMailboxRule {
  const nodes = rule.condition.root.nodes
  const secondNode = nodes[1]
  if (
    rule.condition.root.operator === 'all' &&
    !rule.condition.root.negated &&
    isSourceConditionForAccount(nodes[0], accountId) &&
    secondNode?.type === 'group'
  ) {
    return {
      root: {
        operator: secondNode.operator,
        negated: secondNode.negated,
        nodes: secondNode.nodes,
      },
    }
  }
  return cloneRule(rule.condition)
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

function normalizeAction(action: AutomationAction): AutomationAction {
  switch (action.kind) {
    case 'applyTag':
      return { kind: 'applyTag', tag: action.tag.trim() }
    case 'removeTag':
      return { kind: 'removeTag', tag: action.tag.trim() }
    case 'moveToMailbox':
      return { kind: 'moveToMailbox', mailboxId: action.mailboxId.trim() }
    case 'markRead':
    case 'markUnread':
    case 'flag':
    case 'unflag':
      return action
  }
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

function ruleToDraft(
  accountId: string,
  rule: AutomationRule,
): AutomationRuleDraft {
  return {
    id: rule.id,
    accountId,
    name: rule.name,
    enabled: rule.enabled,
    triggers: rule.triggers.length > 0 ? rule.triggers : ['messageArrived'],
    condition: actionConditionFromAccountRule(rule, accountId),
    actions: rule.actions.map(normalizeAction),
    backfill: rule.backfill,
  }
}

function draftToRule(draft: AutomationRuleDraft): AutomationRule {
  return {
    id: draft.id.trim(),
    name: draft.name.trim(),
    enabled: draft.enabled,
    triggers: draft.triggers.length > 0 ? draft.triggers : ['messageArrived'],
    condition: accountScopedCondition(draft.condition, draft.accountId),
    actions: draft.actions.map(normalizeAction),
    backfill: draft.backfill,
  }
}

function accountRulePrefix(accountId: string): string {
  return `account:${accountId}:`
}

function smartMailboxRulePrefix(smartMailboxId: string): string {
  return `${SMART_MAILBOX_RULE_PREFIX}:${smartMailboxId}:`
}

function isSmartMailboxLinkedRule(
  rule: AutomationRule,
  smartMailboxId: string,
): boolean {
  return rule.id.startsWith(smartMailboxRulePrefix(smartMailboxId))
}

function actionConditionFromSmartMailboxRule(
  rule: AutomationRule,
  accountId: string,
): SmartMailboxRule {
  const nodes = rule.condition.root.nodes
  const thirdNode = nodes[2]
  if (
    rule.condition.root.operator === 'all' &&
    !rule.condition.root.negated &&
    isSourceConditionForAccount(nodes[0], accountId) &&
    nodes[1]?.type === 'group' &&
    thirdNode?.type === 'group'
  ) {
    return {
      root: {
        operator: thirdNode.operator,
        negated: thirdNode.negated,
        nodes: thirdNode.nodes,
      },
    }
  }
  return cloneRule(rule.condition)
}

function smartMailboxDraftToRule(
  draft: AutomationRuleDraft,
  smartMailbox: SmartMailbox,
): AutomationRule {
  return {
    ...draftToRule(draft),
    condition: accountScopedCondition(
      {
        root: {
          operator: 'all',
          negated: false,
          nodes: [
            groupNode(cloneRule(smartMailbox.rule).root),
            groupNode(cloneRule(draft.condition).root),
          ],
        },
      },
      draft.accountId,
    ),
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
}) {
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
        <div className="space-y-6">
          {drafts.map((draft) => (
            <AutomationRuleRow
              key={draft.id}
              draft={draft}
              accounts={accounts}
              staticMailboxes={mailboxesByAccount[draft.accountId] ?? null}
              canEditAccount={canEditAccount}
              onChange={(patch) => updateDraft(draft.id, patch)}
              onRemove={() =>
                onChange(
                  drafts.filter((candidate) => candidate.id !== draft.id),
                )
              }
            />
          ))}
        </div>
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

function AutomationRuleRow({
  draft,
  accounts,
  staticMailboxes,
  canEditAccount,
  onChange,
  onRemove,
}: {
  draft: AutomationRuleDraft
  accounts: AccountOverview[]
  staticMailboxes: Mailbox[] | null
  canEditAccount: boolean
  onChange: (patch: Partial<AutomationRuleDraft>) => void
  onRemove: () => void
}) {
  return (
    <div className="space-y-4 border-t border-border-soft pt-4 first:border-t-0 first:pt-0">
      <div className="grid gap-3 lg:grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)_minmax(0,1fr)_auto] lg:items-end">
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

        <Button
          type="button"
          size="sm"
          variant="ghost"
          className="h-8 justify-self-start px-2 text-muted-foreground hover:text-destructive lg:justify-self-end"
          onClick={onRemove}
        >
          Remove
        </Button>
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

      <ActionListEditor
        accountId={draft.accountId}
        actions={draft.actions}
        staticMailboxes={staticMailboxes}
        onChange={(actions) => onChange({ actions })}
      />
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
