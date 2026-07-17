import type { AccountDriver, ProviderAuthKind, ProviderHint } from '@/gen'

export function providerLabel(provider: ProviderHint): string {
  switch (provider) {
    case 'gmail':
      return 'Google'
    case 'outlook':
      return 'Outlook'
    case 'icloud':
      return 'iCloud'
    case 'generic':
      return 'Generic'
  }
}

export function authLabel(auth: ProviderAuthKind): string {
  switch (auth) {
    case 'oauth2':
      return 'OAuth 2.0'
    case 'appPassword':
      return 'App password'
    case 'password':
      return 'Password'
  }
}

export function driverLabel(driver: AccountDriver): string {
  switch (driver) {
    case 'jmap':
      return 'JMAP'
    case 'imapSmtp':
      return 'IMAP/SMTP'
    case 'mock':
      return 'Mock'
  }
}
