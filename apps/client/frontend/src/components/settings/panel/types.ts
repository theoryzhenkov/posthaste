/**
 * Shared types for the settings panel editor components.
 */
import type { AccountDriver, MailQueryRule, TransportSecurity } from '@/gen'

/** Editor target: `"new"` for create mode, or an existing entity ID. */
export type EditorTarget = 'new' | string
/** Smart mailbox editor target: `"new"` for create mode, or an existing mailbox ID. */
export type SmartMailboxEditorTarget = 'new' | string

/** Driver choices offered in the manual add-account form. */
export type ManualAccountDriver = Extract<AccountDriver, 'jmap' | 'imapSmtp'>

export interface AccountFormState {
  name: string
  fullName: string
  signature: string
  emailPatternsText: string
  appearanceInitials: string
  appearanceColorHue: number
  /** Backend transport driver the manual form is configuring. */
  driver: ManualAccountDriver
  /** JMAP base URL (used only when `driver === 'jmap'`). */
  baseUrl: string
  username: string
  password: string
  /** IMAP/SMTP endpoint fields (used only when `driver === 'imapSmtp'`). Ports
   * are kept as strings for the text inputs and parsed at payload build time. */
  imapHost: string
  imapPort: string
  imapSecurity: TransportSecurity
  smtpHost: string
  smtpPort: string
  smtpSecurity: TransportSecurity
}

export interface SmartMailboxFormState {
  name: string
  /** Optional view role; `null` for a plain saved query. */
  role: string | null
  rule: MailQueryRule
}
