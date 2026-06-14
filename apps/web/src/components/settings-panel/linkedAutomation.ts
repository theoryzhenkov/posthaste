import type { QueryClient } from '@tanstack/react-query'

import { patchSettings } from '../../api/client'
import type { AppSettings, SmartMailbox } from '../../api/types'
import {
  removeSmartMailboxLinkedRules,
  rewriteSmartMailboxLinkedRules,
} from '../../automationRules'
import { queryKeys } from '../../queryKeys'

export async function rewriteLinkedSmartMailboxAutomation(input: {
  queryClient: QueryClient
  settings: AppSettings | null
  smartMailbox: SmartMailbox
}) {
  const { queryClient, settings, smartMailbox } = input
  if (!settings) {
    return
  }
  const automationRules = rewriteSmartMailboxLinkedRules(
    settings.automationRules ?? [],
    smartMailbox,
  )
  const automationDrafts = rewriteSmartMailboxLinkedRules(
    settings.automationDrafts ?? [],
    smartMailbox,
  )
  if (
    automationRules === settings.automationRules &&
    automationDrafts === settings.automationDrafts
  ) {
    return
  }
  const savedSettings = await patchSettings({ automationRules, automationDrafts })
  queryClient.setQueryData(queryKeys.settings, savedSettings)
}

export async function removeLinkedSmartMailboxAutomation(input: {
  queryClient: QueryClient
  settings: AppSettings | null
  smartMailboxId: string
}) {
  const { queryClient, settings, smartMailboxId } = input
  if (!settings) {
    return
  }
  const automationRules = removeSmartMailboxLinkedRules(
    settings.automationRules ?? [],
    smartMailboxId,
  )
  const automationDrafts = removeSmartMailboxLinkedRules(
    settings.automationDrafts ?? [],
    smartMailboxId,
  )
  if (
    automationRules === settings.automationRules &&
    automationDrafts === settings.automationDrafts
  ) {
    return
  }
  const savedSettings = await patchSettings({ automationRules, automationDrafts })
  queryClient.setQueryData(queryKeys.settings, savedSettings)
}
