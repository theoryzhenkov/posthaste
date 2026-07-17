import { useMemo } from 'react'
import { useDefaultLayout } from 'react-resizable-panels'

const SHELL_PANEL_IDS = ['sidebar', 'mail-content']

export function useMailLayoutPersistence(isMessageDetailOpen: boolean) {
  const messagePanelIds = useMemo(
    () =>
      isMessageDetailOpen
        ? ['message-list', 'message-detail']
        : ['message-list'],
    [isMessageDetailOpen],
  )
  const {
    defaultLayout: shellDefaultLayout,
    onLayoutChanged: onShellLayoutChanged,
  } = useDefaultLayout({
    id: 'posthaste-shell-panels',
    panelIds: SHELL_PANEL_IDS,
    storage: localStorage,
  })
  const {
    defaultLayout: messageDefaultLayout,
    onLayoutChanged: onMessageLayoutChanged,
  } = useDefaultLayout({
    id: 'posthaste-message-panels',
    panelIds: messagePanelIds,
    storage: localStorage,
  })

  return {
    messageDefaultLayout,
    onMessageLayoutChanged,
    onShellLayoutChanged,
    shellDefaultLayout,
  }
}
