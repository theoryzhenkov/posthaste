import { useCallback, useState } from 'react'

import type { ComposeIntent } from '@/domain/composeIntent'
import type { SidebarSelection } from '@/data/models/selection'
import type { MailSelection } from '@/data/models/selection'

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

  const replyAllToSelectedMessage = useCallback(() => {
    if (!selectedMessage) {
      return
    }
    setComposeIntent({
      kind: 'replyAll',
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

  const editDraft = useCallback((sourceId: string, messageId: string) => {
    setComposeIntent({ kind: 'draft', sourceId, messageId })
  }, [])

  /** Open the composer prefilled from a `mailto:` URI (the List-Unsubscribe
   *  mailto path). The user reviews and sends — nothing is auto-sent. */
  const composeMailto = useCallback((sourceId: string, mailtoUri: string) => {
    setComposeIntent({ kind: 'mailto', sourceId, mailtoUri })
  }, [])

  const editSelectedDraft = useCallback(() => {
    if (!selectedMessage) {
      return
    }
    setComposeIntent({
      kind: 'draft',
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
    composeMailto,
    editDraft,
    editSelectedDraft,
    forwardSelectedMessage,
    openCompose,
    replyAllToSelectedMessage,
    replyToSelectedMessage,
  }
}
