import { useMutation } from '@tanstack/react-query'
import { useState } from 'react'
import type {
  AccountOverview,
  AppSettings,
  AutomationRule,
  Mailbox,
  SmartMailboxRule,
} from '../../../api/types'
import type { AutomationRuleDraft } from '../../../automationRules'
import { runtimeMutations } from '../../../runtime/mutations'
import {
  isDraftComplete,
  removeRule,
  upsertRule,
  type AutomationRuleItem,
} from '../automationRuleHelpers'
import { AutomationRuleList } from './AutomationRuleList'

export function LinkedAutomationRuleFields({
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
    mutationFn: (input: Partial<AppSettings>) =>
      runtimeMutations.settings.patch(input),
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
