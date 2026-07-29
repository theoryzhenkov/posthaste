import { describe, expect, it } from 'bun:test'

import { shouldForceAccountSettings } from '../src/accountSetup'

describe('account setup routing', () => {
  it('does not force settings before the account query succeeds', () => {
    expect(
      shouldForceAccountSettings({
        accounts: [],
        accountsQuerySucceeded: false,
      }),
    ).toBe(false)
  })

  it('forces settings after a successful empty account query', () => {
    expect(
      shouldForceAccountSettings({
        accounts: [],
        accountsQuerySucceeded: true,
      }),
    ).toBe(true)
  })

  it('does not force settings when accounts are configured', () => {
    expect(
      shouldForceAccountSettings({
        accounts: [{}],
        accountsQuerySucceeded: true,
      }),
    ).toBe(false)
  })
})
