import { describe, expect, test } from 'bun:test'

import type { MessageDetailResult, MessageSummary } from '@/gen'
import { parseEmailPattern, type EmailPattern } from '@/domain/address'
import {
  accountFromOptions,
  deriveReplySeed,
  initialComposeForm,
  replyAllRecipients,
  replyContextFromDetail,
  toSendMessageRequest,
  EMPTY_FORM,
  type ComposeAccount,
} from './model'

function summary(overrides: Partial<MessageSummary> = {}): MessageSummary {
  return {
    id: 'm1',
    sourceId: 'acct-1',
    sourceName: 'Work',
    sourceThreadId: 't1',
    conversationId: 'c1',
    subject: 'Quarterly report',
    fromName: 'Ada Lovelace',
    fromEmail: 'ada@example.com',
    to: [{ name: 'Theo', email: 'theo@example.com' }],
    preview: null,
    receivedAt: '2026-07-01T10:00:00Z',
    hasAttachment: false,
    isRead: true,
    isFlagged: false,
    mailboxIds: ['inbox'],
    keywords: ['$seen'],
    rfcMessageId: '<orig@example.com>',
    inReplyTo: '<parent@example.com>',
    ...overrides,
  }
}

function detail(
  overrides: Partial<MessageDetailResult> = {},
): MessageDetailResult {
  return {
    summary: summary(),
    bodyHtml: null,
    bodyText: 'first line\nsecond line',
    attachments: [],
    ...overrides,
  }
}

describe('replyContextFromDetail', () => {
  test('derives reply recipients, subjects, and threading headers', () => {
    const ctx = replyContextFromDetail(detail())
    expect(ctx.to).toEqual([{ name: 'Ada Lovelace', email: 'ada@example.com' }])
    expect(ctx.originalTo).toEqual([{ name: 'Theo', email: 'theo@example.com' }])
    expect(ctx.replySubject).toBe('Re: Quarterly report')
    expect(ctx.forwardSubject).toBe('Fwd: Quarterly report')
    expect(ctx.inReplyTo).toBe('<orig@example.com>')
    expect(ctx.references).toBe('<parent@example.com> <orig@example.com>')
    expect(ctx.originalDate).toBe('2026-07-01T10:00:00Z')
  })

  test('quotes every body line and builds the forwarded block', () => {
    const ctx = replyContextFromDetail(detail())
    expect(ctx.quotedBody).toBe('> first line\n> second line')
    expect(ctx.forwardedBody).toContain(
      '---------- Forwarded message ----------',
    )
    expect(ctx.forwardedBody).toContain(
      'From: Ada Lovelace <ada@example.com>',
    )
    expect(ctx.forwardedBody).toContain('Subject: Quarterly report')
    expect(ctx.forwardedBody).toContain('first line\nsecond line')
  })

  test('does not double-prefix an already-prefixed subject', () => {
    const ctx = replyContextFromDetail(
      detail({ summary: summary({ subject: 're: hello' }) }),
    )
    expect(ctx.replySubject).toBe('re: hello')
  })

  test('degrades without a body or threading headers', () => {
    const ctx = replyContextFromDetail(
      detail({
        bodyText: null,
        summary: summary({
          rfcMessageId: undefined,
          inReplyTo: undefined,
        }),
      }),
    )
    expect(ctx.quotedBody).toBeNull()
    expect(ctx.forwardedBody).toBeNull()
    expect(ctx.inReplyTo).toBeNull()
    expect(ctx.references).toBeNull()
  })
})

describe('toSendMessageRequest', () => {
  test('pins the draft identity onto the assembled input', () => {
    const request = toSendMessageRequest(
      {
        from: { name: null, email: 'me@example.com' },
        to: [{ name: null, email: 'you@example.com' }],
        cc: [],
        bcc: [],
        subject: 'hi',
        body: 'text',
        inReplyTo: '<orig@example.com>',
        references: '<a> <b>',
        attachments: [],
      },
      'draft-key-1',
    )
    expect(request.draftId).toBe('draft-key-1')
    expect(request.inReplyTo).toBe('<orig@example.com>')
    expect(request.sendAt).toBeUndefined()
    expect(request.undoWindowSeconds).toBeUndefined()
  })
})

