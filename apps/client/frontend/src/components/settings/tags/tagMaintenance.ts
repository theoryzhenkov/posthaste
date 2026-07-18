/**
 * Global tag-maintenance mechanics: rename and delete a keyword-derived tag
 * across every message that carries it.
 *
 * Tags have no registry — they exist only as message keywords — so a rename or
 * delete is a bulk re-keyword over the tag's carriers. Carriers are enumerated
 * in bounded batches (the `automationRulePreview` query evaluates a
 * `keyword = <name>` condition and returns a capped sample plus the total);
 * each processed batch shrinks the match set, so the loop re-enumerates until
 * nothing new matches. The functions here are the pure orchestration
 * (batch loop, per-message mutation, appearance migration);
 * {@link ./useTagMaintenance} injects the real backend deps.
 *
 * NOTE: this issues one mutation per carrier. If tag cardinality ever makes
 * that too slow, a server-side bulk `Tag/rename` / `Tag/delete` command is the
 * residual — swap the pool worker for one call.
 */
import type { TagAppearance } from '@/gen'

export interface TagCarrier {
  sourceId: string
  messageId: string
}

export interface KeywordDelta {
  add: string[]
  remove: string[]
}

/** One enumeration round: a bounded batch of carriers plus the total match
 *  count at enumeration time (drives progress reporting). */
export interface TagCarrierBatch {
  carriers: TagCarrier[]
  total: number
}

export interface TagMaintenanceDeps {
  /** Enumerate a bounded batch of messages still carrying the tag. */
  enumerateBatch: (tag: string) => Promise<TagCarrierBatch>
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
/** Safety valve against a pathological enumeration that never terminates. */
const MAX_BATCHES = 1000

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
  return processTagCarriers(oldName, deps, async (carrier) => {
    await deps.applyKeywords(carrier, { add: [newName], remove: [] })
    await deps.applyKeywords(carrier, { add: [], remove: [oldName] })
  })
}

/** Delete a tag by stripping the keyword from every carrier. */
export async function deleteTagAcrossCarriers(
  name: string,
  deps: TagMaintenanceDeps,
): Promise<TagBulkResult> {
  return processTagCarriers(name, deps, (carrier) =>
    deps.applyKeywords(carrier, { add: [], remove: [name] }),
  )
}

/**
 * The batch loop: enumerate a bounded batch of carriers, mutate each, then
 * re-enumerate — processed carriers stop matching, so the set shrinks until
 * only already-seen (failed or concurrently re-tagged) carriers remain.
 * Dedupes by `(sourceId, messageId)` so a failed carrier is attempted once.
 */
async function processTagCarriers(
  tag: string,
  deps: TagMaintenanceDeps,
  worker: (carrier: TagCarrier) => Promise<void>,
): Promise<TagBulkResult> {
  const seen = new Set<string>()
  const failures: TagCarrier[] = []
  let total = 0
  let done = 0
  let batches = 0

  while (batches < MAX_BATCHES) {
    const batch = await deps.enumerateBatch(tag)
    if (batches === 0) {
      total = batch.total
    }
    const fresh = batch.carriers.filter((carrier) => {
      const key = `${carrier.sourceId}:${carrier.messageId}`
      if (seen.has(key)) return false
      seen.add(key)
      return true
    })
    if (fresh.length === 0) break
    await runCarrierPool(fresh, deps, worker, {
      onSettled: (carrier, failed) => {
        if (failed) failures.push(carrier)
        done += 1
        deps.onProgress?.(done, Math.max(total, done))
      },
    })
    batches += 1
  }
  return { total: Math.max(total, done), failures }
}

async function runCarrierPool(
  carriers: TagCarrier[],
  deps: TagMaintenanceDeps,
  worker: (carrier: TagCarrier) => Promise<void>,
  hooks: { onSettled: (carrier: TagCarrier, failed: boolean) => void },
): Promise<void> {
  const limit = Math.max(1, deps.concurrency ?? DEFAULT_CONCURRENCY)
  let index = 0

  async function pump(): Promise<void> {
    while (index < carriers.length) {
      const carrier = carriers[index++]
      let failed = false
      try {
        await worker(carrier)
      } catch {
        failed = true
      }
      hooks.onSettled(carrier, failed)
    }
  }

  await Promise.all(
    Array.from({ length: Math.min(limit, carriers.length) }, () => pump()),
  )
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
