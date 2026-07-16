import type React from 'react'
import { useState } from 'react'
import { createPortal } from 'react-dom'
import type {
  AccountOverview,
  Mailbox,
  MailQueryRule,
} from '../../../api/types'
import type { AutomationRuleDraft } from '../../../automationRules'
import { cn } from '../../../lib/utils'
import { Button } from '../../ui/button'
import {
  accountName,
  actionListDescription,
  isDraftComplete,
  ruleActionSummary,
  triggerLabel,
  type AutomationRuleItem,
} from '../automationRuleHelpers'
import { FeedbackBanner } from '../shared'
import { AutomationRuleEditor } from './AutomationRuleEditor'

export function AutomationRuleList({
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
  onBackfillItem,
  backfillNoticeFor,
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
  onBackfillItem: (draft: AutomationRuleDraft) => void
  backfillNoticeFor: string | null
  previewConditionForDraft: (draft: AutomationRuleDraft) => MailQueryRule
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
            onBackfill={() => onBackfillItem(editingItem.draft)}
            backfillNoticeFor={backfillNoticeFor}
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
    <div className="fixed inset-0 z-(--z-surface) bg-background text-card-foreground">
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
