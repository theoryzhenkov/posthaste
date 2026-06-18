import type {
  AccountOverview,
  ProviderAuthKind,
  ProviderKind,
} from '../../../api/types'

export function providerLabel(provider: ProviderKind): string {
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

export function driverLabel(driver: AccountOverview['driver']): string {
  switch (driver) {
    case 'jmap':
      return 'JMAP'
    case 'imapSmtp':
      return 'IMAP/SMTP'
    case 'mock':
      return 'Mock'
  }
}
