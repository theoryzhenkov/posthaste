/**
 * Global tag-maintenance mechanics: rename and delete a keyword-derived tag
 * across every message that carries it.
 *
 * Tags have no registry — they exist only as message keywords — so a rename or
 * delete is a bulk re-keyword over the tag's carriers, enumerated through the
 * ordinary `tag:<name>` search surface. The functions here are the pure
 * orchestration (carrier enumeration, per-message mutation, appearance
 * migration); {@link ./useTagMaintenance} injects the real runtime deps.
 *
 * NOTE: this pages the search index and issues one mutation per carrier. If tag
 * cardinality ever makes that too slow, a server-side bulk `Tag/rename` /
 * `Tag/delete` operation is the residual — swap the pool worker for one call.
 *
 * @spec docs/eph/DESIGN-L2-appearance-toml
 */
import type { TagAppearance } from '@/api/types'
import type { MessagePageClient } from '@/messagePageClient'
import { createOperationContext } from '@/observability'

import { tagFilterQuery } from '../tags/tagQuery'

export interface TagCarrier {
  sourceId: string
  messageId: string
}

export interface KeywordDelta {
  add: string[]
  remove: string[]
}

export interface TagMaintenanceDeps {
  /** Enumerate every message carrying the tag (all pages). */
  enumerateCarriers: (tag: string) => Promise<TagCarrier[]>
  /** Apply one `setKeywords` command to a single message. */
  applyKeywords: (carrier: TagCarrier, delta: KeywordDelta) => Promise<void>
  /** Reported after each carrier settles (success or failure). */
  onProgress?: (done: number, total: number) => void
  /** Max carriers mutated concurrently; defaults to {@link DEFAULT_CONCURRENCY}. */
  concurrency?: number
}

export interface TagBulkResult {
  total: number
  /** Carriers whose mutation failed. The op is idempotent — re-running converges. */
  failures: TagCarrier[]
}

/** How a proposed rename resolves against the set of existing tag names. */
export type RenameKind = 'noop' | 'rename' | 'merge'

const DEFAULT_CONCURRENCY = 4
/** Safety valve against a pathological cursor that never terminates. */
const MAX_PAGES = 1000

/**
 * Classify a rename: `noop` when the name is unchanged (case-insensitive),
 * `merge` when the destination already exists as a tag (memberships fold
 * together), otherwise a plain `rename`.
 */
export function classifyRename(
  oldName: string,
  newName: string,
  knownNames: readonly string[],
): RenameKind {
  const from = oldName.trim().toLowerCase()
  const to = newName.trim().toLowerCase()
  if (!to || from === to) return 'noop'
  const collides = knownNames.some(
    (name) =>
      name.trim().toLowerCase() === to && name.trim().toLowerCase() !== from,
  )
  return collides ? 'merge' : 'rename'
}

/**
 * Rename a tag across all its carriers. Per carrier the new keyword is ADDED
 * first and only then is the old one REMOVED, so an interruption may leave both
 * tags but can never drop the message from the tag entirely.
 */
export async function renameTagAcrossCarriers(
  oldName: string,
  newName: string,
  deps: TagMaintenanceDeps,
): Promise<TagBulkResult> {
  const carriers = await deps.enumerateCarriers(oldName)
  return runCarrierPool(carriers, deps, async (carrier) => {
    await deps.applyKeywords(carrier, { add: [newName], remove: [] })
    await deps.applyKeywords(carrier, { add: [], remove: [oldName] })
  })
}

/** Delete a tag by stripping the keyword from every carrier. */
export async function deleteTagAcrossCarriers(
  name: string,
  deps: TagMaintenanceDeps,
): Promise<TagBulkResult> {
  const carriers = await deps.enumerateCarriers(name)
  return runCarrierPool(carriers, deps, (carrier) =>
    deps.applyKeywords(carrier, { add: [], remove: [name] }),
  )
}

async function runCarrierPool(
  carriers: TagCarrier[],
  deps: TagMaintenanceDeps,
  worker: (carrier: TagCarrier) => Promise<void>,
): Promise<TagBulkResult> {
  const total = carriers.length
  const failures: TagCarrier[] = []
  const limit = Math.max(1, deps.concurrency ?? DEFAULT_CONCURRENCY)
  let index = 0
  let done = 0

  async function pump(): Promise<void> {
    while (index < total) {
      const carrier = carriers[index++]
      try {
        await worker(carrier)
      } catch {
        failures.push(carrier)
      }
      done += 1
      deps.onProgress?.(done, total)
    }
  }

  await Promise.all(
    Array.from({ length: Math.min(limit, total) }, () => pump()),
  )
  return { total, failures }
}

/**
 * Next `settings.tags` after a rename. The appearance model is keyed by name,
 * so a plain rename TRANSFERS the entry to the new name. A merge keeps the
 * destination tag's own appearance and drops the source entry. Returns `null`
 * when nothing changes (no source entry to migrate).
 */
export function migrateTagAppearance(
  configured: readonly TagAppearance[],
  oldName: string,
  newName: string,
): TagAppearance[] | null {
  const oldEntry = configured.find((entry) => entry.name === oldName)
  if (!oldEntry) return null
  const destinationHasEntry = configured.some((entry) => entry.name === newName)
  if (destinationHasEntry) {
    return configured.filter((entry) => entry.name !== oldName)
  }
  return configured.map((entry) =>
    entry.name === oldName ? { ...entry, name: newName } : entry,
  )
}

/** Next `settings.tags` after a delete, or `null` when the tag had no entry. */
export function dropTagAppearance(
  configured: readonly TagAppearance[],
  name: string,
): TagAppearance[] | null {
  if (!configured.some((entry) => entry.name === name)) return null
  return configured.filter((entry) => entry.name !== name)
}

/**
 * Enumerate every carrier of a tag by paging the `tag:<name>` search surface.
 * Dedupes by `(sourceId, messageId)` since a message may surface in more than
 * one page under concurrent mutation.
 */
export async function fetchTagCarriers(
  tag: string,
  fetchPage: MessagePageClient['fetchPage'],
  pageLimit = 100,
): Promise<TagCarrier[]> {
  const carriers: TagCarrier[] = []
  const seen = new Set<string>()
  let cursor: string | null | undefined = null
  let pages = 0
  do {
    const page = await fetchPage({
      scope: { kind: 'global' },
      query: tagFilterQuery(tag),
      cursor,
      limit: pageLimit,
      sort: 'date',
      sortDir: 'desc',
      operation: createOperationContext(
        'tag.maintenance.enumerate',
        'settings-tags',
      ),
    })
    for (const message of page.items) {
      const key = `${message.sourceId}:${message.id}`
      if (seen.has(key)) continue
      seen.add(key)
      carriers.push({ sourceId: message.sourceId, messageId: message.id })
    }
    cursor = page.nextCursor
    pages += 1
  } while (cursor && pages < MAX_PAGES)
  return carriers
}
