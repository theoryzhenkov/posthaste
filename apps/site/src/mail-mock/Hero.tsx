import { useEffect, useMemo, useState } from 'react'
import type { SiteMessage } from '../content/types'
import type { MessagePreview, PersistedMockState, MailboxView } from './types'
import { mailboxCounts } from './types'
import {
  loadPersistedMockState,
  MOCK_STATE_STORAGE_KEY,
  persistedSet,
} from './state'
import { ClientToolbar } from './ClientToolbar'
import { MessageListPreview } from './MessageListPreview'
import { ReaderPreview } from './ReaderPreview'
import { SidebarPreview } from './SidebarPreview'

export function Hero({ messages }: { messages: SiteMessage[] }) {
  const persistedState = useMemo(() => loadPersistedMockState(), [])
  const [selectedMailbox, setSelectedMailbox] = useState<MailboxView>(
    persistedState.selectedMailbox === 'archive' ? 'archive' : 'inbox',
  )
  const initialSelectedId =
    persistedState.selectedMessageId ?? messages[0]?.id ?? null
  const [selectedMessageId, setSelectedMessageId] = useState<string | null>(
    initialSelectedId,
  )
  const [readMessageIds, setReadMessageIds] = useState<ReadonlySet<string>>(
    () => {
      // The message that opens on load is focused, so it starts read.
      const seed = persistedSet(persistedState.readMessageIds)
      return initialSelectedId ? new Set(seed).add(initialSelectedId) : seed
    },
  )
  const [archivedMessageIds, setArchivedMessageIds] = useState<
    ReadonlySet<string>
  >(() => persistedSet(persistedState.archivedMessageIds))
  const [flaggedMessageIds, setFlaggedMessageIds] = useState<
    ReadonlySet<string>
  >(() => persistedSet(persistedState.flaggedMessageIds))
  const previewMessages = useMemo<MessagePreview[]>(
    () =>
      messages.map((message) => ({
        ...message,
        archived: archivedMessageIds.has(message.id),
        flagged: flaggedMessageIds.has(message.id),
        unread: !readMessageIds.has(message.id),
      })),
    [archivedMessageIds, flaggedMessageIds, messages, readMessageIds],
  )
  const visibleMessages = useMemo(
    () =>
      previewMessages.filter((message) =>
        selectedMailbox === 'archive' ? message.archived : !message.archived,
      ),
    [previewMessages, selectedMailbox],
  )
  const selectedMessage =
    visibleMessages.find((message) => message.id === selectedMessageId) ??
    visibleMessages[0] ??
    null
  const counts = mailboxCounts(previewMessages)

  useEffect(() => {
    const state: PersistedMockState = {
      selectedMailbox,
      selectedMessageId,
      readMessageIds: [...readMessageIds],
      archivedMessageIds: [...archivedMessageIds],
      flaggedMessageIds: [...flaggedMessageIds],
    }
    window.localStorage.setItem(MOCK_STATE_STORAGE_KEY, JSON.stringify(state))
  }, [
    archivedMessageIds,
    flaggedMessageIds,
    readMessageIds,
    selectedMailbox,
    selectedMessageId,
  ])

  function handleSelectMessage(messageId: string) {
    setSelectedMessageId(messageId)
    setReadMessageIds((current) => new Set(current).add(messageId))
  }

  function handleSelectMailbox(mailbox: MailboxView) {
    setSelectedMailbox(mailbox)
    const nextMessage = previewMessages.find((message) =>
      mailbox === 'archive' ? message.archived : !message.archived,
    )
    setSelectedMessageId(nextMessage?.id ?? null)
    if (nextMessage) {
      setReadMessageIds((current) => new Set(current).add(nextMessage.id))
    }
  }

  function handleArchiveSelected() {
    if (!selectedMessage || selectedMessage.archived) return
    const nextMessage = visibleMessages.find(
      (message) => message.id !== selectedMessage.id,
    )
    setArchivedMessageIds((current) => new Set(current).add(selectedMessage.id))
    setSelectedMessageId(nextMessage?.id ?? null)
    if (nextMessage) {
      setReadMessageIds((current) => new Set(current).add(nextMessage.id))
    }
  }

  function handleToggleFlagSelected() {
    if (!selectedMessage) return
    setFlaggedMessageIds((current) => {
      const next = new Set(current)
      if (next.has(selectedMessage.id)) {
        next.delete(selectedMessage.id)
      } else {
        next.add(selectedMessage.id)
      }
      return next
    })
  }

  return (
    <section className="hero" aria-labelledby="hero-title">
      <div className="client-frame is-visible" data-reveal>
        <ClientToolbar
          canArchive={selectedMailbox === 'inbox' && selectedMessage !== null}
          isFlagged={selectedMessage?.flagged ?? false}
          isMessageSelected={selectedMessage !== null}
          onArchive={handleArchiveSelected}
          onToggleFlag={handleToggleFlagSelected}
        />
        <div className="client-body">
          <SidebarPreview
            counts={counts}
            selectedMailbox={selectedMailbox}
            onSelectMailbox={handleSelectMailbox}
          />
          <MessageListPreview
            messages={visibleMessages}
            selectedMessageId={selectedMessage?.id ?? null}
            selectedMailbox={selectedMailbox}
            onSelectMessage={handleSelectMessage}
          />
          <ReaderPreview message={selectedMessage} />
        </div>
      </div>
    </section>
  )
}
