/**
 * Shared automation action editors for account and smart-mailbox settings.
 *
 * Automation rules are persisted globally in app settings. Account and smart
 * mailbox editors project their UI context into normal query conditions.
 *
 */
import type { AccountRow } from '@/gen'
import type {
  AppSettings,
  Mailbox,
  SmartMailbox,
} from '../../api/types'
import {
  actionConditionFromSourceMailboxRule,
  actionConditionFromSmartMailboxRule,
  extractAccountIdFromRule,
  isSourceMailboxLinkedRule,
  isSmartMailboxLinkedRule,
  sourceMailboxDraftToRule,
  sourceMailboxRulePrefix,
  smartMailboxDraftToRule,
  smartMailboxRulePrefix,
} from '../../automationRules'
import { defaultDraft } from './automationRuleHelpers'
import { LinkedAutomationRuleFields } from './automation-actions/linkedAutomationRules'
import { linkedAutomationRuleItems } from './automation-actions/linkedAutomationRuleItems'

export function SmartMailboxAutomationFields({
  accounts,
  settings,
  smartMailbox,
  disabledReason,
  onSaved,
}: {
  accounts: AccountRow[]
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
  account: AccountRow
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
