import { useEffect, useRef } from 'react'

import type { EmailActions } from '@/hooks/useEmailActions'
import type { MailSelection } from '@/mailState'

interface AutoMarkReadMessage {
  isFlagged: boolean
  isRead: boolean
  keywords: string[]
}

export function useAutoMarkRead(
  selection: MailSelection | null,
  message: AutoMarkReadMessage | undefined,
  actions: Pick<EmailActions, 'markRead'>,
) {
  const lastAutoSeenKeyRef = useRef<string | null>(null)

  useEffect(() => {
    if (!selection || !message) {
      return
    }
    const selectionKey = `${selection.sourceId}:${selection.messageId}`
    if (lastAutoSeenKeyRef.current === selectionKey) {
      return
    }
    lastAutoSeenKeyRef.current = selectionKey

    if (message.isRead) {
      return
    }

    actions.markRead({
      conversationId: selection.conversationId,
      sourceId: selection.sourceId,
      messageId: selection.messageId,
      isFlagged: message.isFlagged,
      isRead: message.isRead,
      keywords: message.keywords,
    })
  }, [actions, message, selection])
}
