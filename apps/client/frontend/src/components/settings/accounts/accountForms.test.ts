import { describe, expect, test } from 'bun:test'

import type { AccountSettingsResult } from '@/gen'
import type { AccountFormState } from '../panel/types'
import type { AccountEditorConnectionModel } from './editor/accountEditorModel'
import {
  buildIdentityPatch,
  buildSecretChange,
  buildTransportIntent,
  hasUnsavedAccountChanges,
} from './accountForms'

/** A saved manual IMAP/SMTP account baseline. */
const savedImap: AccountFormState = {
  name: 'Work',
  fullName: 'Ada Lovelace',
  signature: 'Sent from Posthaste',
  emailPatternsText: 'ada@example.com',
  appearanceInitials: 'W',
  appearanceColorHue: 200,
  driver: 'imapSmtp',
  baseUrl: '',
  username: 'ada@example.com',
  password: '',
  imapHost: 'imap.example.com',
  imapPort: '993',
  imapSecurity: 'tls',
  smtpHost: 'smtp.example.com',
  smtpPort: '465',
  smtpSecurity: 'tls',
}

/** A saved manual JMAP account baseline. */
const savedJmap: AccountFormState = {
  ...savedImap,
  driver: 'jmap',
  baseUrl: 'https://mail.example.com/jmap',
}

const manual: AccountEditorConnectionModel = {
  kind: 'manualCredentials',
  account: null,
}

/** Minimal OAuth account answer — the gate only reads `connection.kind`. */
const oauthAccount: AccountSettingsResult = {
  id: 'a1',
  name: 'Work',
  fullName: null,
  signature: null,
  emailPatterns: ['ada@example.com'],
  driver: 'imapSmtp',
  enabled: true,
  transport: {
    provider: 'gmail',
    auth: 'oauth2',
    baseUrl: null,
    username: 'ada@example.com',
    imap: null,
    smtp: null,
    secret: { storage: 'os', configured: true, label: null },
  },
  createdAt: '2026-01-01T00:00:00Z',
  updatedAt: '2026-01-01T00:00:00Z',
}
const oauth: AccountEditorConnectionModel = {
  kind: 'managedOAuth',
  account: oauthAccount,
}

describe('buildIdentityPatch clear-vs-keep per field', () => {
  test('untouched fields send keep', () => {
    const patch = buildIdentityPatch({ ...savedImap }, savedImap, 'a1')
    expect(patch.fullName).toEqual({ kind: 'keep' })
    expect(patch.signature).toEqual({ kind: 'keep' })
    // Non-patch fields ride along as today.
    expect(patch.name).toBe('Work')
    expect(patch.emailPatterns).toEqual(['ada@example.com'])
  })

  test('an emptied field clears; the sibling stays keep', () => {
    const patch = buildIdentityPatch(
      { ...savedImap, fullName: '  ' },
      savedImap,
      'a1',
    )
    expect(patch.fullName).toEqual({ kind: 'clear' })
    expect(patch.signature).toEqual({ kind: 'keep' })
  })

  test('an edited field sets the trimmed text', () => {
    const patch = buildIdentityPatch(
      { ...savedImap, signature: ' Cheers, Ada ' },
      savedImap,
      'a1',
    )
    expect(patch.signature).toEqual({ kind: 'set', value: 'Cheers, Ada' })
    expect(patch.fullName).toEqual({ kind: 'keep' })
  })
})

describe('buildTransportIntent clear-vs-keep per field', () => {
  test('jmap: untouched baseUrl and username send keep', () => {
    const intent = buildTransportIntent({ ...savedJmap }, savedJmap, 'a1')
    expect(intent.baseUrl).toEqual({ kind: 'keep' })
    expect(intent.username).toEqual({ kind: 'keep' })
    expect(intent.provider).toBe('generic')
    expect(intent.imap).toBeUndefined()
  })

  test('jmap: an emptied baseUrl clears, an edited username sets', () => {
    const intent = buildTransportIntent(
      { ...savedJmap, baseUrl: '', username: ' ada2@example.com ' },
      savedJmap,
      'a1',
    )
    expect(intent.baseUrl).toEqual({ kind: 'clear' })
    expect(intent.username).toEqual({ kind: 'set', value: 'ada2@example.com' })
  })

  test('imapSmtp: baseUrl always clears (it belongs to the JMAP driver), untouched username keeps', () => {
    const intent = buildTransportIntent({ ...savedImap }, savedImap, 'a1')
    expect(intent.baseUrl).toEqual({ kind: 'clear' })
    expect(intent.username).toEqual({ kind: 'keep' })
    expect(intent.imap).toEqual({
      host: 'imap.example.com',
      port: 993,
      security: 'tls',
    })
    expect(intent.smtp).toEqual({
      host: 'smtp.example.com',
      port: 465,
      security: 'tls',
    })
  })

  test('imapSmtp: an unparsable port falls back to the protocol default', () => {
    const intent = buildTransportIntent(
      { ...savedImap, imapPort: 'nope', smtpPort: '70000' },
      savedImap,
      'a1',
    )
    expect(intent.imap?.port).toBe(993)
    expect(intent.smtp?.port).toBe(465)
  })
})

describe('buildSecretChange', () => {
  test('an untouched (or whitespace) password keeps the stored secret', () => {
    expect(buildSecretChange(savedImap)).toEqual({ kind: 'keep' })
    expect(buildSecretChange({ ...savedImap, password: '   ' })).toEqual({
      kind: 'keep',
    })
  })

  test('a typed password replaces, preserving it verbatim', () => {
    expect(buildSecretChange({ ...savedImap, password: ' hunter2 ' })).toEqual({
      kind: 'replace',
      secret: ' hunter2 ',
    })
  })
})

describe('hasUnsavedAccountChanges', () => {
  test('clean form is not dirty', () => {
    expect(hasUnsavedAccountChanges({ ...savedImap }, savedImap, manual)).toBe(
      false,
    )
  })

  test('identity edits count for every connection kind', () => {
    const form = { ...savedImap, name: 'Personal' }
    expect(hasUnsavedAccountChanges(form, savedImap, manual)).toBe(true)
    expect(hasUnsavedAccountChanges(form, savedImap, oauth)).toBe(true)
  })

  test('transport and secret edits count only for manual credentials', () => {
    const hostEdit = { ...savedImap, imapHost: 'imap.other.example' }
    expect(hasUnsavedAccountChanges(hostEdit, savedImap, manual)).toBe(true)
    expect(hasUnsavedAccountChanges(hostEdit, savedImap, oauth)).toBe(false)

    const passwordEdit = { ...savedImap, password: 'hunter2' }
    expect(hasUnsavedAccountChanges(passwordEdit, savedImap, manual)).toBe(true)
    expect(hasUnsavedAccountChanges(passwordEdit, savedImap, oauth)).toBe(false)
  })

  test('appearance edits are excluded — they autosave separately', () => {
    const form = { ...savedImap, appearanceColorHue: 12, appearanceInitials: 'Z' }
    expect(hasUnsavedAccountChanges(form, savedImap, manual)).toBe(false)
  })
})
