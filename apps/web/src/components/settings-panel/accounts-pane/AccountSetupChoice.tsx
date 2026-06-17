import { useMutation } from '@tanstack/react-query'
import { Cloud, Mail, Settings2 } from 'lucide-react'
import { useState, type ReactNode } from 'react'

import type { ProviderKind } from '../../../api/types'
import { providerOAuthClientCredentials } from '../../../config/oauthProviders'
import { openExternalUrl } from '../../../desktop'
import { runtimeMutations } from '../../../runtime/mutations'
import { runtimeViews } from '../../../runtime/views'
import { Button } from '../../ui/button'
import { SettingsPageHeader } from '../shared'

class OAuthOpenError extends Error {
  readonly authorizationUrl: string

  constructor(message: string, authorizationUrl: string) {
    super(message)
    this.name = 'OAuthOpenError'
    this.authorizationUrl = authorizationUrl
  }
}

export function AccountSetupChoice({ onManual }: { onManual: () => void }) {
  const [errorMessage, setErrorMessage] = useState<string | null>(null)
  const [fallbackAuthorizationUrl, setFallbackAuthorizationUrl] = useState<
    string | null
  >(null)
  const [startedProvider, setStartedProvider] = useState<ProviderKind | null>(
    null,
  )
  const startOAuthMutation = useMutation({
    mutationFn: async (provider: ProviderKind) => {
      const credentials = providerOAuthClientCredentials[provider]
      const clientId = credentials?.clientId.trim()
      if (!clientId) {
        throw new Error(
          `${providerLabel(provider)} OAuth client ID is not configured`,
        )
      }
      const session = await runtimeMutations.oauth.startProvider({
        provider,
        clientId,
        clientSecret: credentials?.clientSecret,
        redirectUri: runtimeViews.oauth.redirectUri(),
      })
      try {
        await openExternalUrl(session.authorizationUrl)
      } catch (error) {
        const message =
          error instanceof Error
            ? error.message
            : 'Could not open the authorization URL.'
        throw new OAuthOpenError(message, session.authorizationUrl)
      }
      return { provider }
    },
    onSuccess: ({ provider }) => {
      setErrorMessage(null)
      setFallbackAuthorizationUrl(null)
      setStartedProvider(provider)
    },
    onError: (error: Error) => {
      setStartedProvider(null)
      if (error instanceof OAuthOpenError) {
        setFallbackAuthorizationUrl(error.authorizationUrl)
        setErrorMessage(error.message)
        return
      }
      setFallbackAuthorizationUrl(null)
      setErrorMessage(error.message)
    },
  })

  return (
    <div className="pb-8">
      <SettingsPageHeader
        title="New account"
        description="Choose a provider, or configure the connection manually."
      />

      <div className="grid gap-3 sm:grid-cols-2">
        <ProviderButton
          icon={<Mail size={17} strokeWidth={1.8} />}
          label="Google"
          disabled={startOAuthMutation.isPending}
          onClick={() => startOAuthMutation.mutate('gmail')}
        />
        <ProviderButton
          icon={<Cloud size={17} strokeWidth={1.8} />}
          label="Outlook"
          disabled={startOAuthMutation.isPending}
          onClick={() => startOAuthMutation.mutate('outlook')}
        />
        <ProviderButton
          icon={<Settings2 size={17} strokeWidth={1.8} />}
          label="Manual"
          disabled={startOAuthMutation.isPending}
          onClick={onManual}
        />
      </div>

      {startedProvider && (
        <p className="mt-4 text-[12px] text-muted-foreground">
          {providerLabel(startedProvider)} authorization opened in your browser.
        </p>
      )}
      {errorMessage && (
        <p className="mt-4 text-[12px] text-destructive">{errorMessage}</p>
      )}
      {fallbackAuthorizationUrl && (
        <p className="mt-2 break-all text-[12px] text-muted-foreground">
          Open this authorization URL manually:{' '}
          <a
            className="text-primary underline underline-offset-2"
            href={fallbackAuthorizationUrl}
            target="_blank"
            rel="noreferrer"
          >
            {fallbackAuthorizationUrl}
          </a>
        </p>
      )}
    </div>
  )
}

function ProviderButton({
  icon,
  label,
  disabled,
  onClick,
}: {
  icon: ReactNode
  label: string
  disabled: boolean
  onClick: () => void
}) {
  return (
    <Button
      type="button"
      variant="outline"
      disabled={disabled}
      onClick={onClick}
      className="h-12 justify-start rounded-md border-border bg-bg-elev px-4 text-[13px]"
    >
      <span className="flex size-7 items-center justify-center rounded-md bg-background text-muted-foreground">
        {icon}
      </span>
      {label}
    </Button>
  )
}

function providerLabel(provider: ProviderKind): string {
  switch (provider) {
    case 'gmail':
      return 'Google'
    case 'outlook':
      return 'Outlook'
    case 'icloud':
      return 'iCloud'
    case 'generic':
      return 'Provider'
  }
}