describe('accountFromOptions', () => {
  const accounts: ComposeAccount[] = [
    {
      id: 'acct-1',
      name: 'Work',
      fullName: 'Theo R',
      emailPatterns: ['theo@example.com', '*@corp.example.com'].map(
        (raw): EmailPattern => {
          const pattern = parseEmailPattern(raw)
          if (!pattern) throw new Error(`fixture pattern rejected: ${raw}`)
          return pattern
        },
      ),
    },
  ]

  test('offers configured concrete addresses and the identity first', () => {
    const options = accountFromOptions(
      accounts,
      { name: 'Theo R', email: 'theo@example.com' },
      'acct-1',
      [],
    )
    expect(options[0]).toMatchObject({
      origin: 'identity',
      email: 'theo@example.com',
    })
    // The identical configured address is de-duplicated behind the identity.
    expect(
      options.filter((option) => option.email === 'theo@example.com'),
    ).toHaveLength(1)
  })

  test('keeps cached senders only when they match the account patterns', () => {
    const options = accountFromOptions(accounts, null, 'acct-1', [
      { sourceId: 'acct-1', name: null, email: 'alias@corp.example.com' },
      { sourceId: 'acct-1', name: null, email: 'stranger@elsewhere.com' },
    ])
    const emails = options.map((option) => option.email)
    expect(emails).toContain('alias@corp.example.com')
    expect(emails).not.toContain('stranger@elsewhere.com')
  })
})

describe('replyAllRecipients', () => {
  test('spans From + To in `to`, keeps Cc, excluding self and duplicates', () => {
    const { to, cc } = replyAllRecipients(
      [{ name: 'Ada', email: 'ada@example.com' }],
      [
        { name: 'Theo', email: 'THEO@example.com' },
        { name: 'Grace', email: 'grace@example.com' },
        { name: 'Ada twice', email: 'Ada@Example.com' },
      ],
      [
        { name: 'Cc self', email: 'theo@example.com' },
        { name: 'Cc other', email: 'cc@example.com' },
      ],
      'theo@example.com',
    )
    expect(to.map((r) => r.email)).toEqual([
      'ada@example.com',
      'grace@example.com',
    ])
    expect(cc.map((r) => r.email)).toEqual(['cc@example.com'])
  })

  test('without a known self keeps every distinct recipient', () => {
    const { to } = replyAllRecipients(
      [{ name: null, email: 'ada@example.com' }],
      [{ name: null, email: 'theo@example.com' }],
      [],
      undefined,
    )
    expect(to.map((r) => r.email)).toEqual([
      'ada@example.com',
      'theo@example.com',
    ])
  })
})

describe('deriveReplySeed', () => {
  test('reply addresses the sender and heads the quote with the attribution', () => {
    const seed = deriveReplySeed('reply', replyContextFromDetail(detail()), 'theo@example.com')
    expect(seed.to).toEqual([{ name: 'Ada Lovelace', email: 'ada@example.com' }])
    expect(seed.cc).toEqual([])
    expect(seed.subject).toBe('Re: Quarterly report')
    expect(seed.quoteBlock).toContain('Ada Lovelace <ada@example.com> wrote:')
    expect(seed.quoteBlock).toContain('> first line\n> second line')
  })

  test('reply-all spans From + To minus self', () => {
    const seed = deriveReplySeed(
      'replyAll',
      replyContextFromDetail(detail()),
      'theo@example.com',
    )
    expect(seed.to.map((r) => r.email)).toEqual(['ada@example.com'])
  })

  test('forward starts unaddressed with the forwarded block', () => {
    const seed = deriveReplySeed('forward', replyContextFromDetail(detail()), undefined)
    expect(seed.to).toEqual([])
    expect(seed.subject).toBe('Fwd: Quarterly report')
    expect(seed.quoteBlock).toContain('---------- Forwarded message ----------')
  })

  test('a missing body yields no quote block for a reply', () => {
    const seed = deriveReplySeed(
      'reply',
      replyContextFromDetail(detail({ bodyText: null })),
      undefined,
    )
    expect(seed.quoteBlock).toBeNull()
  })
})

describe('initialComposeForm', () => {
  test('a resumed draft replaces the empty form once loaded', () => {
    const draftSeed = {
      from: 'me@example.com',
      to: 'you@example.com',
      cc: '',
      bcc: '',
      subject: 'Kept',
      body: 'draft text',
    }
    expect(
      initialComposeForm({ draftSeed, intentKind: 'draft', mailtoSeed: undefined }),
    ).toEqual({ ...draftSeed, attachments: [] })
    expect(
      initialComposeForm({
        draftSeed: undefined,
        intentKind: 'draft',
        mailtoSeed: undefined,
      }),
    ).toEqual(EMPTY_FORM)
  })

  test('a mailto seeds its known fields synchronously', () => {
    const form = initialComposeForm({
      draftSeed: undefined,
      intentKind: 'mailto',
      mailtoSeed: { to: 'a@b.c', subject: 'Hi', body: 'text' },
    })
    expect(form.to).toBe('a@b.c')
    expect(form.subject).toBe('Hi')
    expect(form.body).toBe('text')
  })

  test('a reply starts empty — its seed streams in later', () => {
    expect(
      initialComposeForm({
        draftSeed: undefined,
        intentKind: 'reply',
        mailtoSeed: undefined,
      }),
    ).toEqual(EMPTY_FORM)
  })
})
