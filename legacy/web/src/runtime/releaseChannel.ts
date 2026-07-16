/**
 * The release channel baked into the web bundle at build time.
 *
 * Set via `VITE_RELEASE_CHANNEL` at build (`nightly` | `stable`). Defaults to
 * `dev` for local/browser development. The desktop binary carries the same
 * channel as a compile-time constant exposed via the `release_channel` Tauri
 * command; the two are set from the same source in the release workflow so they
 * cannot silently disagree.
 *
 * @see docs/eph/DESIGN-L2-release-channels.md
 */
export type ReleaseChannel = 'nightly' | 'stable' | 'dev'

function isReleaseChannel(value: unknown): value is ReleaseChannel {
  return value === 'nightly' || value === 'stable' || value === 'dev'
}

const envValue = import.meta.env?.VITE_RELEASE_CHANNEL

/** The release channel this web bundle was built for. */
export const releaseChannel: ReleaseChannel = isReleaseChannel(envValue)
  ? envValue
  : 'dev'

/** Whether this build is a nightly/dogfood build. */
export const isNightly = releaseChannel === 'nightly'

/** Whether this build is a stable/release build. */
export const isStable = releaseChannel === 'stable'
