import type { ReactNode } from 'react'

import { isMacDesktop } from '@/lib/platform/runtime'
import { cn } from '@/lib/design/cn'

// Shared macOS window chrome. On macOS every app window uses the inset/overlay
// title bar (the traffic-light "semaphore" is drawn inside the webview), so each
// window has to reserve the traffic-light zone and paint its own drag region.
// These primitives are the single source of truth for that, so the main shell
// and every separate surface window integrate the semaphore identically rather
// than re-implementing it per window. They render nothing off macOS desktop
// (other desktop OSes keep native decorations; the browser has none).
//
// Keep these values in sync with `traffic_light_position` in
// apps/desktop/src/lib.rs (currently LogicalPosition::new(14.0, 15.0)).
export const WINDOW_TITLEBAR_HEIGHT = 42
const WINDOW_TRAFFIC_LIGHT_INSET = 78

// Draggable spacer that reserves the traffic-light zone inside an existing top
// bar (e.g. the main toolbar). Renders nothing when no inset is needed.
export function TrafficLightInset({ className }: { className?: string }) {
  if (!isMacDesktop()) {
    return null
  }
  return (
    <div
      data-tauri-drag-region
      className={cn('shrink-0 self-stretch', className)}
      style={{ width: WINDOW_TRAFFIC_LIGHT_INSET }}
    />
  )
}

// Standalone draggable title bar for windows that do not otherwise have a top
// bar of their own (the separate surface windows). Reserves the traffic-light
// zone, provides the drag region, and optionally centers a window title.
export function WindowTitlebar({ title }: { title?: ReactNode }) {
  if (!isMacDesktop()) {
    return null
  }
  return (
    <div
      data-tauri-drag-region
      className="relative flex shrink-0 select-none items-center border-b border-border-soft bg-chrome text-chrome-foreground"
      style={{
        height: WINDOW_TITLEBAR_HEIGHT,
        paddingLeft: WINDOW_TRAFFIC_LIGHT_INSET,
        paddingRight: 12,
      }}
    >
      {title ? (
        <span className="pointer-events-none absolute inset-x-0 truncate px-20 text-center text-[13px] font-medium text-foreground/80">
          {title}
        </span>
      ) : null}
    </div>
  )
}
