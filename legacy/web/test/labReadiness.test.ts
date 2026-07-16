import { describe, expect, it } from 'bun:test'

import {
  appReadinessStateFromAccountsQuery,
  LAB_READINESS_STATES,
  settingsReadinessStateFromQueries,
} from '../src/labReadiness'

describe('Lab readiness state markers', () => {
  it('marks the app ready only after the accounts query succeeds', () => {
    expect(
      appReadinessStateFromAccountsQuery({
        isLoading: true,
        isSuccess: false,
        isError: false,
      }),
    ).toBe(LAB_READINESS_STATES.appLoading)

    expect(
      appReadinessStateFromAccountsQuery({
        isLoading: false,
        isSuccess: true,
        isError: false,
      }),
    ).toBe(LAB_READINESS_STATES.appReady)
  })

  it('exposes an app error marker when the accounts query fails', () => {
    expect(
      appReadinessStateFromAccountsQuery({
        isLoading: false,
        isSuccess: false,
        isError: true,
      }),
    ).toBe(LAB_READINESS_STATES.appError)
  })

  it('waits for active settings queries before marking settings ready', () => {
    expect(
      settingsReadinessStateFromQueries([
        { isLoading: false, isError: false },
        { isLoading: true, isError: false },
        { enabled: false, isLoading: true, isError: true },
      ]),
    ).toBe(LAB_READINESS_STATES.settingsLoading)

    expect(
      settingsReadinessStateFromQueries([
        { isLoading: false, isError: false },
        { isLoading: false, isError: false },
        { enabled: false, isLoading: true, isError: true },
      ]),
    ).toBe(LAB_READINESS_STATES.settingsReady)
  })

  it('exposes a settings error marker when an active settings query fails', () => {
    expect(
      settingsReadinessStateFromQueries([
        { isLoading: false, isError: false },
        { isLoading: false, isError: true },
      ]),
    ).toBe(LAB_READINESS_STATES.settingsError)
  })
})
