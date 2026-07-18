/**
 * Keyboard shortcut reference overlay, toggled with `?`.
 *
 * The action rows are generated from the action registry (their chords are the
 * exact data the keyboard tier dispatches), so the list can no longer drift from
 * behavior. Native, non-action keys stay a small static list. See
 * ./shortcutReferenceRows.
 */
import { Keyboard } from 'lucide-react'

import { FloatingPanel } from '@/components/floating/FloatingPanel'
import { shortcutReferenceRows } from './shortcutReferenceRows'

interface ShortcutReferenceProps {
  onClose: () => void
}

const SHORTCUTS = shortcutReferenceRows()

export function ShortcutReference({ onClose }: ShortcutReferenceProps) {
  return (
    <FloatingPanel
      panelLabel="keyboard shortcuts"
      storageKey="posthaste.shortcuts.panelOffset"
      closeIgnoreSelector="[data-shortcut-reference-trigger='true']"
      className="max-w-sm"
      header={
        <div className="flex h-12 min-w-0 items-center gap-2 px-3">
          <Keyboard
            size={15}
            strokeWidth={1.7}
            className="shrink-0 text-muted-foreground"
          />
          <span className="truncate text-sm font-semibold">
            Keyboard shortcuts
          </span>
        </div>
      }
      onClose={onClose}
    >
      <div className="p-5">
        <div className="space-y-2.5">
          {SHORTCUTS.map((shortcut) => (
            <div
              key={shortcut.action}
              className="flex items-center gap-4 text-sm"
            >
              <span className="min-w-0 flex-1 text-muted-foreground">
                {shortcut.action}
              </span>
              <div className="flex shrink-0 items-center gap-1.5">
                {shortcut.keys.map((key, index) => (
                  <span key={key}>
                    {index > 0 && (
                      <span className="mr-1.5 text-xs text-muted-foreground/60">
                        /
                      </span>
                    )}
                    <kbd className="rounded border border-border bg-[var(--bg-elev)] px-1.5 py-0.5 font-mono text-xs text-foreground">
                      {key}
                    </kbd>
                  </span>
                ))}
              </div>
            </div>
          ))}
        </div>
      </div>
    </FloatingPanel>
  )
}
