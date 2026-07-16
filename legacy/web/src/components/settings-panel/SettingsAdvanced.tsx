import { useState, type ReactNode } from 'react'
import { ChevronRight } from 'lucide-react'

import { cn } from '../../lib/utils'

/**
 * Progressive-disclosure wrapper: hides advanced / rarely-used settings behind
 * an "Advanced ▸" toggle so each pane leads with the basics. RFC §5 — one
 * disclosure per pane, basic-first. Dependency-free (internal state + chevron).
 *
 * @spec docs/eph/RFC-L2-configuration-surface.md#5
 */
export function SettingsAdvanced({
  label = 'Advanced',
  children,
}: {
  label?: string
  children: ReactNode
}) {
  const [open, setOpen] = useState(false)
  return (
    <div className="border-t border-border/60 pt-3">
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
        className="group flex w-full items-center gap-1.5 text-left text-[12px] font-medium text-muted-foreground transition-colors hover:text-foreground"
      >
        <ChevronRight
          size={13}
          strokeWidth={1.8}
          className={cn(
            'shrink-0 transition-transform duration-150',
            open && 'rotate-90',
          )}
        />
        {label}
      </button>
      {open ? <div className="mt-3">{children}</div> : null}
    </div>
  )
}
