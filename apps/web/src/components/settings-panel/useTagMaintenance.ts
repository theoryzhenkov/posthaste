/**
 * Runtime wiring for global tag rename/delete. Injects the real carrier
 * enumeration (search paging) and per-message `setKeywords` mutation into the
 * pure {@link ./tagMaintenance} orchestration, tracks inline progress, migrates
 * the appearance overlay, and surfaces partial failures as a toast.
 *
 * All mutations go through `runtimeMutations` / the appearance mutation — never
 * a raw fetch.
 *
 * @spec docs/eph/DESIGN-L2-appearance-toml
 */
import { useCallback, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'

import type { MessageCommand, TagAppearance } from '@/api/types'
import { messagePageClient } from '@/messagePageClient'
import { queryKeys } from '@/queryKeys'
import { runtimeMutations } from '@/runtime/mutations'

import {
  deleteTagAcrossCarriers,
  dropTagAppearance,
  fetchTagCarriers,
  migrateTagAppearance,
  renameTagAcrossCarriers,
  type KeywordDelta,
  type TagBulkResult,
  type TagCarrier,
  type TagMaintenanceDeps,
} from './tagMaintenance'
import { useTagAppearanceMutation } from './useTagAppearanceMutation'

export interface TagMaintenanceProgress {
  action: 'rename' | 'delete'
  tag: string
  done: number
  total: number
}

async function applyKeywords(
  carrier: TagCarrier,
  delta: KeywordDelta,
): Promise<void> {
  await runtimeMutations.messages.command({
    command: {
      kind: 'setKeywords',
      add: delta.add,
      remove: delta.remove,
    } satisfies MessageCommand,
    messageId: carrier.messageId,
    sourceId: carrier.sourceId,
  })
}

export function useTagMaintenance() {
  const queryClient = useQueryClient()
  const appearanceMutation = useTagAppearanceMutation()
  const [progress, setProgress] = useState<TagMaintenanceProgress | null>(null)

  const invalidate = useCallback(async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: queryKeys.tags }),
      queryClient.invalidateQueries({ queryKey: queryKeys.mailNavigationRead }),
    ])
  }, [queryClient])

  const run = useCallback(
    async (
      action: 'rename' | 'delete',
      tag: string,
      operation: (deps: TagMaintenanceDeps) => Promise<TagBulkResult>,
      nextAppearance: TagAppearance[] | null,
    ) => {
      setProgress({ action, tag, done: 0, total: 0 })
      const deps: TagMaintenanceDeps = {
        enumerateCarriers: (name) =>
          fetchTagCarriers(name, messagePageClient.fetchPage),
        applyKeywords,
        onProgress: (done, total) => setProgress({ action, tag, done, total }),
      }
      try {
        const result = await operation(deps)
        if (nextAppearance) {
          await appearanceMutation.mutateAsync(nextAppearance)
        }
        await invalidate()
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
    [appearanceMutation, invalidate],
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
