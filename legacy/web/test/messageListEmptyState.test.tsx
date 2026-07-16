import { describe, expect, it, mock } from 'bun:test'
import { fireEvent, render } from '@testing-library/react'

import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

// Spy on surface navigation so we can assert the CTA routes to account settings.
const openFocusedSurface = mock(() => {})
mock.module('@/hooks/useSurfaceRouting', () => ({ openFocusedSurface }))

const { NoMailboxSelected, EmptyMessages } =
  await import('../src/components/message-list/MessageListStates')

const noop = () => {}

describe('NoMailboxSelected onboarding CTA', () => {
  it('with no accounts, shows an "Add an account" CTA that opens account settings', () => {
    openFocusedSurface.mockClear()
    const { getByRole, queryByText } = render(
      <NoMailboxSelected onMouseDown={noop} hasNoAccounts={true} />,
    )

    // Onboarding copy, not the dead-end "pick a mailbox".
    expect(queryByText('No accounts yet')).not.toBeNull()
    expect(queryByText('No mailbox selected')).toBeNull()

    const button = getByRole('button', { name: /add an account/i })
    fireEvent.click(button)

    expect(openFocusedSurface).toHaveBeenCalledTimes(1)
    const surface = openFocusedSurface.mock.calls[0][0] as {
      kind: string
      params: { category?: string }
    }
    expect(surface.kind).toBe('settings')
    expect(surface.params.category).toBe('accounts')
  })

  it('with accounts present, shows the plain "no mailbox selected" state (no CTA)', () => {
    const { queryByText, queryByRole } = render(
      <NoMailboxSelected onMouseDown={noop} />,
    )
    expect(queryByText('No mailbox selected')).not.toBeNull()
    expect(queryByRole('button', { name: /add an account/i })).toBeNull()
  })
})

describe('EmptyMessages', () => {
  it('renders the plain empty state, and the syncing state when mid-sync', () => {
    const plain = render(<EmptyMessages />)
    expect(plain.queryByText('No messages here yet')).not.toBeNull()

    const syncing = render(<EmptyMessages isSyncing={true} />)
    expect(syncing.queryByText('Syncing your mail…')).not.toBeNull()
  })
})
