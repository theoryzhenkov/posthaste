import { afterEach, describe, expect, it } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'

import { AccountHealthNotice } from '../src/components/settings-panel/AccountHealthNotice'
import type { AccountOverview, AccountRuntime } from '../src/api/types'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

function account(runtime: Partial<AccountRuntime>): AccountOverview {
  return {
    id: 'primary',
    name: 'Primary',
    fullName: null,
    signature: null,
    emailPatterns: ['primary@gmail.com'],
    driver: 'imapSmtp',
    enabled: true,
    appearance: { kind: 'initials', initials: 'P', colorHue: 200 },
    connection: {
      kind: 'managedOAuth',
      provider: 'gmail',
      providerKind: 'gmail',
      auth: 'oauth2',
      username: 'primary@gmail.com',
      imap: null,
      smtp: null,
      secret: { storage: 'os', configured: true, label: null },
    },
    createdAt: '2026-04-28T12:00:00Z',
    updatedAt: '2026-04-28T12:00:00Z',
    isDefault: true,
    runtime: {
      status: 'ready',
      push: 'connected',
      lastSyncAt: null,
      lastSyncError: null,
      lastSyncErrorCode: null,
      syncProgress: null,
      ...runtime,
    },
  }
}

afterEach(() => {
  document.body.innerHTML = ''
})

describe('AccountHealthNotice (M45 degraded/error state + recovery)', () => {
  it('renders nothing for a healthy account', () => {
    const view = render(
      <AccountHealthNotice account={account({ status: 'ready' })} />,
    )
    expect(view.container.textContent).toBe('')
  })

  it('shows a classified network message with a Retry action, never a raw string', () => {
    const clicked: AccountOverview[] = []
    const errored = account({
      status: 'offline',
      lastSyncErrorCode: 'network_error',
      lastSyncError: 'network error: cannot connect to TCP stream',
    })
    const view = render(
      <AccountHealthNotice
        account={errored}
        onAction={(a) => clicked.push(a)}
      />,
    )

    expect(view.container.textContent).toContain('Gmail')
    expect(view.container.textContent).not.toContain('TCP stream')

    const retry = view.getByRole('button', { name: /retry/i })
    fireEvent.click(retry)
    expect(clicked).toHaveLength(1)
    expect(clicked[0]?.id).toBe('primary')
  })

  it('offers Reconnect for an auth error', () => {
    const view = render(
      <AccountHealthNotice
        account={account({
          status: 'authError',
          lastSyncErrorCode: 'auth_error',
        })}
        onAction={() => undefined}
      />,
    )
    expect(view.getByRole('button', { name: /reconnect/i })).toBeTruthy()
  })

  it('clears once the account recovers to ready', () => {
    const errored = account({
      status: 'offline',
      lastSyncErrorCode: 'network_error',
    })
    const view = render(
      <AccountHealthNotice account={errored} onAction={() => undefined} />,
    )
    expect(view.container.textContent).toContain('Gmail')

    // Simulate the supervisor clearing the error latch on a successful retry:
    // the re-served account status view yields a ready runtime with no error.
    view.rerender(
      <AccountHealthNotice
        account={account({ status: 'ready' })}
        onAction={() => undefined}
      />,
    )
    expect(view.container.textContent).toBe('')
  })
})
