/**
 * Wires the pure {@link resolveGotoTarget} logic to live read models and the
 * view-selection handlers, returning a single `goto(role, options)` callback
 * for the keyboard controller.
 *
 */
import { useCallback, useMemo } from 'react'

import { useAccountDirectory } from '@/data/models/accountDirectory'
import type { SidebarSelection } from '@/components/sidebar/Sidebar'
import { useMailboxCounts, useSmartMailboxes } from '@/data/queries/queries'

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

  // The same answers the sidebar renders, deduped by the shared family keys:
  // the smart-mailbox list, and the current source's mailboxes when in a
  // source-mailbox view.
  const smartMailboxes = useSmartMailboxes().data?.rows
  const sourceId =
    effectiveView?.kind === 'source-mailbox' ? effectiveView.sourceId : null
  const mailboxCounts = useMailboxCounts(sourceId ?? undefined, {
    enabled: sourceId !== null,
  })
  const countRows = mailboxCounts.data?.rows
  const sourceMailboxes = useMemo(
    () =>
      sourceId
        ? (countRows ?? [])
            .filter((row) => row.accountId === sourceId)
            .map((row) => row.mailbox)
        : [],
    [countRows, sourceId],
  )

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
