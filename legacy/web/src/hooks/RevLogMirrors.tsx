import { useRevLogMirror } from './useRevLogMirror'

/**
 * Phase 2: mirror the server-authoritative `RevLog` view for EVERY enabled
 * account (a wrapper around {@link useRevLogMirror} — React forbids calling a
 * hook in a loop, so each account renders its own item component). This is what
 * makes undo/redo history converge cross-device per-account: each account's
 * partition reconciles with its own `RevLog` view. The global Ctrl+Z merges the
 * per-account partitions by `createdAt` (in the store).
 *
 * @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
 */
function RevLogMirrorItem({ accountId }: { accountId: string }) {
  useRevLogMirror(accountId)
  return null
}

export function RevLogMirrors({ accountIds }: { accountIds: string[] }) {
  return (
    <>
      {accountIds.map((id) => (
        <RevLogMirrorItem key={id} accountId={id} />
      ))}
    </>
  )
}
