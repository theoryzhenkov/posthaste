/**
 * The randomness seam (R8, docs/client/L2-charter.md): every random draw
 * routes through here, so tests fake randomness by mocking this module. The
 * eslint ratchet bans `Math.random()` and `crypto.randomUUID()` everywhere
 * else; `newId` is the ONE id mint (R0) — the per-module uuid/`Date.now`
 * fallbacks consolidated onto it.
 */
import { nowMs } from './time'

const ULID_ALPHABET = '0123456789ABCDEFGHJKMNPQRSTVWXYZ'

/** A ULID-shaped id: 48-bit millisecond timestamp + 80 random bits, Crockford
 * base32. Sortable by mint time; used for command idempotency ids,
 * client-minted draft/notification ids, and local entity ids. */
export function newId(): string {
  let ts = nowMs()
  const time: string[] = []
  for (let i = 0; i < 10; i++) {
    time.unshift(ULID_ALPHABET[ts % 32]!)
    ts = Math.floor(ts / 32)
  }
  const rand = new Uint8Array(16)
  if (typeof crypto !== 'undefined' && crypto.getRandomValues) {
    crypto.getRandomValues(rand)
  } else {
    for (let i = 0; i < rand.length; i++)
      rand[i] = Math.floor(Math.random() * 256)
  }
  let out = time.join('')
  for (let i = 0; i < 16; i++) out += ULID_ALPHABET[rand[i]! % 32]!
  return out
}

/** A uniform random integer in [0, boundExclusive). */
export function randomInt(boundExclusive: number): number {
  return Math.floor(Math.random() * boundExclusive)
}
