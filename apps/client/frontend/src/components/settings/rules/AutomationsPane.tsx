/**
 * The Automations settings pane: every automation rule in the settings
 * document, listed and edited in one place. Rules live on
 * `settings.automationRules` (drafts on `automationDrafts`) and are written
 * read-modify-write through the `updateSettings` command; the shared
 * {@link LinkedAutomationRuleFields} machinery supplies the list, the
 * drill-in editor (WHEN-clause grammar builder + action list), preview, and
 * backfill.
 *
 * Rules created from a smart mailbox or a source mailbox editor also appear
 * here — this pane is the unscoped superset.
 */
import { Workflow } from 'lucide-react'

import { useAccounts, useAppSettings } from '@/data'
import {
  actionConditionFromAccountRule,
  extractAccountIdFromRule,
  draftToRule,
} from '../../../domain/automation/index'
import { defaultDraft } from './model'
import { LinkedAutomationRuleFields } from './actions/linkedAutomationRules'
import { linkedAutomationRuleItems } from './actions/linkedAutomationRuleItems'
import {
  SettingsEmptyState,
  SettingsPage,
  SettingsPageHeader,
} from '../panel/shared'

export function AutomationsPane() {
  const settingsQuery = useAppSettings()
  const accountsQuery = useAccounts()
  const settings = settingsQuery.data?.settings ?? null
  const accounts = accountsQuery.data?.rows ?? []
  const fallbackAccountId = accounts[0]?.id ?? ''

  return (
    <SettingsPage>
      <SettingsPageHeader
        title="Automations"
        description="Rules that react to your mail on the backend — tag, move, or flag matching messages. Rules run whenever a matching message arrives, even with every window closed."
      />

      {settings === null ? (
        settingsQuery.isError ? (
          <SettingsEmptyState
            icon={<Workflow size={34} strokeWidth={1.4} />}
            title="Settings unavailable"
            description={
              settingsQuery.error instanceof Error
                ? settingsQuery.error.message
                : 'The settings document could not be loaded.'
            }
          />
        ) : (
          <p className="text-[13px] text-muted-foreground">Loading rules.</p>
        )
      ) : (
        <LinkedAutomationRuleFields
          accounts={accounts}
          canEditAccount
          settings={settings}
          addLabel="New rule"
          emptyText="No automations yet. Create a rule to tag, move, or flag a message when it arrives."
          addDisabled={accounts.length === 0}
          disabledReason={
            accounts.length === 0 ? 'Add an account first.' : null
          }
          onSaved={async () => {}}
          itemsFromSettings={(sourceSettings) =>
            linkedAutomationRuleItems({
              rules: sourceSettings.automationRules ?? [],
              drafts: sourceSettings.automationDrafts ?? [],
              isLinkedRule: () => true,
              accountIdForRule: (rule) =>
                extractAccountIdFromRule(rule, fallbackAccountId),
              conditionForRule: actionConditionFromAccountRule,
            })
          }
          createDraft={() => {
            const account = accounts[0]
            if (!account) {
              return null
            }
            return defaultDraft({ accountId: account.id, name: 'New rule' })
          }}
          draftToRule={draftToRule}
          previewConditionForDraft={(draft) => draftToRule(draft).condition}
        />
      )}
    </SettingsPage>
  )
}
