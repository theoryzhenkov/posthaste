export const LAB_READINESS_STATES = {
  appLoading: 'state.app.loading.test',
  appReady: 'state.app.ready.test',
  appError: 'state.app.error.test',
  settingsLoading: 'state.settings.loading.test',
  settingsReady: 'state.settings.ready.test',
  settingsError: 'state.settings.error.test',
} as const

export type AppReadinessState =
  | typeof LAB_READINESS_STATES.appLoading
  | typeof LAB_READINESS_STATES.appReady
  | typeof LAB_READINESS_STATES.appError

export type SettingsReadinessState =
  | typeof LAB_READINESS_STATES.settingsLoading
  | typeof LAB_READINESS_STATES.settingsReady
  | typeof LAB_READINESS_STATES.settingsError

export interface AppReadinessQueryState {
  isLoading: boolean
  isSuccess: boolean
  isError: boolean
}

export interface LabReadinessQueryState {
  isLoading: boolean
  isError: boolean
  enabled?: boolean
}

export function appReadinessStateFromAccountsQuery({
  isLoading,
  isSuccess,
  isError,
}: AppReadinessQueryState): AppReadinessState {
  if (isError) {
    return LAB_READINESS_STATES.appError
  }
  if (isSuccess) {
    return LAB_READINESS_STATES.appReady
  }
  if (isLoading) {
    return LAB_READINESS_STATES.appLoading
  }
  return LAB_READINESS_STATES.appLoading
}

export function settingsReadinessStateFromQueries(
  queries: readonly LabReadinessQueryState[],
): SettingsReadinessState {
  const activeQueries = queries.filter((query) => query.enabled ?? true)
  if (activeQueries.some((query) => query.isError)) {
    return LAB_READINESS_STATES.settingsError
  }
  if (activeQueries.some((query) => query.isLoading)) {
    return LAB_READINESS_STATES.settingsLoading
  }
  return LAB_READINESS_STATES.settingsReady
}
