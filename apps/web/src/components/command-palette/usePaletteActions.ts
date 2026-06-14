import { useCallback } from 'react'

import type { PaletteAction } from '@/command-search/types'
import type { MailSelection } from '@/mailState'
import type { SettingsSurfaceCategory as SettingsCategory } from '@/surfaces'

export interface PaletteActionHandlers {
  onApplySearch: (query: string) => void
  onArchive: () => void
  onCompose: () => void
  onOpenSettings: (category?: SettingsCategory) => void
  onOpenShortcuts: () => void
  onPlaceholderAction: (label: string) => void
  onReply: () => void
  onSelectMessage: (selection: MailSelection) => void
  onSelectSmartMailbox: (smartMailboxId: string, name: string) => void
  onSelectSourceMailbox: (
    sourceId: string,
    mailboxId: string,
    name: string,
  ) => void
  onToggleFlag: () => void
  replaceQuery: (query: string) => void
}

export function usePaletteActions(handlers: PaletteActionHandlers) {
  return useCallback(
    (action: PaletteAction) => {
      switch (action.kind) {
        case 'command':
          executeCommandAction(action.commandId, handlers)
          break
        case 'apply-query':
          handlers.onApplySearch(action.query)
          break
        case 'replace-query':
          handlers.replaceQuery(action.query)
          break
        case 'open-source-mailbox':
          handlers.onSelectSourceMailbox(
            action.sourceId,
            action.mailboxId,
            action.name,
          )
          break
        case 'open-smart-mailbox':
          handlers.onSelectSmartMailbox(action.smartMailboxId, action.name)
          break
        case 'open-message':
          if (action.mailboxHint) {
            handlers.onSelectSourceMailbox(
              action.sourceId,
              action.mailboxHint.mailboxId,
              action.mailboxHint.name,
            )
          }
          handlers.onSelectMessage({
            conversationId: action.conversationId,
            sourceId: action.sourceId,
            messageId: action.messageId,
          })
          break
        case 'open-settings':
          handlers.onOpenSettings(action.category)
          break
        case 'open-compose':
          handlers.onCompose()
          break
        case 'open-contact':
          handlers.onApplySearch(action.query)
          break
        case 'noop':
          handlers.onPlaceholderAction(action.label)
          break
      }
    },
    [handlers],
  )
}

function executeCommandAction(
  commandId: Extract<PaletteAction, { kind: 'command' }>['commandId'],
  handlers: PaletteActionHandlers,
) {
  switch (commandId) {
    case 'compose':
      handlers.onCompose()
      break
    case 'reply':
      handlers.onReply()
      break
    case 'archive':
      handlers.onArchive()
      break
    case 'flag':
      handlers.onToggleFlag()
      break
    case 'shortcuts':
      handlers.onOpenShortcuts()
      break
    case 'snooze':
      handlers.onPlaceholderAction('Snooze')
      break
    case 'newSmart':
    case 'newRule':
      handlers.onOpenSettings('mailboxes')
      break
    case 'settings':
      handlers.onOpenSettings()
      break
    case 'account':
      handlers.onOpenSettings('accounts')
      break
  }
}
