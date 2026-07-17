/**
 * Backend wiring for global tag rename/delete. Injects the real carrier
 * enumeration (an `automationRulePreview` query evaluating `keyword = name`)
 * and the per-message `setKeywords` command into the pure
 * {@link ./tagMaintenance} orchestration, tracks inline progress, migrates
 * the appearance overlay, and surfaces partial failures as a toast.
 *
 * Commands are posted directly (not through `runCommand`) so a bulk run does
 * not storm the global invalidation policy; every active query is invalidated
 * once when the run settles.
 */
import { useCallback, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'

import type { AutomationRulePreviewResult, TagAppearance } from '@/gen'
import { useMailClient } from '@/data/context'
import { fetchQuery } from '@/data/queries'

import {
  deleteTagAcrossCarriers,
  dropTagAppearance,
  migrateTagAppearance,
  renameTagAcrossCarriers,
  type TagBulkResult,
  type TagCarrierBatch,
  type TagMaintenanceDeps,
} from './tagMaintenance'
import { useTagAppearanceMutation } from './useTagAppearanceMutation'

export interface TagMaintenanceProgress {
  action: 'rename' | 'delete'
  tag: string
  done: number
  total: number
}

/** Carriers fetched per enumeration round; the backend caps samples at 200. */
const CARRIER_BATCH_LIMIT = 200

export function useTagMaintenance() {
  const client = useMailClient()
  const queryClient = useQueryClient()
  const appearanceMutation = useTagAppearanceMutation()
  const [progress, setProgress] = useState<TagMaintenanceProgress | null>(null)

  const enumerateBatch = useCallback(
    async (tag: string): Promise<TagCarrierBatch> => {
      const result = await fetchQuery<AutomationRulePreviewResult>(client, {
        automationRulePreview: {
          condition: {
            root: {
              operator: 'all',
              negated: false,
              nodes: [
                {
                  type: 'condition',
                  field: 'keyword',
                  operator: 'equals',
                  negated: false,
                  value: tag,
                },
              ],
            },
          },
          limit: CARRIER_BATCH_LIMIT,
        },
      })
      return {
        total: result.total,
        carriers: result.rows.map((row) => ({
          sourceId: row.sourceId,
          messageId: row.id,
        })),
      }
    },
    [client],
  )

  const run = useCallback(
    async (
      action: 'rename' | 'delete',
      tag: string,
      operation: (deps: TagMaintenanceDeps) => Promise<TagBulkResult>,
      nextAppearance: TagAppearance[] | null,
    ) => {
      setProgress({ action, tag, done: 0, total: 0 })
      const deps: TagMaintenanceDeps = {
        enumerateBatch,
        applyKeywords: async (carrier, delta) => {
          await client.command({
            setKeywords: {
              accountId: carrier.sourceId,
              messageId: carrier.messageId,
              change: { add: delta.add, remove: delta.remove },
            },
          })
        },
        onProgress: (done, total) => setProgress({ action, tag, done, total }),
      }
      try {
        const result = await operation(deps)
        if (nextAppearance) {
          await appearanceMutation.mutateAsync(nextAppearance)
        }
        void queryClient.invalidateQueries()
        if (result.failures.length > 0) {
          toast.error(
            `${result.failures.length} of ${result.total} messages couldn't be updated. ` +
              `Run it again — the operation is idempotent and re-running converges.`,
          )
        }
        return result
      } finally {
        setProgress(null)
      }
    },
    [appearanceMutation, client, enumerateBatch, queryClient],
  )

  const rename = useCallback(
    (oldName: string, newName: string, configured: readonly TagAppearance[]) =>
      run(
        'rename',
        oldName,
        (deps) => renameTagAcrossCarriers(oldName, newName, deps),
        migrateTagAppearance(configured, oldName, newName),
      ),
    [run],
  )

  const remove = useCallback(
    (name: string, configured: readonly TagAppearance[]) =>
      run(
        'delete',
        name,
        (deps) => deleteTagAcrossCarriers(name, deps),
        dropTagAppearance(configured, name),
      ),
    [run],
  )

  return { rename, remove, progress, isRunning: progress !== null }
}
