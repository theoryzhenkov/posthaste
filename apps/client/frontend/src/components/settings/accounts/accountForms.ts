import type {
  AccountAppearance,
  AccountSecretChange,
  AccountSettingsResult,
  CreateAccountIntent,
  FieldPatch,
  ProviderAuthKind,
  ProviderHint,
  TransportSecurity,
  UpdateAccountIntent,
  UpdateAccountTransportIntent,
} from '@/gen'
import type { ExistingAccountEditorModel } from './editor/accountEditorModel'
import type { AccountFormState } from '../panel/types'

/** host/port/security for one endpoint (the shared IMAP/SMTP wire shape). */
export interface EndpointSettings {
  host: string
  port: number
  security: TransportSecurity
}

/** Default empty form state for creating a new account. */
export const EMPTY_FORM: AccountFormState = {
  name: '',
  fullName: '',
  signature: '',
  emailPatternsText: '',
  appearanceInitials: 'A',
  appearanceColorHue: 0,
  driver: 'jmap',
  baseUrl: '',
  username: '',
  password: '',
  imapHost: '',
  imapPort: '993',
  imapSecurity: 'tls',
  smtpHost: '',
  smtpPort: '465',
  smtpSecurity: 'tls',
}

export function emptyAccountForm(): AccountFormState {
  return {
    ...EMPTY_FORM,
    driver: 'imapSmtp',
    appearanceColorHue: Math.floor(Math.random() * 361),
  }
}

/** Convert an account's settings answer into editable form state. */
export function formFromAccount(account: AccountSettingsResult): AccountFormState {
  const { imap, smtp } = account.transport
  const driver = account.driver === 'imapSmtp' ? 'imapSmtp' : 'jmap'
  const appearance = account.appearance ?? null
  return {
    name: account.name,
    fullName: account.fullName ?? '',
    signature: account.signature ?? '',
    emailPatternsText: account.emailPatterns?.join('\n') ?? '',
    appearanceInitials: normalizeAccountInitials(
      appearance?.initials ?? account.name,
    ),
    appearanceColorHue: appearance?.colorHue ?? 0,
    driver,
    baseUrl: account.transport.baseUrl ?? '',
    username: account.transport.username ?? '',
    password: '',
    imapHost: imap?.host ?? EMPTY_FORM.imapHost,
    imapPort: imap ? String(imap.port) : EMPTY_FORM.imapPort,
    imapSecurity: imap?.security ?? EMPTY_FORM.imapSecurity,
    smtpHost: smtp?.host ?? EMPTY_FORM.smtpHost,
    smtpPort: smtp ? String(smtp.port) : EMPTY_FORM.smtpPort,
    smtpSecurity: smtp?.security ?? EMPTY_FORM.smtpSecurity,
  }
}

/**
 * The secret write for the current form state: replace when a new password
 * was typed, keep otherwise. The material only ever travels inside the
 * dedicated `setAccountSecret` command.
 */
export function buildSecretChange(form: AccountFormState): AccountSecretChange {
  if (form.password.trim() !== '') {
    return { kind: 'replace', secret: form.password }
  }
  return { kind: 'keep' }
}

/** The patch for a clearable text field: an emptied input clears the stored
 * value; anything else sets the trimmed text. */
export function textFieldPatch(value: string): FieldPatch<string> {
  const trimmed = value.trim()
  return trimmed === '' ? { kind: 'clear' } : { kind: 'set', value: trimmed }
}

/** Parse newline/comma-separated addresses and catch-all patterns. */
export function parseEmailPatterns(value: string): string[] {
  return value
    .split(/[\n,]/)
    .map((pattern) => pattern.trim())
    .filter((pattern) => pattern.length > 0)
}

/** Identity payload for the `createAccount` command. Transport and secret
 * follow as their own commands once the backend has minted the id. */
export function buildCreateAccountIntent(
  form: AccountFormState,
): CreateAccountIntent {
  return {
    name: form.name.trim(),
    fullName: form.fullName.trim() || null,
    signature: form.signature.trim() || null,
    emailPatterns: parseEmailPatterns(form.emailPatternsText),
    enabled: true,
  }
}

