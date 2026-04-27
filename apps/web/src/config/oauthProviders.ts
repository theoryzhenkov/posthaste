import type { ProviderHint } from '../api/types'
import oauthProviderConfig from './oauthProviders.json'

interface OAuthProviderConfig {
  oauthClientId?: string
  oauthClientSecret?: string
}

interface OAuthClientCredentials {
  clientId: string
  clientSecret?: string
}

const providers = oauthProviderConfig as Partial<
  Record<ProviderHint, OAuthProviderConfig>
>

export const providerOAuthClientCredentials: Partial<
  Record<ProviderHint, OAuthClientCredentials | undefined>
> = {
  gmail: oauthClientCredentials('gmail'),
  outlook: oauthClientCredentials('outlook'),
}

function oauthClientCredentials(
  provider: ProviderHint,
): OAuthClientCredentials | undefined {
  const config = providers[provider]
  if (!config) {
    return undefined
  }
  return {
    clientId: config.oauthClientId?.trim() ?? '',
    clientSecret:
      oauthClientSecretFromEnv(provider) ??
      (config.oauthClientSecret?.trim() || undefined),
  }
}

function oauthClientSecretFromEnv(provider: ProviderHint): string | undefined {
  switch (provider) {
    case 'gmail':
      return (
        import.meta.env.VITE_GOOGLE_OAUTH_CLIENT_SECRET?.trim() || undefined
      )
    case 'outlook':
      return (
        import.meta.env.VITE_MICROSOFT_OAUTH_CLIENT_SECRET?.trim() || undefined
      )
    case 'generic':
    case 'icloud':
      return undefined
  }
}
