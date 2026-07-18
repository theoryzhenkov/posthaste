/**
 * Stream-connection banner: renders only while the event stream is down, from
 * the facade's connection status. While disconnected the queries keep showing
 * their last answers; reconnection refetches everything, so the banner is the
 * only "you may be stale" surface the shell needs.
 */
import { Loader2, WifiOff } from 'lucide-react'

import { useConnectionStatus } from '@/data'
import { Z } from '@/app/shell/layering'

export function ConnectionBanner() {
  const status = useConnectionStatus()
  if (status === 'connected') {
    return null
  }

  const stale = status === 'stale'
  return (
    <div
      role="status"
      style={{ zIndex: Z.TOAST }}
      className="pointer-events-none fixed inset-x-0 top-2 flex justify-center"
    >
      <div className="flex items-center gap-2 rounded-full border border-border bg-background/95 px-3 py-1.5 text-xs text-muted-foreground shadow-sm">
        {stale ? (
          <WifiOff size={13} aria-hidden />
        ) : (
          <Loader2 size={13} className="animate-spin" aria-hidden />
        )}
        {stale
          ? 'Connection lost — showing the last known state'
          : 'Reconnecting…'}
      </div>
    </div>
  )
}
