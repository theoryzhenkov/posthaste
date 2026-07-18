import { createClientPreferencesStore } from './store'
import type { ClientPreferencesStore } from './types'

export type { ClientPreferencesStore } from './types'

export const clientPreferencesStore: ClientPreferencesStore =
  createClientPreferencesStore()
