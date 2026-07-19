import { useEffect, useId, useRef } from 'react'
import type { KeyboardEvent, ReactNode } from 'react'

/**
 * A confirmation scoped to the surface it affects: scrim + dialog rendered
 * in place, inside the surface's own positioned container, instead of being
 * portaled over the whole window. Only the surface is scrimmed — the rest of
 * the app stays fully interactive, and the surface's chrome outside the
 * scrim (e.g. a floating panel's header) keeps working.
 *
 * While open, focus is contained in the dialog: it takes focus on mount and
 * reclaims it if it leaves, Tab/Shift+Tab cycle the dialog's controls, and
 * Escape (or a scrim click) resolves the prompt via `onDismiss`. Escape is
 * consumed with preventDefault + stopPropagation so it never reaches the
 * surface's own dismissal (which skips defaultPrevented events) or the
 * global keyboard dispatcher.
 */
export function SurfaceConfirm({
  open,
  title,
  description,
  onDismiss,
  children,
}: {
  open: boolean
  title: string
  description: string
  /** Resolve the prompt without acting (Escape / scrim click). */
  onDismiss: () => void
  /** The action row: buttons resolving the prompt. */
  children: ReactNode
}) {
  const dialogRef = useRef<HTMLDivElement | null>(null)
  const titleId = useId()
  const descriptionId = useId()

  // Take focus on open, hand it back to the surface's previously focused
  // element on close/unmount.
  useEffect(() => {
    if (!open) {
      return
    }
    const previouslyFocused =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null
    focusablesIn(dialogRef.current)[0]?.focus()
    return () => previouslyFocused?.focus()
  }, [open])

  if (!open) {
    return null
  }

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      onDismiss()
      return
    }
    if (event.key === 'Tab') {
      const focusables = focusablesIn(dialogRef.current)
      if (focusables.length === 0) {
        return
      }
      const index = focusables.indexOf(
        document.activeElement as HTMLElement,
      )
      const next =
        (index + (event.shiftKey ? -1 : 1) + focusables.length) %
        focusables.length
      event.preventDefault()
      focusables[next]?.focus()
    }
  }

  return (
    <div
      className="absolute inset-0 z-30"
      onKeyDown={handleKeyDown}
      onBlur={(event) => {
        // Containment: focus that leaves the dialog comes back to it. The
        // pointer interaction that pulled it away has already landed.
        const next = event.relatedTarget
        if (!(next instanceof Node) || !dialogRef.current?.contains(next)) {
          focusablesIn(dialogRef.current)[0]?.focus()
        }
      }}
    >
      <div
        role="presentation"
        className="absolute inset-0 bg-black/10 supports-backdrop-filter:backdrop-blur-xs"
        onClick={onDismiss}
      />
      <div
        ref={dialogRef}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={descriptionId}
        className="absolute top-1/2 left-1/2 grid w-full max-w-xs -translate-x-1/2 -translate-y-1/2 gap-4 rounded-xl bg-popover p-4 pb-0 text-popover-foreground ring-1 ring-foreground/10"
      >
        <div className="grid gap-1.5 text-center">
          <h2 id={titleId} className="font-heading text-base font-medium">
            {title}
          </h2>
          <p
            id={descriptionId}
            className="text-sm text-balance text-muted-foreground"
          >
            {description}
          </p>
        </div>
        <div className="-mx-4 grid grid-cols-2 gap-2 rounded-b-xl border-t bg-muted/50 p-4">
          {children}
        </div>
      </div>
    </div>
  )
}

const FOCUSABLE_SELECTOR =
  'button:not([disabled]), [href], input:not([disabled]), select, textarea, [tabindex]:not([tabindex="-1"])'

function focusablesIn(root: HTMLElement | null): HTMLElement[] {
  if (!root) {
    return []
  }
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR))
}
