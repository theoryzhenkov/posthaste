/**
 * Diagnostics bundle: a sanitized, copyable snapshot of app + account state for
 * beta support. Pure functions so the sanitization + format are unit-testable.
 *
 * Privacy: the bundle NEVER includes message bodies, auth tokens, account
 * credentials, or email addresses. Account names + addresses are omitted; only
 * structural fields (driver, status, push, last-error code) are included, and
 * any free-text error message is run through {@link sanitizeText} (email
 * masking + secret redaction) before inclusion.
 *
 * @see docs/eph/REPORT-L2-public-beta-readiness-audit.md (diagnostics/support)
 */
import type { AccountOverview, AccountStatus, PushStatus } from './api/types'
import type { ReleaseChannel } from './runtime/releaseChannel'

/** Structural per-account fields surfaced in the diagnostics bundle. */
export interface DiagnosticsAccountSummary {
  driver: string
  enabled: boolean
  status: AccountStatus
  push: PushStatus
  lastSyncErrorCode: string | null
  lastSyncError: string | null
}

/** Inputs to {@link formatDiagnosticsBundle}; gathered renderer-side. */
export interface DiagnosticsBundleInput {
  appVersion: string
  releaseChannel: ReleaseChannel
  os: string
  arch: string
  logDirPath: string | null
  accounts: readonly AccountOverview[]
  generatedAt: Date
}

const EMAIL_PATTERN = /[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}/g

/** `Bearer <8+ token-chars>` — the keyword almost always precedes a real token. */
const BEARER_PATTERN = /\bbearer\s+[A-Za-z0-9._~+/=-]{8,}/gi

/** `<keyword>=<value>` / `<keyword>: <value>` (explicit delimiter, 4+ chars). */
const KV_SECRET_PATTERN =
  /(?:password|passwd|secret|token|api[_-]?key|apikey|private[_-]?key|credential)\s*[:=]\s*["']?[A-Za-z0-9._~+/=-]{4,}/gi

/** A long opaque blob (base64-ish, 40+ chars) that is almost certainly a token. */
const LONG_OPAQUE_PATTERN = /\b[A-Za-z0-9+/]{40,}={0,2}\b/g

/** Replace email addresses with `[email]`. */
export function maskEmails(text: string): string {
  return text.replace(EMAIL_PATTERN, '[email]')
}

/** Redact token/secret-like substrings from free text. */
export function redactSecrets(text: string): string {
  return text
    .replace(BEARER_PATTERN, '[redacted]')
    .replace(KV_SECRET_PATTERN, '[redacted]')
    .replace(LONG_OPAQUE_PATTERN, '[redacted]')
}

/** Full sanitization pass for any free-text field: mask emails, redact secrets. */
export function sanitizeText(text: string): string {
  return redactSecrets(maskEmails(text))
}

/**
 * Collapse a (possibly multi-line) error message to a single compact line,
 * sanitized + truncated so the bundle stays readable + leak-free.
 */
function compactError(text: string): string {
  const oneLine = text.replace(/\s+/g, ' ').trim()
  const sanitized = sanitizeText(oneLine)
  return sanitized.length > 200 ? `${sanitized.slice(0, 200)}…` : sanitized
}

/** Project an account down to the structural fields the bundle includes. */
export function summarizeAccount(account: AccountOverview): DiagnosticsAccountSummary {
  return {
    driver: account.driver,
    enabled: account.enabled,
    status: account.runtime.status,
    push: account.runtime.push,
    lastSyncErrorCode: account.runtime.lastSyncErrorCode,
    lastSyncError: account.runtime.lastSyncError
      ? compactError(account.runtime.lastSyncError)
      : null,
  }
}

/** Format the sanitized diagnostics bundle as copyable text. */
export function formatDiagnosticsBundle(input: DiagnosticsBundleInput): string {
  const platform = input.arch ? `${input.os} ${input.arch}` : input.os
  const lines: string[] = [
    'Posthaste diagnostics',
    '====================',
    `Generated: ${input.generatedAt.toISOString()}`,
    `Version: ${input.appVersion} (${input.releaseChannel})`,
    `Platform: ${platform}`,
    '',
    `Accounts (${input.accounts.length}):`,
  ]

  if (input.accounts.length === 0) {
    lines.push('  (none configured)')
  } else {
    input.accounts.forEach((account, index) => {
      const summary = summarizeAccount(account)
      const parts: string[] = [`[${summary.driver}]`]
      if (!summary.enabled) {
        parts.push('disabled')
      }
      parts.push(summary.status)
      parts.push(`(push: ${summary.push})`)
      if (summary.lastSyncErrorCode) {
        parts.push(`— last error: ${summary.lastSyncErrorCode}`)
        if (summary.lastSyncError) {
          parts.push(summary.lastSyncError)
        }
      }
      lines.push(`  ${index + 1}. ${parts.join(' ')}`)
    })
  }

  lines.push('')
  lines.push(
    `Log location: ${input.logDirPath ?? '(not available — see release notes)'}`,
  )
  return lines.join('\n')
}
