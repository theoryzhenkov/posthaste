import type { ProviderHint } from '../api/types'
import oauthProviderConfig from './oauthProviders.json'

interface OAuthProviderConfig {
  oauthClientId?: string
}

const providers = oauthProviderConfig as Partial<
  Record<ProviderHint, OAuthProviderConfig>
>

export const providerOAuthClientIds: Partial<Record<ProviderHint, string>> = {
  gmail: providers.gmail?.oauthClientId?.trim() ?? '',
  outlook: providers.outlook?.oauthClientId?.trim() ?? '',
}
