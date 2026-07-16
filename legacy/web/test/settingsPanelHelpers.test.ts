import { describe, expect, it } from 'bun:test'

import type { AccountOverview } from '../src/api/types'
import type { ExistingAccountEditorModel } from '../src/components/settings-panel/accountEditorModel'
import {
  EMPTY_FORM,
  applyImapDefaults,
  buildAccountAppearanceInput,
  buildCreateAccountPayload,
  buildSecretInput,
  buildUpdateAccountPayload,
  imapDefaultsForEmail,
  normalizeAccountInitials,
  parseEmailPatterns,
  syncProgressLabel,
} from '../src/components/settings-panel/helpers'

function accountOverview(
  overrides: Partial<AccountOverview> = {},
): AccountOverview {
  return {
    id: 'primary',
    name: 'Primary',
    fullName: null,
    emailPatterns: ['primary@example.com'],
    driver: 'imapSmtp',
    enabled: true,
    appearance: {
      kind: 'initials',
      initials: 'P',
      colorHue: 210,
    },
    connection: {
      kind: 'manualCredentials',
      provider: 'generic',
      providerKind: 'generic',
      auth: 'password',
      baseUrl: null,
      username: 'primary@example.com',
      imap: null,
      smtp: null,
      secret: {
        storage: 'os',
        configured: true,
        label: null,
      },
    },
    createdAt: '2026-05-24T00:00:00Z',
    updatedAt: '2026-05-24T00:00:00Z',
    isDefault: false,
    runtime: {
      status: 'syncing',
      push: 'connected',
      lastSyncAt: null,
      lastSyncError: null,
      lastSyncErrorCode: null,
      syncProgress: {
        syncId: 'sync-1',
        trigger: 'poll',
        startedAt: '2026-05-24T00:01:00Z',
        stage: 'fetching',
        detail: 'Syncing messages',
        mailboxName: 'Inbox',
        mailboxIndex: 1,
        mailboxCount: 2,
        messageCount: 10,
        totalCount: null,
      },
    },
    ...overrides,
  }
}

describe('settings panel helper contracts', () => {
  // spec: docs/L1-api#account-crud-lifecycle
  it('displays sync progress whenever progress is present, regardless of status', () => {
    expect(syncProgressLabel(accountOverview())).toBe(
      'Syncing messages · Inbox · 1/2 · 10 messages',
    )

    // Progress is shown even when status has moved past syncing, as long as a
    // progress object is present; it is only hidden when there is no progress.
    const ready = accountOverview({
      runtime: {
        status: 'ready',
        push: 'connected',
        lastSyncAt: null,
        lastSyncError: null,
        lastSyncErrorCode: null,
        syncProgress: null,
      },
    })
    expect(syncProgressLabel(ready)).toBeNull()
  })
})

