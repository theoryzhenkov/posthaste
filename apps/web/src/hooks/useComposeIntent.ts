import { useCallback, useState } from 'react'

import type { ComposeIntent } from '@/components/ComposeOverlay'
import type { SidebarSelection } from '@/components/Sidebar'
import type { MailSelection } from '@/mailState'

interface EnabledAccountRef {
  id: string
}

export function useComposeIntent({
  enabledAccounts,
  selectedMessage,
  selectedView,
  onMissingSource,
}: {
  enabledAccounts: EnabledAccountRef[]
  selectedMessage: MailSelection | null
  selectedView: SidebarSelection | null
  onMissingSource: () => void
}) {
  const [composeIntent, setComposeIntent] = useState<ComposeIntent | null>(null)

  const resolveComposeSourceId = useCallback(() => {
    return (
      selectedMessage?.sourceId ??
      (selectedView?.kind === 'source-mailbox'
        ? selectedView.sourceId
        : null) ??
      enabledAccounts[0]?.id ??
      null
    )
  }, [enabledAccounts, selectedMessage, selectedView])

  const openCompose = useCallback(() => {
    const sourceId = resolveComposeSourceId()
    if (!sourceId) {
      onMissingSource()
      return
    }
    setComposeIntent({ kind: 'new', sourceId })
  }, [onMissingSource, resolveComposeSourceId])

  const replyToSelectedMessage = useCallback(() => {
    if (!selectedMessage) {
      return
    }
    setComposeIntent({
      kind: 'reply',
      sourceId: selectedMessage.sourceId,
      messageId: selectedMessage.messageId,
    })
  }, [selectedMessage])

  const forwardSelectedMessage = useCallback(() => {
    if (!selectedMessage) {
      return
    }
    setComposeIntent({
      kind: 'forward',
      sourceId: selectedMessage.sourceId,
      messageId: selectedMessage.messageId,
    })
  }, [selectedMessage])

  const closeCompose = useCallback(() => {
    setComposeIntent(null)
  }, [])

  return {
    closeCompose,
    composeIntent,
    forwardSelectedMessage,
    openCompose,
    replyToSelectedMessage,
  }
}
