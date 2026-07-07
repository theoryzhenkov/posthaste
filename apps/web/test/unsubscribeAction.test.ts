/**
 * `message.unsubscribe` — the List-Unsubscribe (RFC 2369/8058) action.
 *
 * Resolver-level coverage of the DOUBLE availability gate (parsed targets on
 * the detail DTO + the `services.unsubscribe` capability binding), the
 * context-resolved confirm (one-click ONLY — mailto/browser paths are
 * user-mediated), and the three execution paths' dispatch priority
 * (one-click POST > mailto composer > plain https in the system browser).
 * The `mailto:` URI → compose-seed parser is covered alongside.
 */
import { describe, expect, it, mock } from 'bun:test'

// Side-effect import: registers the message definitions into the registry.
import '../src/actions/defs/message'
import '../src/actions/defs/app'
import { runResolvedWithConfirm } from '../src/actions/keyboard'
import { resolveActions } from '../src/actions/resolve'
import type {
  ActionContext,
  ActionServices,
  MessageTarget,
} from '../src/actions/types'
import type { ListUnsubscribe, MessageDetail } from '../src/api/types'
import { parseMailtoUri } from '../src/composeIntent'
import { SYSTEM_KEYWORDS } from '../src/domainVocabulary'
import type { EmailActions } from '../src/hooks/useEmailActions'

const ONE_CLICK: ListUnsubscribe = {
  https: 'https://news.example.test/unsub/opaque',
  mailto: 'mailto:unsub@example.test?subject=stop',
  oneClick: true,
}

function detail(over: Partial<MessageDetail> = {}): MessageDetail {
  return {
    id: 'm1',
    sourceId: 's1',
    sourceName: 'Acct',
    sourceThreadId: 't1',
    conversationId: 'c1',
    subject: 'Weekly digest',
    fromName: 'Newsletter',
    fromEmail: 'news@example.test',
    to: [],
    preview: null,
    receivedAt: '2026-01-01T00:00:00Z',
    hasAttachment: false,
    isRead: true,
    isFlagged: false,
    mailboxIds: ['mb1'],
    keywords: [],
    draftId: null,
    bodyHtml: null,
    bodyText: null,
    rawMessage: null,
    attachments: [],
    ...over,
  }
}

function makeServices(bindUnsubscribe = true) {
  const unsubscribe = {
    oneClick: mock(() => {}),
    mailto: mock(() => {}),
    openLink: mock(() => {}),
  }
  const services: ActionServices = {
    email: { isPending: false } as unknown as EmailActions,
    ...(bindUnsubscribe ? { unsubscribe } : {}),
  }
  return { services, unsubscribe }
}

function ctx(
  summary: MessageDetail,
  over: Partial<ActionContext> = {},
): ActionContext {
  const target: MessageTarget = {
    ref: { sourceId: summary.sourceId, messageId: summary.id },
    summary,
    isDraft: summary.keywords.includes(SYSTEM_KEYWORDS.Draft),
    draftId: summary.draftId,
    conversationId: summary.conversationId,
  }
  return {
    targets: [target],
    viewRole: 'inbox',
    activePane: 'list',
    surface: 'detail-header',
    inputOwner: 'mail',
    hasPendingMutation: false,
    connection: 'unknown',
    ...over,
  }
}

function resolveUnsubscribe(
  summary: MessageDetail,
  over: Partial<ActionContext> = {},
  bindUnsubscribe = true,
) {
  const { services, unsubscribe } = makeServices(bindUnsubscribe)
  const resolved = resolveActions(ctx(summary, over), services).find(
    (action) => action.def.id === 'message.unsubscribe',
  )
  return { resolved, unsubscribe }
}

describe('message.unsubscribe availability gating', () => {
  it('is hidden when the message carries no unsubscribe data', () => {
    const { resolved } = resolveUnsubscribe(detail())
    expect(resolved).toBeUndefined()
  })

  it('is hidden without the services.unsubscribe binding (confirm-less hosts)', () => {
    const { resolved } = resolveUnsubscribe(
      detail({ listUnsubscribe: ONE_CLICK }),
      {},
      false,
    )
    expect(resolved).toBeUndefined()
  })

  it('is hidden on a draft', () => {
    const { resolved } = resolveUnsubscribe(
      detail({
        listUnsubscribe: ONE_CLICK,
        keywords: [SYSTEM_KEYWORDS.Draft],
      }),
    )
    expect(resolved).toBeUndefined()
  })

  it('resolves on the detail header when the data + binding are present', () => {
    const { resolved } = resolveUnsubscribe(
      detail({ listUnsubscribe: ONE_CLICK }),
    )
    expect(resolved?.title).toBe('Unsubscribe')
    expect(resolved?.enabled).toBe(true)
  })

  it('is data-gated on other surfaces too (context menu with a detail target)', () => {
    const withData = resolveUnsubscribe(
      detail({ listUnsubscribe: ONE_CLICK }),
      {
        surface: 'context-menu',
      },
    )
    expect(withData.resolved).toBeDefined()
    const withoutData = resolveUnsubscribe(detail(), {
      surface: 'context-menu',
    })
    expect(withoutData.resolved).toBeUndefined()
  })
})

