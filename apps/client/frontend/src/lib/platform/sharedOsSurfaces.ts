/**
 * Ownership of the app's SHARED OS SURFACES — the Dock/taskbar badge and the
 * new-mail banners. Both belong to the process, not to a window: the badge is
 * one counter on one app icon, and a banner posted by two windows is two
 * banners. Exactly one window may drive them.
 *
 * Window IDENTITY is the wrong test for that, and was the one used. "The main
 * window owns it" leaves NOBODY owning it the moment the main window is closed
 * while a surface window stays open, and it cannot be asked at all in the
 * browser build, where surfaces are `window.open` popups and every one of them
 * ran the OS effects. Identity answers "which window is this"; what the bridges
 * need answered is "may this window drive the shared surfaces", and the two
 * stop agreeing as soon as a window closes.
 *
 * So ownership is a CLAIM, not an inference. Windows race for one leased slot
 * in shared storage; the holder renews it while it lives; a lease nobody renews
 * expires and the next window takes it. First claimant keeps winning, so the
 * window that booted first — the main one — holds it in practice, and a surface
 * window inherits it only once the holder is gone.
 *
 * What this guarantees:
 *
 *  - In steady state, one holder. A claim is only taken over an EXPIRED lease,
 *    and the holder renews at well under the lease length.
 *  - Recovery. If the holder stops renewing — closed, crashed, or its webview
 *    frozen — another window owns the surfaces within roughly lease + renew.
 *    Nothing depends on an unload handler running, which for a closing window
 *    is exactly what cannot be relied on.
 *
 * What it does NOT guarantee:
 *
 *  - Mutual exclusion under a simultaneous race. Web storage has no
 *    compare-and-swap; two windows that find the same lease expired in the same
 *    instant can both write. The read-back below narrows that window, it does
 *    not close it. The cost is bounded — one duplicated banner, one redundant
 *    badge push — and the next renewal converges on a single holder.
 *  - Coverage during handover. For up to lease + renew after a holder dies
 *    nobody owns the surfaces, so banners in that gap are dropped and the badge
 *    is stale. Better than the failure it replaces, which had no recovery at
 *    all.
 *  - Anything when windows do not share storage. This assumes every window of
 *    the app reads one localStorage. True by construction for browser popups;
 *    for desktop webviews it follows from their sharing an origin and a data
 *    store, which is an inference and is not exercised by any test here.
 *  - Ownership stability under throttling. A backgrounded window whose timers
 *    are throttled past the lease can lose the claim and take it back later.
 *    The effects are edge-triggered and idempotent, so a flap is churn, not
 *    corruption.
 */
import { useEffect, useState } from 'react'

import { newId } from '@/lib/ambient/random'
import {
  ambientStorage,
  readStorageItem,
  writeStorageItem,
  type StorageLike,
} from '@/lib/ambient/storage'
import { nowMs } from '@/lib/ambient/time'

const CLAIM_KEY = 'posthaste.osSurfaces.claim.v1'

/** How long a claim survives with no renewal. */
const LEASE_MS = 5_000

/** How often the holder renews — and every other window re-checks whether the
 *  slot has fallen vacant. Well under the lease so a renewal has to miss
 *  twice before a peer may take over. */
const RENEW_MS = 2_000

interface ClaimRecord {
  holder: string
  expiresAt: number
}

/**
 * The live claim, or `null` when the slot is takeable. Parse, don't validate
 * (R3): absent, unparseable, misshapen and expired all read as vacant, so a
 * corrupt record can never lock every window out of the OS surfaces forever.
 */
function readClaim(storage: StorageLike, at: number): ClaimRecord | null {
  const raw = readStorageItem(CLAIM_KEY, storage)
  if (raw === null) {
    return null
  }
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    return null
  }
  if (typeof parsed !== 'object' || parsed === null) {
    return null
  }
  const { holder, expiresAt } = parsed as Record<string, unknown>
  if (typeof holder !== 'string' || typeof expiresAt !== 'number') {
    return null
  }
  return expiresAt <= at ? null : { holder, expiresAt }
}

export interface SharedOsSurfaceClaim {
  /** Take or renew the claim; returns whether this holder owns the shared OS
   *  surfaces as of now. Call on an interval — it is the renewal too. */
  poll(): boolean
  /** Hand the claim back on a clean teardown so a peer need not wait out the
   *  lease. Expiry covers every unclean one. */
  release(): void
}

export interface SharedOsSurfaceClaimOptions {
  holderId?: string
  storage?: StorageLike | null
  now?: () => number
  leaseMs?: number
}

export function createSharedOsSurfaceClaim({
  holderId = newId(),
  storage = ambientStorage(),
  now = nowMs,
  leaseMs = LEASE_MS,
}: SharedOsSurfaceClaimOptions = {}): SharedOsSurfaceClaim {
  if (storage === null) {
    // Without shared storage there is nothing to coordinate through. A single
    // window is the overwhelming case (storage blocked, or a host without it),
    // and "nobody ever notifies" is a worse outcome than "two windows might",
    // so an uncoordinated window owns the surfaces.
    return { poll: () => true, release: () => {} }
  }

  const write = (expiresAt: number) =>
    writeStorageItem(
      CLAIM_KEY,
      JSON.stringify({ holder: holderId, expiresAt } satisfies ClaimRecord),
      storage,
    )

  return {
    poll() {
      const at = now()
      const held = readClaim(storage, at)
      if (held !== null && held.holder !== holderId) {
        return false
      }
      write(at + leaseMs)
      // Read back what landed. Storage offers no compare-and-swap, so a peer
      // writing in the same instant would have overwritten us and must not be
      // left with two windows both believing they hold the claim.
      return readClaim(storage, at)?.holder === holderId
    },
    release() {
      if (readClaim(storage, now())?.holder === holderId) {
        write(0)
      }
    },
  }
}

/**
 * Whether THIS window may drive the shared OS surfaces. Starts `false` and
 * flips on the first poll, so a bridge gated on it mounts a tick late rather
 * than racing the claim.
 */
export function useOwnsSharedOsSurfaces(): boolean {
  const [owns, setOwns] = useState(false)
  useEffect(() => {
    const claim = createSharedOsSurfaceClaim()
    setOwns(claim.poll())
    const timer = setInterval(() => setOwns(claim.poll()), RENEW_MS)
    return () => {
      clearInterval(timer)
      claim.release()
    }
  }, [])
  return owns
}
