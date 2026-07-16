/**
 * Notification policy — the wire shape of `[notifications]` in `app.toml`.
 * TOML is the source of truth; OS-level delivery permission stays device-local.
 *
 * @spec docs/eph/RFC-L2-configuration-matrix
 */
export interface Notifications {
  newMail?: boolean | null
  sound?: boolean | null
}
