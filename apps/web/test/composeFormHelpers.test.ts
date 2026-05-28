import { describe, expect, it } from 'bun:test'

import type { AccountOverview, CachedSenderAddress } from '../src/api/types'
import {
  EMPTY_FORM,
  accountFromOptions,
  buildSendInput,
  formatRecipient,
  formatRecipients,
  isConcreteEmailPattern,
  optionLabel,
  parseRecipients,
  parseSender,
  wildcardMatchesEmail,
} from '../src/components/composeFormHelpers'

describe('compose form helpers', () => {
  // spec: docs/L1-compose#mime-structure
  it('parses recipients with and without display names, splitting on , and ;', () => {
    expect(parseRecipients('a@x.com, b@y.com; c@z.com')).toEqual([
      { name: null, email: 'a@x.com' },
      { name: null, email: 'b@y.com' },
      { name: null, email: 'c@z.com' },
    ])
  })

  it('extracts and unquotes display names, dropping empty fragments', () => {
    expect(
      parseRecipients('Ada <a@x.com>,  , "Grace Hopper" <g@y.com>'),
    ).toEqual([
      { name: 'Ada', email: 'a@x.com' },
      { name: 'Grace Hopper', email: 'g@y.com' },
    ])
    // angle brackets with no name -> null name
    expect(parseRecipients('<only@x.com>')).toEqual([
      { name: null, email: 'only@x.com' },
    ])
  })

  it('parseSender returns the first recipient or null', () => {
    expect(parseSender('Ada <a@x.com>, b@y.com')).toEqual({
      name: 'Ada',
      email: 'a@x.com',
    })
    expect(parseSender('   ')).toBeNull()
  })

  it('formats recipients, preferring a name when present', () => {
    expect(formatRecipient({ name: 'Ada', email: 'a@x.com' })).toBe(
      'Ada <a@x.com>',
    )
    expect(formatRecipient({ name: null, email: 'a@x.com' })).toBe('a@x.com')
    expect(
      formatRecipients([
        { name: 'Ada', email: 'a@x.com' },
        { name: null, email: 'b@y.com' },
      ]),
    ).toBe('Ada <a@x.com>, b@y.com')
  })

  it('builds a send input, trimming the subject and parsing all address fields', () => {
    const input = buildSendInput({
      ...EMPTY_FORM,
      from: 'Me <me@x.com>',
      to: 'a@x.com, b@y.com',
      cc: 'c@z.com',
      bcc: '',
      subject: '  Hello  ',
      body: 'Body text',
    })
    expect(input.from).toEqual({ name: 'Me', email: 'me@x.com' })
    expect(input.to).toHaveLength(2)
    expect(input.cc).toEqual([{ name: null, email: 'c@z.com' }])
    expect(input.bcc).toEqual([])
    expect(input.subject).toBe('Hello')
    expect(input.inReplyTo).toBeNull()
  })

  it('recognizes concrete vs wildcard/invalid email patterns', () => {
    expect(isConcreteEmailPattern('me@x.com')).toBe(true)
    expect(isConcreteEmailPattern('  me@x.com  ')).toBe(true)
    expect(isConcreteEmailPattern('*@x.com')).toBe(false)
    expect(isConcreteEmailPattern('notanemail')).toBe(false)
    expect(isConcreteEmailPattern('')).toBe(false)
  })

  it('matches wildcard domain patterns case-insensitively, ignoring concrete ones', () => {
    expect(wildcardMatchesEmail('*@x.com', 'ME@X.COM')).toBe(true)
    expect(wildcardMatchesEmail('*@x.com', 'me@other.com')).toBe(false)
    expect(wildcardMatchesEmail('me@x.com', 'me@x.com')).toBe(false)
  })

  it('optionLabel prefers the name when present', () => {
    expect(
      optionLabel({
        sourceId: 's',
        sourceName: 'S',
        name: 'Ada',
        email: 'a@x.com',
        origin: 'configured',
      }),
    ).toBe('Ada <a@x.com>')
    expect(
      optionLabel({
        sourceId: 's',
        sourceName: 'S',
        name: null,
        email: 'a@x.com',
        origin: 'cached',
      }),
    ).toBe('a@x.com')
  })

  it('derives from-address options: identity first, concrete patterns, cached, deduped', () => {
    const accounts = [
      {
        id: 'acct-1',
        name: 'Work',
        fullName: 'Ada Work',
        emailPatterns: ['ada@work.com', '*@work.com'],
      },
    ] as unknown as AccountOverview[]
    const cached = [
      { sourceId: 'acct-1', name: 'Cached', email: 'ada@work.com' },
      { sourceId: 'unknown', name: 'Ghost', email: 'x@y.com' },
    ] as unknown as CachedSenderAddress[]

    const options = accountFromOptions(
      accounts,
      { name: 'Ada', email: 'ada@work.com' },
      'acct-1',
      cached,
    )

    // identity is unshifted to the front
    expect(options[0]?.origin).toBe('identity')
    // wildcard pattern '*@work.com' is not a concrete option
    expect(options.some((o) => o.email === '*@work.com')).toBe(false)
    // cached entry for an unknown account is skipped
    expect(options.some((o) => o.email === 'x@y.com')).toBe(false)
    // dedup by sourceId:email — identity + configured + cached all 'ada@work.com'
    // on acct-1 collapse to a single entry
    const adaOnAcct1 = options.filter(
      (o) => o.sourceId === 'acct-1' && o.email === 'ada@work.com',
    )
    expect(adaOnAcct1).toHaveLength(1)
  })
})
