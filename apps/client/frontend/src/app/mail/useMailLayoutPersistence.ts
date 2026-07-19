import { useMemo } from 'react'
import { useDefaultLayout } from 'react-resizable-panels'

import { ambientStorage } from '@/lib/ambient/storage'

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
    storage: ambientStorage() ?? undefined,
  })
  const {
    defaultLayout: messageDefaultLayout,
    onLayoutChanged: onMessageLayoutChanged,
  } = useDefaultLayout({
    id: 'posthaste-message-panels',
    panelIds: messagePanelIds,
    storage: ambientStorage() ?? undefined,
  })

  return {
    messageDefaultLayout,
    onMessageLayoutChanged,
    onShellLayoutChanged,
    shellDefaultLayout,
  }
}