describe('account form helpers', () => {
  // spec: docs/L1-api#account-crud-lifecycle
  it('normalizes initials to a single uppercase glyph, defaulting to A', () => {
    expect(normalizeAccountInitials('  ada lovelace ')).toBe('A')
    expect(normalizeAccountInitials('xavier')).toBe('X')
    expect(normalizeAccountInitials('')).toBe('A')
    expect(normalizeAccountInitials('   ')).toBe('A')
  })

  it('parses email patterns split on newlines/commas, trimming blanks', () => {
    expect(parseEmailPatterns('a@x.com, b@y.com\n c@z.com ,, \n')).toEqual([
      'a@x.com',
      'b@y.com',
      'c@z.com',
    ])
    expect(parseEmailPatterns('   ')).toEqual([])
  })

  it('builds a secret input that replaces only when a password is entered', () => {
    expect(buildSecretInput({ ...EMPTY_FORM, password: 'hunter2' })).toEqual({
      mode: 'replace',
      password: 'hunter2',
    })
    expect(buildSecretInput({ ...EMPTY_FORM, password: '   ' })).toEqual({
      mode: 'keep',
    })
  })

  it('derives appearance initials from explicit value or name and clamps hue', () => {
    expect(
      buildAccountAppearanceInput({
        ...EMPTY_FORM,
        name: 'Work',
        appearanceInitials: '',
        appearanceColorHue: 512.6,
      }),
    ).toEqual({ kind: 'initials', initials: 'W', colorHue: 360 })

    expect(
      buildAccountAppearanceInput({
        ...EMPTY_FORM,
        appearanceInitials: 'zed',
        appearanceColorHue: -10,
      }),
    ).toEqual({ kind: 'initials', initials: 'Z', colorHue: 0 })
  })

  it('builds a create payload that trims fields and nulls an empty full name', () => {
    const payload = buildCreateAccountPayload({
      ...EMPTY_FORM,
      name: '  Personal  ',
      fullName: '   ',
      emailPatternsText: 'me@x.com, *@x.com',
      baseUrl: 'https://x.com',
      username: 'me@x.com',
      password: 'secret',
    })
    expect(payload.name).toBe('Personal')
    expect(payload.fullName).toBeNull()
    expect(payload.driver).toBe('jmap')
    expect(payload.enabled).toBe(true)
    expect(payload.emailPatterns).toEqual(['me@x.com', '*@x.com'])
    expect(payload.secret).toEqual({ mode: 'replace', password: 'secret' })
  })

  it('builds a complete IMAP/SMTP transport when the driver is imapSmtp', () => {
    const payload = buildCreateAccountPayload({
      ...EMPTY_FORM,
      name: 'Fastmail',
      driver: 'imapSmtp',
      emailPatternsText: 'me@fastmail.com',
      username: 'me@fastmail.com',
      password: 'app-pw',
      imapHost: 'imap.fastmail.com',
      imapPort: '993',
      imapSecurity: 'tls',
      smtpHost: 'smtp.fastmail.com',
      smtpPort: '465',
      smtpSecurity: 'tls',
    })
    expect(payload.driver).toBe('imapSmtp')
    expect(payload.transport).toEqual({
      provider: 'generic',
      auth: 'appPassword',
      baseUrl: '',
      username: 'me@fastmail.com',
      imap: { host: 'imap.fastmail.com', port: 993, security: 'tls' },
      smtp: { host: 'smtp.fastmail.com', port: 465, security: 'tls' },
    })
    expect(payload.secret).toEqual({ mode: 'replace', password: 'app-pw' })
  })

  it('falls back to default ports when the IMAP/SMTP port fields are invalid', () => {
    const payload = buildCreateAccountPayload({
      ...EMPTY_FORM,
      driver: 'imapSmtp',
      username: 'me@example.com',
      emailPatternsText: 'me@example.com',
      imapHost: 'imap.example.com',
      imapPort: 'not-a-number',
      smtpHost: 'smtp.example.com',
      smtpPort: '',
    })
    expect(payload.transport.imap?.port).toBe(993)
    expect(payload.transport.smtp?.port).toBe(465)
  })

  it('infers IMAP/SMTP defaults from a known provider email domain', () => {
    const fastmail = imapDefaultsForEmail('user@fastmail.com')
    expect(fastmail?.imap).toEqual({
      host: 'imap.fastmail.com',
      port: 993,
      security: 'tls',
    })
    expect(fastmail?.smtp).toEqual({
      host: 'smtp.fastmail.com',
      port: 465,
      security: 'tls',
    })
    expect(fastmail?.auth).toBe('appPassword')
    expect(fastmail?.appPasswordHint).toContain('Fastmail')

    const icloud = imapDefaultsForEmail('user@icloud.com')
    expect(icloud?.provider).toBe('icloud')
    expect(icloud?.smtp).toEqual({
      host: 'smtp.mail.me.com',
      port: 587,
      security: 'startTls',
    })
  })

  it('guesses generic imap./smtp. hosts for an unknown domain', () => {
    const defaults = imapDefaultsForEmail('person@acme.example')
    expect(defaults?.imap.host).toBe('imap.acme.example')
    expect(defaults?.smtp.host).toBe('smtp.acme.example')
    expect(defaults?.auth).toBe('password')
    expect(imapDefaultsForEmail('')).toBeNull()
  })

  it('prefills empty endpoint fields from defaults but keeps user edits', () => {
    const prefilled = applyImapDefaults({
      ...EMPTY_FORM,
      driver: 'imapSmtp',
      username: 'me@fastmail.com',
      imapHost: '',
      smtpHost: '',
    })
    expect(prefilled.imapHost).toBe('imap.fastmail.com')
    expect(prefilled.smtpHost).toBe('smtp.fastmail.com')

    const edited = applyImapDefaults({
      ...EMPTY_FORM,
      driver: 'imapSmtp',
      username: 'me@fastmail.com',
      imapHost: 'mail.custom.test',
      smtpHost: 'send.custom.test',
    })
    expect(edited.imapHost).toBe('mail.custom.test')
    expect(edited.smtpHost).toBe('send.custom.test')
  })

  it('omits transport/secret on managed-OAuth updates but includes them for manual credentials', () => {
    const form = {
      ...EMPTY_FORM,
      name: 'Mail',
      baseUrl: 'https://x.com',
      username: 'me@x.com',
      password: 'pw',
    }
    const managed = {
      connection: { kind: 'managedOAuth' },
    } as unknown as ExistingAccountEditorModel
    const manual = {
      connection: { kind: 'manualCredentials' },
    } as unknown as ExistingAccountEditorModel

    const managedPayload = buildUpdateAccountPayload(form, managed)
    expect('transport' in managedPayload).toBe(false)
    expect('secret' in managedPayload).toBe(false)

    const manualPayload = buildUpdateAccountPayload(form, manual)
    expect(manualPayload.transport).toEqual({
      baseUrl: 'https://x.com',
      username: 'me@x.com',
    })
    expect(manualPayload.secret).toEqual({ mode: 'replace', password: 'pw' })
  })
})
