import type { ProviderHint } from '../api/types'
import oauthProviderConfig from './oauthProviders.json'

interface OAuthProviderConfig {
  oauthClientId?: string
  oauthClientSecret?: string
}

const providers = oauthProviderConfig as Partial<
  Record<ProviderHint, OAuthProviderConfig>
>

export const providerOAuthClientCredentials: Partial<
  Record<ProviderHint, { clientId: string; clientSecret?: string }>
> = Object.fromEntries(
  Object.entries(providers).map(([provider, config]) => [
    provider,
    {
      clientId: config.oauthClientId?.trim() ?? '',
      clientSecret: config.oauthClientSecret?.trim() || undefined,
    },
  ]),
) as Partial<Record<ProviderHint, { clientId: string; clientSecret?: string }>>