/**
 * Transport patch for the chosen driver. JMAP carries a base URL; IMAP/SMTP
 * carries incoming (IMAP) + outgoing (SMTP) endpoints plus a provider/auth
 * hint derived from the account's primary email domain so the backend can
 * request an app-password when the provider needs one.
 */
export function buildTransportIntent(
  form: AccountFormState,
  accountId: string,
): UpdateAccountTransportIntent {
  if (form.driver !== 'imapSmtp') {
    return {
      accountId,
      provider: 'generic',
      auth: 'password',
      baseUrl: textFieldPatch(form.baseUrl),
      username: textFieldPatch(form.username),
    }
  }
  const email = primaryEmailFor(form)
  const defaults = email ? imapDefaultsForEmail(email) : null
  return {
    accountId,
    provider: defaults?.provider ?? 'generic',
    auth: defaults?.auth ?? 'password',
    // The base URL belongs to the JMAP driver; an IMAP/SMTP save clears it.
    baseUrl: { kind: 'clear' },
    username: textFieldPatch(form.username),
    imap: {
      host: form.imapHost.trim(),
      port: parsePort(form.imapPort, 993),
      security: form.imapSecurity,
    },
    smtp: {
      host: form.smtpHost.trim(),
      port: parsePort(form.smtpPort, 465),
      security: form.smtpSecurity,
    },
  }
}

function parsePort(value: string, fallback: number): number {
  const parsed = Number.parseInt(value.trim(), 10)
  return Number.isFinite(parsed) && parsed > 0 && parsed <= 65535
    ? parsed
    : fallback
}

/** First concrete (non-wildcard) email address the account claims. */
function primaryEmailFor(form: AccountFormState): string | null {
  const patterns = parseEmailPatterns(form.emailPatternsText)
  const concrete = patterns.find(
    (pattern) => !pattern.includes('*') && pattern.includes('@'),
  )
  if (concrete) {
    return concrete
  }
  const username = form.username.trim()
  return username.includes('@') ? username : null
}

/**
 * Smart, editable defaults inferred from an email domain for well-known IMAP
 * providers (the cheap, high-value alternative to full ISPDB autodiscovery).
 * Returns `null` for an unrecognized/blank domain so the caller can fall back to
 * a generic `imap.<domain>` guess.
 */
export interface ImapProviderDefaults {
  provider: ProviderHint
  auth: ProviderAuthKind
  imap: EndpointSettings
  smtp: EndpointSettings
  /** Human hint shown when the provider requires an app-specific password. */
  appPasswordHint: string | null
}

const KNOWN_IMAP_PROVIDERS: Record<
  string,
  Omit<ImapProviderDefaults, 'imap' | 'smtp'> & {
    imap: EndpointSettings
    smtp: EndpointSettings
    domains: string[]
  }
> = {
  fastmail: {
    domains: ['fastmail.com', 'fastmail.fm', 'messagingengine.com'],
    provider: 'generic',
    auth: 'appPassword',
    imap: { host: 'imap.fastmail.com', port: 993, security: 'tls' },
    smtp: { host: 'smtp.fastmail.com', port: 465, security: 'tls' },
    appPasswordHint:
      'Fastmail requires an app password: Settings → Password & Security → App passwords.',
  },
  icloud: {
    domains: ['icloud.com', 'me.com', 'mac.com'],
    provider: 'icloud',
    auth: 'appPassword',
    imap: { host: 'imap.mail.me.com', port: 993, security: 'tls' },
    smtp: { host: 'smtp.mail.me.com', port: 587, security: 'startTls' },
    appPasswordHint:
      'iCloud Mail requires an app-specific password from appleid.apple.com.',
  },
  gmail: {
    domains: ['gmail.com', 'googlemail.com'],
    provider: 'gmail',
    auth: 'appPassword',
    imap: { host: 'imap.gmail.com', port: 993, security: 'tls' },
    smtp: { host: 'smtp.gmail.com', port: 465, security: 'tls' },
    appPasswordHint:
      'Gmail requires an app password (2-Step Verification must be on): myaccount.google.com/apppasswords.',
  },
  outlook: {
    domains: ['outlook.com', 'hotmail.com', 'live.com', 'msn.com'],
    provider: 'outlook',
    auth: 'password',
    imap: { host: 'outlook.office365.com', port: 993, security: 'tls' },
    smtp: { host: 'smtp.office365.com', port: 587, security: 'startTls' },
    appPasswordHint: null,
  },
}

