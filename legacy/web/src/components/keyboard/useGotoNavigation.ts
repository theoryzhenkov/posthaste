/**
 * Wires the pure {@link resolveGotoTarget} logic to live read models and the
 * view-selection handlers, returning a single `goto(role, options)` callback
 * for the keyboard controller.
 *
 * @spec docs/ui/L0#navigation-model
 */
import { useQuery } from '@tanstack/react-query'
import { useCallback } from 'react'

import { useAccountDirectory } from '@/accountDirectory'
import type { Mailbox, SmartMailboxSummary } from '@/api/types'
import type { SidebarSelection } from '@/components/Sidebar'
import { queryKeys } from '@/queryKeys'
import { runtimeViews } from '@/runtime/views'

import { resolveGotoTarget, type GotoRole } from './goto'

export interface GotoNavigation {
  goto: (role: GotoRole, options: { forceSmart: boolean }) => void
}

export function useGotoNavigation(input: {
  effectiveView: SidebarSelection | null
  onSelectSmartMailbox: (smartMailboxId: string, name: string) => void
  onSelectSourceMailbox: (
    sourceId: string,
    mailboxId: string,
    name: string,
  ) => void
}): GotoNavigation {
  const { effectiveView, onSelectSmartMailbox, onSelectSourceMailbox } = input
  const accountDirectory = useAccountDirectory()

  // Cache-only observers seeded by the navigation bootstrap; no fetch of their
  // own beyond the current source's mailboxes when in a source-mailbox view.
  const { data: smartMailboxes } = useQuery<SmartMailboxSummary[]>({
    queryKey: queryKeys.smartMailboxes,
    queryFn: runtimeViews.smartMailboxes.list,
    enabled: false,
  })
  const sourceId =
    effectiveView?.kind === 'source-mailbox' ? effectiveView.sourceId : null
  const { data: sourceMailboxes } = useQuery<Mailbox[]>({
    queryKey: queryKeys.mailboxes(sourceId),
    queryFn: () => runtimeViews.mail.mailboxes(sourceId!),
    enabled: sourceId !== null,
  })

  const goto = useCallback<GotoNavigation['goto']>(
    (role, { forceSmart }) => {
      const target = resolveGotoTarget({
        effectiveView,
        role,
        forceSmart,
        sourceMailboxes: sourceMailboxes ?? [],
        smartMailboxes: smartMailboxes ?? [],
      })
      if (!target) return
      if (target.kind === 'smart-mailbox') {
        onSelectSmartMailbox(target.id, target.name)
        return
      }
      const accountName = accountDirectory.resolveAccountName(target.sourceId)
      onSelectSourceMailbox(
        target.sourceId,
        target.mailboxId,
        `${accountName} / ${target.mailboxName}`,
      )
    },
    [
      accountDirectory,
      effectiveView,
      onSelectSmartMailbox,
      onSelectSourceMailbox,
      smartMailboxes,
      sourceMailboxes,
    ],
  )

  return { goto }
}
