import { afterEach, describe, expect, it } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  act,
  fireEvent,
  render,
  renderHook,
  waitFor,
} from '@testing-library/react'
import type { ReactNode } from 'react'

import { AccountEditor } from '../src/components/settings-panel/AccountEditor'
import { ApiError } from '../src/api/errors'
import { AccountSetupChoice } from '../src/components/settings-panel/accounts-pane/AccountSetupChoice'
import { useAccountCommandMutation } from '../src/components/settings-panel/useAccountCommandMutation'
import type { AccountOverview } from '../src/api/types'
import {
  resetRuntimeAdapterForTesting,
  setRuntimeAdapterForTesting,
} from '../src/runtime/adapter'
import { createFakeRuntimeAdapter } from '../src/runtime/fakeAdapter'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

const account: AccountOverview = {
  id: 'primary',
  name: 'Primary',
  fullName: null,
  emailPatterns: ['primary@example.com'],
  driver: 'mock',
  enabled: true,
  appearance: { kind: 'initials', initials: 'P', colorHue: 200 },
  connection: {
    kind: 'manualCredentials',
    provider: 'generic',
    providerKind: 'generic',
    auth: 'password',
    baseUrl: null,
    username: 'primary@example.com',
    imap: null,
    smtp: null,
    secret: { storage: 'os', configured: true, label: null },
  },
  createdAt: '2026-04-28T12:00:00Z',
  updatedAt: '2026-04-28T12:00:00Z',
  isDefault: true,
  runtime: {
    status: 'ready',
    push: 'disabled',
    lastSyncAt: null,
    lastSyncError: null,
    lastSyncErrorCode: null,
    syncProgress: null,
  },
}

const fallbackAccount: AccountOverview = {
  ...account,
  id: 'fallback',
  name: 'Fallback',
  emailPatterns: ['fallback@example.com'],
  isDefault: false,
}

const queryClients: QueryClient[] = []

function createQueryClient(): QueryClient {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  })
  queryClients.push(queryClient)
  return queryClient
}

function withQueryClient(queryClient = createQueryClient()) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    )
  }
}

afterEach(() => {
  resetRuntimeAdapterForTesting()
  for (const queryClient of queryClients.splice(0)) queryClient.clear()
})

