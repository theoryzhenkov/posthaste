import { createClientPreferencesStore } from './store'
import type { ClientPreferencesStore } from './types'

export { createClientPreferencesStore } from './store'
export type {
  ClientPreferencesSnapshot,
  ClientPreferencesStore,
} from './types'

export const clientPreferencesStore: ClientPreferencesStore =
  createClientPreferencesStore()
