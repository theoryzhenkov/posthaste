/**
 * Whether the renderer should force the account-setup (Settings → Accounts)
 * surface because the user has no accounts.
 *
 * This decision must be driven only by a **settled** accounts query —
 * `isSuccess && !isFetching` on the accounts query's own data — never by a
 * transient empty during a refetch or by another query's success. Deriving it
 * from churning cache state caused Settings to hijack the UI on every mutation:
 * a post-mutation invalidation could leave the accounts list momentarily empty
 * (or gate on the bootstrap query's success) and flip this true. Requiring a
 * settled query makes the only true case the genuine one — first run, or the
 * last account deleted.
 *
 * Per the renderer boundary (docs/client/L1): the renderer renders runtime-
 * served state and must not derive authority from churning cache. A fully
 * owned app-mode is the subsystem-3 follow-up; this gate is the minimal
 * defensive fix.
 */
export function shouldForceAccountSettings(input: {
  accounts: readonly unknown[]
  /** The accounts query has settled (success and not currently fetching). */
  accountsSettled: boolean
}): boolean {
  return input.accountsSettled && input.accounts.length === 0
}