describe('account settings runtime adapter UI paths', () => {
  it('verifies an existing account through a fake runtime adapter', async () => {
    const fake = createFakeRuntimeAdapter({
      defaultVerificationResponse: {
        ok: true,
        identityEmail: 'primary@example.com',
        pushSupported: false,
      },
    })
    let verified = 0
    setRuntimeAdapterForTesting(fake)

    const view = render(
      <AccountEditor
        editorTarget="primary"
        editingAccount={account}
        onSaved={async () => undefined}
        onVerified={async () => {
          verified += 1
        }}
        onCommand={() => undefined}
        isCommandPending={false}
        commandError={null}
      />,
      { wrapper: withQueryClient() },
    )

    await act(async () => {
      fireEvent.click(view.getByRole('button', { name: 'Verify connection' }))
    })

    await waitFor(() => {
      expect(fake.accountVerificationCalls).toEqual(['primary'])
      expect(verified).toBe(1)
    })
    expect(
      view.getByText('Verified identity: primary@example.com'),
    ).toBeTruthy()
  })

  // Seed account used to drive the create-account form: a manual-credentials
  // account whose email is a known IMAP provider, so switching the protocol
  // picker to IMAP prefills the endpoints from smart defaults (the form's text
  // inputs are exercised at the pure-helper level; here we drive the button-only
  // interactions the happy-dom harness dispatches reliably).
  const fastmailSeed: AccountOverview = {
    ...account,
    id: 'seed',
    name: 'Fastmail',
    driver: 'jmap',
    emailPatterns: ['me@fastmail.com'],
    connection: {
      kind: 'manualCredentials',
      provider: 'generic',
      providerKind: 'generic',
      auth: 'password',
      baseUrl: 'https://mail.example.com/jmap',
      username: 'me@fastmail.com',
      imap: null,
      smtp: null,
      secret: { storage: 'os', configured: true, label: null },
    },
  }

  it('submits an IMAP/SMTP account with smart-default endpoints from the form', async () => {
    const fake = createFakeRuntimeAdapter({ defaultAccount: fastmailSeed })
    setRuntimeAdapterForTesting(fake)

    const view = render(
      <AccountEditor
        editorTarget="new"
        editingAccount={fastmailSeed}
        onSaved={async () => undefined}
        onVerified={async () => undefined}
        onCommand={() => undefined}
        isCommandPending={false}
        commandError={null}
      />,
      { wrapper: withQueryClient() },
    )

    // The manual form defaults to the JMAP base URL field.
    expect(
      view.getByPlaceholderText('https://mail.example.com/jmap'),
    ).toBeTruthy()

    // Switching the protocol picker to IMAP swaps in the endpoint fields and
    // prefills them from the fastmail.com smart defaults.
    await act(async () => {
      fireEvent.click(view.getByRole('radio', { name: 'IMAP / SMTP' }))
    })
    expect(view.getByPlaceholderText('imap.example.com')).toBeTruthy()
    expect(view.getByPlaceholderText('smtp.example.com')).toBeTruthy()
    expect(
      view.queryByPlaceholderText('https://mail.example.com/jmap'),
    ).toBeNull()

    await act(async () => {
      fireEvent.click(view.getByRole('button', { name: 'Apply' }))
    })

    await waitFor(() => {
      expect(fake.accountCreateCalls.length).toBe(1)
    })
    const [created] = fake.accountCreateCalls
    expect(created.driver).toBe('imapSmtp')
    expect(created.transport).toMatchObject({
      provider: 'generic',
      auth: 'appPassword',
      username: 'me@fastmail.com',
      imap: { host: 'imap.fastmail.com', port: 993, security: 'tls' },
      smtp: { host: 'smtp.fastmail.com', port: 465, security: 'tls' },
    })
  })

  it('surfaces a classified error (with app-password hint) when IMAP creation fails', async () => {
    const fake = createFakeRuntimeAdapter()
    fake.queueAccountError(
      new ApiError(
        401,
        'Unauthorized',
        'authentication failed: LOGIN rejected',
        'auth_error',
      ),
    )
    setRuntimeAdapterForTesting(fake)

    const view = render(
      <AccountEditor
        editorTarget="new"
        editingAccount={fastmailSeed}
        onSaved={async () => undefined}
        onVerified={async () => undefined}
        onCommand={() => undefined}
        isCommandPending={false}
        commandError={null}
      />,
      { wrapper: withQueryClient() },
    )

    await act(async () => {
      fireEvent.click(view.getByRole('radio', { name: 'IMAP / SMTP' }))
    })
    await act(async () => {
      fireEvent.click(view.getByRole('button', { name: 'Apply' }))
    })

    // The banner shows the classified auth message with the Fastmail
    // app-password hint appended, and never leaks the raw library string.
    await waitFor(() => {
      expect(
        view.getByText(
          (content) =>
            content.includes('Sign-in was rejected') &&
            content.includes('app password'),
        ),
      ).toBeTruthy()
    })
    expect(view.queryByText(/LOGIN rejected/)).toBeNull()
  })

  it('starts provider OAuth through a fake runtime adapter', async () => {
    const fake = createFakeRuntimeAdapter({
      defaultOAuthStartResponse: {
        authorizationUrl: 'https://accounts.example.test/auth',
        state: 'state-1',
        redirectUri: 'http://localhost:3001/v1/oauth/callback',
      },
    })
    const openedUrls: string[] = []
    const originalOpen = window.open
    window.open = ((url: string | URL | undefined) => {
      openedUrls.push(String(url))
      return window
    }) as typeof window.open
    setRuntimeAdapterForTesting(fake)

    try {
      const view = render(<AccountSetupChoice onManual={() => undefined} />, {
        wrapper: withQueryClient(),
      })

      await act(async () => {
        fireEvent.click(view.getByRole('button', { name: 'Google' }))
      })

      await waitFor(() => {
        expect(fake.oauthStartCalls.length).toBe(1)
        expect(openedUrls).toEqual(['https://accounts.example.test/auth'])
      })
      const [oauthCall] = fake.oauthStartCalls
      expect(oauthCall.provider).toBe('gmail')
      expect(oauthCall.clientId.length).toBeGreaterThan(0)
      expect(oauthCall.redirectUri).toBe(
        'http://localhost:3001/v1/oauth/callback',
      )
      expect(
        view.getByText('Google authorization opened in your browser.'),
      ).toBeTruthy()
    } finally {
      window.open = originalOpen
    }
  })

  it('runs account commands through a fake runtime adapter', async () => {
    const fake = createFakeRuntimeAdapter()
    const queryClient = createQueryClient()
    const activeAccountIds: Array<string | null> = []
    const navigations: unknown[] = []
    const errors: Array<string | null> = []
    setRuntimeAdapterForTesting(fake)

    const { result } = renderHook(
      () =>
        useAccountCommandMutation({
          accounts: [account, fallbackAccount],
          activeAccountId: 'primary',
          effectiveEditorTarget: 'primary',
          onActiveAccountChange: (accountId) =>
            activeAccountIds.push(accountId),
          onNavigate: (surface) => navigations.push(surface),
          queryClient,
          setAccountCommandError: (message) => errors.push(message),
        }),
      { wrapper: withQueryClient(queryClient) },
    )

    await act(async () => {
      result.current.mutate({ action: 'disable', account })
    })
    await waitFor(() =>
      expect(fake.accountCommandCalls).toEqual([
        { kind: 'disable', accountId: 'primary' },
      ]),
    )

    await act(async () => {
      result.current.mutate({ action: 'delete', account })
    })
    await waitFor(() =>
      expect(fake.accountCommandCalls).toEqual([
        { kind: 'disable', accountId: 'primary' },
        { kind: 'delete', accountId: 'primary' },
      ]),
    )
    expect(activeAccountIds).toEqual(['fallback'])
    expect(navigations).toEqual([
      {
        kind: 'settings',
        disposition: 'focused',
        params: { category: 'accounts' },
      },
    ])
    expect(errors).toEqual([null, null])
  })
})
