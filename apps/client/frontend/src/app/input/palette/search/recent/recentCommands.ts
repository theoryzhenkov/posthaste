/**
 * Persisted recency/frequency counter for palette commands.
 *
 * Persists a {@link DecayedCounter} keyed by stable action id
 * (`message.archive`, `app.compose`, …) in localStorage and bumps it on every
 * palette execution, so recently and frequently used commands float toward the
 * top of the ranker.
 *
 * A single decayed counter captures both signals: repeated use accumulates
 * value (frequency); the half-life decay discounts stale entries (recency). It
 * is fed to the ranker as `recentCommands`.
 *
 * Pure/localStorage-only — no React, no cross-window sync (a per-tab bias is
 * fine; correctness never depends on it).
 */
import { readStorageItem, writeStorageItem } from '@/lib/ambient/storage'
import { nowMs } from '@/lib/ambient/time'

import type { DecayedCounter } from '../types'

const STORAGE_KEY = 'posthaste.commandPalette.recents.v1'
const HALF_LIFE_MS = 7 * 24 * 60 * 60 * 1000 // one week

function emptyCounter(): DecayedCounter {
  return { halfLifeMs: HALF_LIFE_MS, entries: {} }
}

/** Decay a stored value to `now` using the counter half-life. */
function decayed(
  entry: { value: number; updatedAt: number },
  now: number,
  halfLifeMs: number,
): number {
  if (halfLifeMs <= 0) return entry.value
  const age = Math.max(0, now - entry.updatedAt)
  return entry.value * Math.pow(0.5, age / halfLifeMs)
}

/** Read the persisted counter, tolerating absent/corrupt storage. */
export function loadRecentCommands(): DecayedCounter {
  try {
    const raw = readStorageItem(STORAGE_KEY)
    if (!raw) return emptyCounter()
    const parsed = JSON.parse(raw) as Partial<DecayedCounter> | null
    if (!parsed || typeof parsed !== 'object' || !parsed.entries) {
      return emptyCounter()
    }
    return {
      halfLifeMs:
        typeof parsed.halfLifeMs === 'number'
          ? parsed.halfLifeMs
          : HALF_LIFE_MS,
      entries: parsed.entries,
    }
  } catch {
    return emptyCounter()
  }
}

/**
 * Bump the counter for `actionId` — decay the prior value to now, then add 1.
 * Called from the palette's execute path for registry (`kind: 'action'`) rows.
 */
export function recordCommandUse(actionId: string): void {
  const now = nowMs()
  const counter = loadRecentCommands()
  const previous = counter.entries[actionId]
  const base = previous ? decayed(previous, now, counter.halfLifeMs) : 0
  counter.entries[actionId] = { value: base + 1, updatedAt: now }
  // Absent/full storage no-ops inside the seam — recents are best-effort.
  writeStorageItem(STORAGE_KEY, JSON.stringify(counter))
}
