import { createClientPreferencesStore } from './client-preferences/store'
import type { ClientPreferencesStore } from './client-preferences/types'

export { createClientPreferencesStore } from './client-preferences/store'
export type {
  ClientPreferencesSnapshot,
  ClientPreferencesStore,
} from './client-preferences/types'

export const clientPreferencesStore: ClientPreferencesStore =
  createClientPreferencesStore()
