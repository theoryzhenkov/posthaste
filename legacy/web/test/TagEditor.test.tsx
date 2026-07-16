import { describe, expect, it, mock } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, within } from '@testing-library/react'
import type { ReactNode } from 'react'

import type { AppSettings, MessageDetail, TagSummary } from '../src/api/types'
import type { EmailActions } from '../src/hooks/useEmailActions'
import { TagEditor } from '../src/components/TagEditor'
import { queryKeys } from '../src/queryKeys'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

const message: MessageDetail = {
  id: 'message-1',
  sourceId: 'account-1',
  sourceName: 'Work',
  sourceThreadId: 'thread-1',
  conversationId: 'conversation-1',
  subject: 'Hello',
  fromName: 'Sender',
  fromEmail: 'sender@example.test',
  to: [],
  preview: 'Preview',
  receivedAt: '2026-05-31T00:00:00Z',
  hasAttachment: false,
  isRead: false,
  isFlagged: false,
  mailboxIds: ['mailbox-1'],
  keywords: ['work'],
  bodyHtml: null,
  bodyText: null,
  rawMessage: null,
  attachments: [],
}

const KNOWN_TAGS: TagSummary[] = [
  { name: 'work', unreadMessages: 0, totalMessages: 3 },
  { name: 'travel', unreadMessages: 2, totalMessages: 4 },
  { name: 'invoices', unreadMessages: 0, totalMessages: 1 },
  { name: 'family', unreadMessages: 0, totalMessages: 1 },
  { name: 'newsletter', unreadMessages: 5, totalMessages: 9 },
]

function makeActions(overrides: Partial<EmailActions> = {}): EmailActions {
  return {
    toggleRead: mock(() => {}),
    markRead: mock(() => {}),
    toggleFlag: mock(() => {}),
    setUserTags: mock(() => {}),
    archive: mock(() => {}),
    trash: mock(() => {}),
    discardDraft: mock(() => {}),
    moveToInbox: mock(() => {}),
    deletePermanently: mock(() => {}),
    clearError: mock(() => {}),
    errorMessage: null,
    isPending: false,
    ...overrides,
  }
}

function renderTagEditor(props: {
  actions?: EmailActions
  knownTags?: TagSummary[]
  onClose?: () => void
  onManageTags?: () => void
  tagAppearance?: AppSettings['tags']
}) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Infinity } },
  })
  queryClient.setQueryData<AppSettings>(queryKeys.settings, {
    defaultAccountId: null,
    cachePolicy: 'auto' as never,
    automationRules: [],
    automationDrafts: [],
    mailboxColors: [],
    tags: props.tagAppearance ?? [],
    smartMailboxOrder: [],
    accountOrder: [],
    mailboxGroups: [],
  })
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  )
  const actions = props.actions ?? makeActions()
  const onClose = props.onClose ?? mock(() => {})
  const onManageTags = props.onManageTags ?? mock(() => {})
  const view = render(
    <TagEditor
      actions={actions}
      knownTags={props.knownTags ?? KNOWN_TAGS}
      message={message}
      onClose={onClose}
      onManageTags={onManageTags}
    />,
    { wrapper },
  )
  return { view, actions, onClose, onManageTags, queryClient }
}

describe('TagEditor', () => {
  it('renders a capped, scrollable suggestions container', () => {
    const { view } = renderTagEditor({})
    const screen = within(view.container)
    const container = screen.getByTestId('tag-suggestions')
    expect(container.className).toContain('overflow-y-auto')
    expect(container.className).toContain('max-h-60')
    // Every known tag not already applied is offered inside the scroll area.
    expect(screen.getByText('travel')).toBeTruthy()
    expect(screen.getByText('newsletter')).toBeTruthy()
    // The already-applied tag is not offered again as a suggestion.
    expect(screen.queryByText('invoices')?.closest('button')).toBeTruthy()
  })

  it("renders a suggestion using the tag's resolved appearance color", () => {
    const { view } = renderTagEditor({
      // Hex, not oklch: happy-dom's CSS engine silently drops oklch() color
      // values when serializing style, which would make this assertion moot.
      tagAppearance: [{ name: 'travel', fg: '#111111', bg: '#eeeeee' }],
    })
    const screen = within(view.container)
    const travelChip = screen.getByText('travel')
    // TagChip renders the name in an inner <span>; the resolved color lives on
    // the outer chip <span> that wraps it — the exact node TagChip colors for
    // the current-tags row too, so this is the same color-resolution path.
    expect(travelChip.parentElement?.getAttribute('style')).toContain('#111111')
  })

  it('calls onManageTags (closing + navigating to settings) when the manage button is clicked', () => {
    const { view, onManageTags } = renderTagEditor({})
    const screen = within(view.container)
    fireEvent.click(screen.getByText('Manage tags…'))
    expect(onManageTags).toHaveBeenCalledTimes(1)
  })

  it('still supports adding a suggested tag and removing an existing tag', () => {
    const { view, actions } = renderTagEditor({})
    const screen = within(view.container)

    // Add: clicking a suggestion row applies that tag alongside the existing one.
    fireEvent.click(screen.getByText('travel'))
    expect(actions.setUserTags).toHaveBeenCalledWith(
      expect.objectContaining({ messageId: 'message-1' }),
      ['work', 'travel'],
    )

    // Remove: the existing tag's chip has a remove control.
    fireEvent.click(screen.getByLabelText('Remove work'))
    expect(actions.setUserTags).toHaveBeenCalledWith(
      expect.objectContaining({ messageId: 'message-1' }),
      [],
    )
  })

  it('shows an empty state when there are no more tags to suggest', () => {
    const { view } = renderTagEditor({
      knownTags: [{ name: 'work', unreadMessages: 0, totalMessages: 1 }],
    })
    const screen = within(view.container)
    expect(screen.getByText('No more tags to suggest')).toBeTruthy()
  })
})
