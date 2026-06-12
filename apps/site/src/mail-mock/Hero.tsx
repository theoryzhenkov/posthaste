import { useEffect, useMemo, useState } from 'react'
import type { SiteMessage } from '../content/types'
import type {
  MessagePreview,
  MockTheme,
  PersistedMockState,
  MailboxView,
} from './types'
import { mailboxCounts } from './types'
import {
  isMockTheme,
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
  const [selectedMessageId, setSelectedMessageId] = useState<string | null>(
    persistedState.selectedMessageId ?? messages[0]?.id ?? null,
  )
  const [readMessageIds, setReadMessageIds] = useState<ReadonlySet<string>>(
    () => persistedSet(persistedState.readMessageIds),
  )
  const [archivedMessageIds, setArchivedMessageIds] = useState<
    ReadonlySet<string>
  >(() => persistedSet(persistedState.archivedMessageIds))
  const [flaggedMessageIds, setFlaggedMessageIds] = useState<
    ReadonlySet<string>
  >(() => persistedSet(persistedState.flaggedMessageIds))
  const [mockTheme, setMockTheme] = useState<MockTheme>(
    isMockTheme(persistedState.mockTheme)
      ? persistedState.mockTheme
      : 'baseline',
  )
  const [hasUnlockedSecretTheme, setHasUnlockedSecretTheme] = useState(
    persistedState.hasUnlockedSecretTheme === true,
  )
  const [horseClicks, setHorseClicks] = useState(0)
  const [isHorseLaunching, setIsHorseLaunching] = useState(false)
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

  // Propagate mock theme to <html> so the rest of the page adopts it.
  useEffect(() => {
    const html = document.documentElement
    html.classList.remove('theme-glass', 'theme-typewriter', 'theme-pigeon')
    if (mockTheme !== 'baseline') {
      html.classList.add(`theme-${mockTheme}`)
    }
    return () => {
      html.classList.remove('theme-glass', 'theme-typewriter', 'theme-pigeon')
    }
  }, [mockTheme])

  useEffect(() => {
    const state: PersistedMockState = {
      selectedMailbox,
      selectedMessageId,
      readMessageIds: [...readMessageIds],
      archivedMessageIds: [...archivedMessageIds],
      flaggedMessageIds: [...flaggedMessageIds],
      mockTheme,
      hasUnlockedSecretTheme,
    }
    window.localStorage.setItem(MOCK_STATE_STORAGE_KEY, JSON.stringify(state))
  }, [
    archivedMessageIds,
    flaggedMessageIds,
    hasUnlockedSecretTheme,
    mockTheme,
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

  function handleSecretHorseClick() {
    if (hasUnlockedSecretTheme || isHorseLaunching) return

    const nextClicks = horseClicks + 1
    setHorseClicks(nextClicks)
    if (nextClicks < 5) return

    setIsHorseLaunching(true)
    window.setTimeout(() => {
      setHasUnlockedSecretTheme(true)
      setIsHorseLaunching(false)
    }, 1200)
  }

  return (
    <section className="hero" aria-labelledby="hero-title">
      <div className={`client-frame theme-${mockTheme} is-visible`} data-reveal>
        <ClientToolbar
          canArchive={selectedMailbox === 'inbox' && selectedMessage !== null}
          isFlagged={selectedMessage?.flagged ?? false}
          isMessageSelected={selectedMessage !== null}
          onArchive={handleArchiveSelected}
          selectedTheme={mockTheme}
          isSecretThemeUnlocked={hasUnlockedSecretTheme}
          onSelectTheme={setMockTheme}
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
            horseClicks={horseClicks}
            isHorseLaunching={isHorseLaunching}
            isSecretThemeUnlocked={hasUnlockedSecretTheme}
            onSecretHorseClick={handleSecretHorseClick}
            onSelectMessage={handleSelectMessage}
          />
          <ReaderPreview message={selectedMessage} />
        </div>
      </div>
    </section>
  )
}
