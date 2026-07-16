/**
 * PLAN-L2 Slice 3 — the registry-backed palette provider.
 *
 * Proves `createActionProvider` (which replaces the hand-rolled `commands` +
 * `tagActions` providers) maps the resolved palette surface to palette entries
 * with the enrichments the registry unlocks: contextual availability,
 * disabled-with-reason, shortcut hints, and the folded-in "Tag" command.
 */
import { describe, expect, it, mock } from 'bun:test'

import { createActionProvider } from '../src/command-search/providers/actions'
import type { ActionContext, ActionServices } from '../src/actions'
import type { MessageSummary } from '../src/api/types'
import { createRankingContext } from '../src/components/command-palette/model'
import type { ProviderSearchRequest } from '../src/command-search/types'
import { SYSTEM_KEYWORDS } from '../src/domainVocabulary'
import type { EmailActions } from '../src/hooks/useEmailActions'
import type { useMailClientHandlers } from '../src/app/useMailClientHandlers'

function makeMessage(over: Partial<MessageSummary> = {}): MessageSummary {
  return {
    id: 'm1',
    sourceId: 's1',
    sourceName: 'Acct',
    sourceThreadId: 't1',
    conversationId: 'c1',
    subject: 'Hi',
    fromName: 'A',
    fromEmail: 'a@x.test',
    to: [],
    preview: null,
    receivedAt: '2026-01-01T00:00:00Z',
    hasAttachment: false,
    isRead: false,
    isFlagged: false,
    mailboxIds: ['mb1'],
    keywords: [],
    draftId: null,
    ...over,
  }
}

function makeServices() {
  const email = {
    archive: mock(() => {}),
    trash: mock(() => {}),
    toggleFlag: mock(() => {}),
    toggleRead: mock(() => {}),
    moveToInbox: mock(() => {}),
    deletePermanently: mock(() => {}),
    discardDraft: mock(() => {}),
    isPending: false,
  }
  const app = {
    handleReply: mock(() => {}),
    handleCompose: mock(() => {}),
    handleOpenTagEditor: mock(() => {}),
    handleOpenSettings: mock(() => {}),
    handleShowShortcuts: mock(() => {}),
    handlePlaceholderAction: mock(() => {}),
  }
  const services: ActionServices = {
    email: email as unknown as EmailActions,
    app: app as unknown as ReturnType<typeof useMailClientHandlers>,
  }
  return { services, email, app }
}

function ctx(over: Partial<ActionContext> = {}): ActionContext {
  const summary = makeMessage()
  return {
    targets: [
      {
        ref: { sourceId: summary.sourceId, messageId: summary.id },
        summary,
        isDraft: summary.keywords.includes(SYSTEM_KEYWORDS.Draft),
        draftId: summary.draftId,
        conversationId: summary.conversationId,
      },
    ],
    viewRole: 'inbox',
    activePane: 'list',
    surface: 'palette',
    inputOwner: 'overlay',
    hasPendingMutation: false,
    connection: 'unknown',
    ...over,
  }
}

function request(query: string): ProviderSearchRequest {
  return {
    query,
    limit: 50,
    context: createRankingContext({ hasSelectedMessage: true }),
  }
}

async function labelsFor(query: string, context: ActionContext) {
  const { services } = makeServices()
  const provider = createActionProvider({
    getContext: () => context,
    getServices: () => services,
  })
  const page = await provider.search(request(query))
  return page.candidates.map((c) => c.entry)
}

describe('createActionProvider', () => {
  it('keeps the commands provider id + vertical so ranker wiring is unchanged', () => {
    const { services } = makeServices()
    const provider = createActionProvider({
      getContext: () => ctx(),
      getServices: () => services,
    })
    expect(provider.id).toBe('commands')
    expect(provider.vertical).toBe('command')
  })

  it('surfaces message + app palette commands for a selected inbox message', async () => {
    const entries = await labelsFor('', ctx())
    const ids = entries.map((e) => e.id)
    expect(ids).toContain('message.archive')
    expect(ids).toContain('message.reply')
    expect(ids).toContain('message.tag') // folded-in Tag command
    expect(ids).toContain('message.snooze')
    expect(ids).toContain('app.compose')
    expect(ids).toContain('app.open-settings')
    // Coverage-gap fixes: the open/show-conversation entries reach the palette
    // through the app-handler fallback.
    expect(ids).toContain('message.open')
    expect(ids).toContain('message.view-conversation')
    // Non-applicable actions stay out (delete-permanently is trash-view only).
    expect(ids).not.toContain('message.delete-permanently')
  })

  it('renders selection-scoped actions disabled-with-reason when nothing is selected', async () => {
    const entries = await labelsFor('', ctx({ targets: [] }))
    const archive = entries.find((e) => e.id === 'message.archive')
    expect(archive?.disabled).toBe(true)
    expect(archive?.disabledReason).toBe('Select a message first')
    // Ungated app commands stay enabled.
    const compose = entries.find((e) => e.id === 'app.compose')
    expect(compose?.disabled).toBe(false)
  })

  it('offers contextual commands per view role (trash → delete permanently)', async () => {
    const entries = await labelsFor('', ctx({ viewRole: 'trash' }))
    const ids = entries.map((e) => e.id)
    expect(ids).toContain('message.delete-permanently')
    expect(ids).toContain('message.move-to-inbox')
    expect(ids).not.toContain('message.archive') // hidden in trash
  })

  it('carries shortcut hints on rows', async () => {
    const entries = await labelsFor('', ctx())
    expect(entries.find((e) => e.id === 'message.archive')?.shortcut).toBe('E')
    expect(entries.find((e) => e.id === 'message.toggle-flag')?.shortcut).toBe(
      '⌘⇧L',
    )
    expect(entries.find((e) => e.id === 'app.compose')?.shortcut).toBe('⌘N')
  })

  it('filters entries by the typed query', async () => {
    const entries = await labelsFor('archive', ctx())
    const ids = entries.map((e) => e.id)
    expect(ids).toContain('message.archive')
    expect(ids).not.toContain('app.compose')
  })
})
