import { jsonRequest } from './core'

import type { SyncMode } from '../types'

interface TriggerSyncInput {
  sourceId: string
  mode?: SyncMode
}

function normalizeTriggerSyncInput(
  input: string | TriggerSyncInput,
): TriggerSyncInput {
  return typeof input === 'string' ? { sourceId: input } : input
}

/** @spec docs/L1-api#endpoint-table */
export async function triggerSync(
  input: string | TriggerSyncInput,
): Promise<{ ok: boolean; eventCount: number; mode: SyncMode }> {
  const { sourceId, mode = 'incremental' } = normalizeTriggerSyncInput(input)
  return jsonRequest<{ ok: boolean; eventCount: number; mode: SyncMode }>(
    `/sources/${sourceId}/commands/sync`,
    'POST',
    { mode },
  )
}
