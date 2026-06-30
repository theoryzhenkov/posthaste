import type { ReleaseEntry } from './types'

/**
 * Rank a tag as [major, minor, patch, nightly] so newest sorts first. A stable
 * release ranks above its own nightlies (nightly = +Infinity), and higher
 * nightly serials rank above lower ones on the same base version.
 */
export function releaseRank(tag: string): number[] {
  const m = tag
    .replace(/^v/i, '')
    .match(/^(\d+)\.(\d+)\.(\d+)(?:-nightly\.(\d+))?/i)
  if (!m) return [0, 0, 0, 0]
  return [
    Number(m[1]),
    Number(m[2]),
    Number(m[3]),
    m[4] ? Number(m[4]) : Number.POSITIVE_INFINITY,
  ]
}

/** Descending release order: newest release first. */
export function compareReleasesDesc(a: ReleaseEntry, b: ReleaseEntry): number {
  const ra = releaseRank(a.tag)
  const rb = releaseRank(b.tag)
  for (let i = 0; i < 4; i += 1) {
    if (rb[i] !== ra[i]) return rb[i] - ra[i]
  }
  return 0
}