/** Infer IMAP/SMTP defaults from an email address (or bare domain). */
export function imapDefaultsForEmail(
  email: string,
): ImapProviderDefaults | null {
  const domain = email.trim().toLowerCase().split('@').pop() ?? ''
  if (domain === '' || !domain.includes('.')) {
    return null
  }
  for (const preset of Object.values(KNOWN_IMAP_PROVIDERS)) {
    if (preset.domains.includes(domain)) {
      return {
        provider: preset.provider,
        auth: preset.auth,
        imap: { ...preset.imap },
        smtp: { ...preset.smtp },
        appPasswordHint: preset.appPasswordHint,
      }
    }
  }
  // Generic best-effort guess for an unknown provider domain.
  return {
    provider: 'generic',
    auth: 'password',
    imap: { host: `imap.${domain}`, port: 993, security: 'tls' },
    smtp: { host: `smtp.${domain}`, port: 465, security: 'tls' },
    appPasswordHint: null,
  }
}

/**
 * Merge smart IMAP defaults into form state without clobbering fields the user
 * has already filled in. Used when the user switches the driver to IMAP or the
 * primary email changes, so the endpoint fields prefill but stay editable.
 */
export function applyImapDefaults(form: AccountFormState): AccountFormState {
  const email = primaryEmailFor(form)
  const defaults = email ? imapDefaultsForEmail(email) : null
  if (!defaults) {
    return form
  }
  return {
    ...form,
    imapHost: form.imapHost.trim() === '' ? defaults.imap.host : form.imapHost,
    imapPort:
      form.imapHost.trim() === '' ? String(defaults.imap.port) : form.imapPort,
    imapSecurity:
      form.imapHost.trim() === '' ? defaults.imap.security : form.imapSecurity,
    smtpHost: form.smtpHost.trim() === '' ? defaults.smtp.host : form.smtpHost,
    smtpPort:
      form.smtpHost.trim() === '' ? String(defaults.smtp.port) : form.smtpPort,
    smtpSecurity:
      form.smtpHost.trim() === '' ? defaults.smtp.security : form.smtpSecurity,
  }
}

/** Identity/appearance patch for the `updateAccount` command. */
export function buildIdentityPatch(
  form: AccountFormState,
  accountId: string,
): UpdateAccountIntent {
  return {
    accountId,
    name: form.name.trim(),
    fullName: textFieldPatch(form.fullName),
    signature: textFieldPatch(form.signature),
    emailPatterns: parseEmailPatterns(form.emailPatternsText),
    appearance: buildAccountAppearanceInput(form),
  }
}

/** Whether the connection editor's transport + secret changes also apply on
 * save (manual accounts only — OAuth transport is provider-managed). */
export function shouldWriteTransport(
  editorModel: ExistingAccountEditorModel,
): boolean {
  return editorModel.connection.kind === 'manualCredentials'
}

export function buildAccountAppearanceInput(
  form: AccountFormState,
): AccountAppearance {
  const initials = normalizeAccountInitials(
    form.appearanceInitials || form.name,
  )
  const colorHue = Math.min(
    360,
    Math.max(0, Math.round(form.appearanceColorHue)),
  )
  return {
    kind: 'initials',
    initials,
    colorHue,
  }
}

export function normalizeAccountInitials(value: string): string {
  const trimmed = value.trim().toUpperCase()
  return trimmed.length === 0 ? 'A' : Array.from(trimmed).slice(0, 1).join('')
}
