// Keeping smart-mailbox-linked automation rules in step with their mailbox:
// when a smart mailbox is saved its linked rules are rewritten to the new
// rule/name, and when it is deleted they are dropped. Both are
// read-modify-write settings updates through the `updateSettings` command.

import type { QueryClient } from '@tanstack/react-query'

import type { AppSettings, SmartMailboxRow } from '@/gen'
import type { MailClient } from '@/data/transport/client'
import { runCommand } from '@/data/transport/commands'
import {
  removeSmartMailboxLinkedRules,
  rewriteSmartMailboxLinkedRules,
} from '../../../domain/automation/index'

async function saveAutomation(
  client: MailClient,
  queryClient: QueryClient,
  settings: AppSettings,
  automationRules: AppSettings['automationRules'],
  automationDrafts: AppSettings['automationDrafts'],
) {
  if (
    automationRules === settings.automationRules &&
    automationDrafts === settings.automationDrafts
  ) {
    return
  }
  await runCommand(client, queryClient, {
    updateSettings: {
      settings: { ...settings, automationRules, automationDrafts },
      forceBackfill: false,
    },
  })
}

export async function rewriteLinkedSmartMailboxAutomation(input: {
  client: MailClient
  queryClient: QueryClient
  settings: AppSettings | null
  smartMailbox: SmartMailboxRow
}) {
  const { client, queryClient, settings, smartMailbox } = input
  if (!settings) {
    return
  }
  await saveAutomation(
    client,
    queryClient,
    settings,
    rewriteSmartMailboxLinkedRules(settings.automationRules ?? [], smartMailbox),
    rewriteSmartMailboxLinkedRules(settings.automationDrafts ?? [], smartMailbox),
  )
}

export async function removeLinkedSmartMailboxAutomation(input: {
  client: MailClient
  queryClient: QueryClient
  settings: AppSettings | null
  smartMailboxId: string
}) {
  const { client, queryClient, settings, smartMailboxId } = input
  if (!settings) {
    return
  }
  await saveAutomation(
    client,
    queryClient,
    settings,
    removeSmartMailboxLinkedRules(settings.automationRules ?? [], smartMailboxId),
    removeSmartMailboxLinkedRules(settings.automationDrafts ?? [], smartMailboxId),
  )
}
