import { useCallback } from 'react'

import { getAction, type ActionContext, type ActionServices } from '@/commands'
import type { PaletteAction } from '@/components/palette/search/types'
import { recordCommandUse } from '@/components/palette/search/recent/recentCommands'
import type { MailSelection } from '@/data/models/selection'

/**
 * The palette's single execution path.
 *
 * Registry rows (`kind: 'action'`) dispatch through the resolved action's
 * `run(ctx, services)` — the same code path the context menu uses — and bump
 * the recency counter. Everything else here is pure search-result navigation
 * emitted by the other providers (mailboxes/messages/tags/query), which are not
 * registry actions.
 */
export interface PaletteNavHandlers {
  onApplySearch: (query: string) => void
  onSelectMessage: (selection: MailSelection) => void
  onSelectSmartMailbox: (smartMailboxId: string, name: string) => void
  onSelectSourceMailbox: (
    sourceId: string,
    mailboxId: string,
    name: string,
  ) => void
  replaceQuery: (query: string) => void
  /** Push the palette into a parameterized action's pick-step (the two-step
   *  flow: pick the command, then pick its target). Optional so non-palette
   *  hosts of this hook need not care. */
  openActionParams?: (actionId: string) => void
}

export function usePaletteActions(input: {
  actionContext: ActionContext
  services: ActionServices
  nav: PaletteNavHandlers
}) {
  const { actionContext, services, nav } = input
  return useCallback(
    (action: PaletteAction) => {
      switch (action.kind) {
        case 'action': {
          const def = getAction(action.actionId)
          if (!def) break
          recordCommandUse(action.actionId)
          void def.run(actionContext, services)
          break
        }
        case 'open-action-params':
          // Two-step flow, step 1: a parameterized action never runs bare —
          // the palette swaps into its searchable pick-step.
          nav.openActionParams?.(action.actionId)
          break
        case 'run-action-param': {
          // Two-step flow, step 2: run with the chosen option — the SAME
          // `def.run` the context submenu / header popover invoke.
          const def = getAction(action.actionId)
          if (!def) break
          recordCommandUse(action.actionId)
          void def.run(actionContext, services, action.param)
          break
        }
        case 'apply-query':
          nav.onApplySearch(action.query)
          break
        case 'replace-query':
          nav.replaceQuery(action.query)
          break
        case 'open-source-mailbox':
          nav.onSelectSourceMailbox(
            action.sourceId,
            action.mailboxId,
            action.name,
          )
          break
        case 'open-smart-mailbox':
          nav.onSelectSmartMailbox(action.smartMailboxId, action.name)
          break
        case 'open-message':
          if (action.mailboxHint) {
            nav.onSelectSourceMailbox(
              action.sourceId,
              action.mailboxHint.mailboxId,
              action.mailboxHint.name,
            )
          }
          nav.onSelectMessage({
            conversationId: action.conversationId,
            sourceId: action.sourceId,
            messageId: action.messageId,
          })
          break
      }
    },
    [actionContext, services, nav],
  )
}
