import { useMutation } from '@tanstack/react-query'
import { Cloud, Loader2, Mail, Settings2 } from 'lucide-react'
import { useState, type ReactNode } from 'react'

import type { ProviderHint } from '@/gen'
import { providerOAuthClientCredentials } from '../../../config/oauthProviders'
import { openExternalUrl } from '../../../desktop'
import { Button } from '../../ui/button'
import { SettingsPageHeader } from '../shared'
import {
  oauthRedirectUri,
  useOauthCallbackCapture,
  useStartOauth,
} from './oauth'

class OAuthOpenError extends Error {
  readonly authorizationUrl: string

  constructor(message: string, authorizationUrl: string) {
    super(message)
    this.name = 'OAuthOpenError'
    this.authorizationUrl = authorizationUrl
  }
}

export function AccountSetupChoice({ onManual }: { onManual: () => void }) {
  const startOauth = useStartOauth()
  const callback = useOauthCallbackCapture()
  const [errorMessage, setErrorMessage] = useState<string | null>(null)
  const [fallbackAuthorizationUrl, setFallbackAuthorizationUrl] = useState<
    string | null
  >(null)
  const [startedProvider, setStartedProvider] = useState<ProviderHint | null>(
    null,
  )
  const startOAuthMutation = useMutation({
    mutationFn: async (provider: ProviderHint) => {
      const credentials = providerOAuthClientCredentials[provider]
      const clientId = credentials?.clientId.trim()
      if (!clientId) {
        throw new Error(
          `${providerLabel(provider)} OAuth client ID is not configured`,
        )
      }
      const session = await startOauth({
        provider,
        clientId,
        clientSecret: credentials?.clientSecret,
        redirectUri: oauthRedirectUri(),
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

      {callback.kind === 'completing' && (
        <p className="mt-4 flex items-center gap-2 text-[12px] text-muted-foreground">
          <Loader2 size={14} className="animate-spin" />
          Completing the provider authorization…
        </p>
      )}
      {callback.kind === 'done' && (
        <p className="mt-4 text-[12px] text-muted-foreground">
          Account connected — it appears in the accounts list.
        </p>
      )}
      {callback.kind === 'error' && (
        <p className="mt-4 text-[12px] text-destructive">{callback.message}</p>
      )}
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

function providerLabel(provider: ProviderHint): string {
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