describe('message.unsubscribe confirm gating (one-click only)', () => {
  it('carries a sender-naming confirm for a one-click target', () => {
    const { resolved } = resolveUnsubscribe(
      detail({ listUnsubscribe: ONE_CLICK }),
    )
    expect(resolved?.confirm?.title).toBe('Unsubscribe from Newsletter?')
    expect(resolved?.confirm?.confirmLabel).toBe('Unsubscribe')
  })

  it('never runs the one-click POST bare — it routes through the confirm host', () => {
    const { resolved, unsubscribe } = resolveUnsubscribe(
      detail({ listUnsubscribe: ONE_CLICK }),
    )
    let parked: (() => void) | null = null
    runResolvedWithConfirm(resolved!, (_confirm, onConfirm) => {
      parked = onConfirm
    })
    expect(unsubscribe.oneClick).not.toHaveBeenCalled()
    parked!()
    expect(unsubscribe.oneClick).toHaveBeenCalledWith({
      sourceId: 's1',
      messageId: 'm1',
    })
    expect(unsubscribe.mailto).not.toHaveBeenCalled()
    expect(unsubscribe.openLink).not.toHaveBeenCalled()
  })

  it('has no confirm for the user-mediated mailto path and runs it instantly', () => {
    const { resolved, unsubscribe } = resolveUnsubscribe(
      detail({
        listUnsubscribe: {
          mailto: 'mailto:unsub@example.test',
          oneClick: false,
        },
      }),
    )
    expect(resolved?.confirm).toBeUndefined()
    runResolvedWithConfirm(resolved!, () => {
      throw new Error('mailto must not confirm')
    })
    expect(unsubscribe.mailto).toHaveBeenCalledWith('mailto:unsub@example.test')
    expect(unsubscribe.oneClick).not.toHaveBeenCalled()
  })
})

describe('message.unsubscribe path priority', () => {
  it('prefers the one-click POST when marked one-click', () => {
    const { resolved, unsubscribe } = resolveUnsubscribe(
      detail({ listUnsubscribe: ONE_CLICK }),
    )
    void resolved!.execute()
    expect(unsubscribe.oneClick).toHaveBeenCalledTimes(1)
    expect(unsubscribe.mailto).not.toHaveBeenCalled()
    expect(unsubscribe.openLink).not.toHaveBeenCalled()
  })

  it('falls back to the composer for a mailto-only target', () => {
    const { resolved, unsubscribe } = resolveUnsubscribe(
      detail({
        listUnsubscribe: {
          mailto: 'mailto:unsub@example.test?subject=stop',
          oneClick: false,
        },
      }),
    )
    void resolved!.execute()
    expect(unsubscribe.mailto).toHaveBeenCalledWith(
      'mailto:unsub@example.test?subject=stop',
    )
    expect(unsubscribe.oneClick).not.toHaveBeenCalled()
    expect(unsubscribe.openLink).not.toHaveBeenCalled()
  })

  it('opens a plain (non-one-click) https target in the system browser', () => {
    const { resolved, unsubscribe } = resolveUnsubscribe(
      detail({
        listUnsubscribe: {
          https: 'https://news.example.test/unsub/landing',
          oneClick: false,
        },
      }),
    )
    void resolved!.execute()
    expect(unsubscribe.openLink).toHaveBeenCalledWith(
      'https://news.example.test/unsub/landing',
    )
    expect(unsubscribe.oneClick).not.toHaveBeenCalled()
    expect(unsubscribe.mailto).not.toHaveBeenCalled()
  })

  it('prefers mailto over the POST when one-click lacks an https target (defensive)', () => {
    const { resolved, unsubscribe } = resolveUnsubscribe(
      detail({
        listUnsubscribe: {
          mailto: 'mailto:unsub@example.test',
          oneClick: true,
        },
      }),
    )
    void resolved!.execute()
    expect(unsubscribe.mailto).toHaveBeenCalledTimes(1)
    expect(unsubscribe.oneClick).not.toHaveBeenCalled()
  })
})

describe('parseMailtoUri (composer prefill seed)', () => {
  it('parses address, subject, and body', () => {
    expect(
      parseMailtoUri(
        'mailto:unsub@example.test?subject=Unsubscribe%20me&body=please',
      ),
    ).toEqual({
      to: 'unsub@example.test',
      subject: 'Unsubscribe me',
      body: 'please',
    })
  })

  it('joins multiple addresses and appended to= params', () => {
    expect(
      parseMailtoUri('mailto:a@x.test,b@x.test?to=c@x.test&subject=stop'),
    ).toEqual({ to: 'a@x.test, b@x.test, c@x.test', subject: 'stop', body: '' })
  })

  it('is case-insensitive on the scheme and ignores unknown params', () => {
    expect(parseMailtoUri('MAILTO:unsub@example.test?x-tracker=1')).toEqual({
      to: 'unsub@example.test',
      subject: '',
      body: '',
    })
  })

  it('degrades a malformed percent-escape to its raw text', () => {
    expect(parseMailtoUri('mailto:unsub@example.test?subject=100%')).toEqual({
      to: 'unsub@example.test',
      subject: '100%',
      body: '',
    })
  })
})
