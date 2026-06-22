import { describe, expect, it } from 'bun:test'

import { shouldForceAccountSettings } from '../src/accountSetup'

describe('account setup routing', () => {
  it('does not force settings before the account query settles', () => {
    expect(
      shouldForceAccountSettings({
        accounts: [],
        accountsSettled: false,
      }),
    ).toBe(false)
  })

  it('forces settings only after a settled, empty account query', () => {
    expect(
      shouldForceAccountSettings({
        accounts: [],
        accountsSettled: true,
      }),
    ).toBe(true)
  })

  it('does not force settings when accounts are configured', () => {
    expect(
      shouldForceAccountSettings({
        accounts: [{}],
        accountsSettled: true,
      }),
    ).toBe(false)
  })

  it('does not force settings on a transient empty mid-fetch', () => {
    // The regression: a post-mutation refetch must not flip the UI into
    // Settings. `accountsSettled` is false while fetching, so an empty list
    // observed during the refetch is ignored.
    expect(
      shouldForceAccountSettings({
        accounts: [],
        accountsSettled: false,
      }),
    ).toBe(false)
  })
})
