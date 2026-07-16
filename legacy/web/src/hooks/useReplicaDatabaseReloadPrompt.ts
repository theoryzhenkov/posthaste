/**
 * Multi-tab IndexedDB schema-version nudge (W1 / N19).
 *
 * `replicaDatabase.ts` closes this tab's connection the moment another
 * tab/context starts a schema upgrade (`onversionchange`), and reports when
 * this tab's own `open()` is stuck waiting on a blocker (`onblocked`). Either
 * way the tab is left unable to durably write until it reloads — surface that
 * with a persistent, explicit "Reload" prompt rather than letting mutations
 * fail silently later.
 */
import { useEffect, useRef } from 'react'
import { toast } from 'sonner'

import { onReplicaDatabaseNotice } from '@/runtime/replica/replicaDatabase'

export function useReplicaDatabaseReloadPrompt(): void {
  const shownRef = useRef(false)

  useEffect(() => {
    return onReplicaDatabaseNotice((notice) => {
      // Only the first notice prompts — once the toast is up, a second
      // 'blocked'/'outdated' notice (e.g. from another store on the same
      // connection) doesn't need to stack another one.
      if (shownRef.current) {
        return
      }
      shownRef.current = true
      toast(
        notice === 'outdated'
          ? 'Posthaste updated in another tab.'
          : 'Posthaste is waiting on another tab to finish updating.',
        {
          description: 'Reload this tab to keep saving changes.',
          duration: Infinity,
          action: {
            label: 'Reload',
            onClick: () => window.location.reload(),
          },
        },
      )
    })
  }, [])
}
