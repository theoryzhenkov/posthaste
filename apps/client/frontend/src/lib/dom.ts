/**
 * Named low-level DOM primitives (the R4 allowlist's second half).
 *
 * All input is a command (R4): components do not register their own global
 * listeners. The exceptions live HERE, as named primitives with a single
 * responsibility each — dismissal, measurement, and unload guarding — plus the
 * editable-target predicate the dispatchers share. Anything that looks like a
 * shortcut belongs in `commands/`, not here.
 */
import { useEffect } from 'react'

/** True when the event target is a text-editing element — the shared "user is
 *  typing" predicate keyboard dispatchers gate plain keys on. Duck-typed (not
 *  `instanceof HTMLElement`) so the pure dispatchers that call it stay
 *  unit-testable without a DOM. */
export function isEditableKeyboardTarget(target: EventTarget | null): boolean {
  const element = target as {
    tagName?: string
    isContentEditable?: boolean
  } | null
  return Boolean(
    element &&
      (element.tagName === 'INPUT' ||
        element.tagName === 'TEXTAREA' ||
        element.isContentEditable),
  )
}

/**
 * Overlay dismissal primitive: Escape dismisses the calling overlay (same
 * class as the floating panel's `usePanelDismissal`). Not a command: the
 * overlay owns input while mounted and its dismissal needs no availability
 * context.
 */
export function useEscapeToDismiss(onDismiss: () => void): void {
  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        event.preventDefault()
        onDismiss()
      }
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [onDismiss])
}

/**
 * Measurement primitive: re-run an anchored overlay's measurement on viewport
 * resize and any scroll (capture phase, so nested scroll containers count).
 * Non-input infra — the resize-observer class of listener.
 */
export function useViewportRemeasure(measure: () => void): void {
  useEffect(() => {
    window.addEventListener('resize', measure)
    window.addEventListener('scroll', measure, true)
    return () => {
      window.removeEventListener('resize', measure)
      window.removeEventListener('scroll', measure, true)
    }
  }, [measure])
}

/**
 * Data-loss guard: while `active`, a tab/app close prompts the browser's
 * native "leave site?" dialog. Non-input infra — there is no command to
 * route; the browser owns this interaction.
 */
export function useBeforeUnloadGuard(active: boolean): void {
  useEffect(() => {
    if (!active) return
    function handleBeforeUnload(event: BeforeUnloadEvent) {
      event.preventDefault()
      event.returnValue = ''
    }
    window.addEventListener('beforeunload', handleBeforeUnload)
    return () => window.removeEventListener('beforeunload', handleBeforeUnload)
  }, [active])
}

/**
 * Dismissal primitive for a transient popup that any further interaction
 * (press, keystroke, scroll) should close — the lightweight cousin of
 * `usePanelDismissal` for popups that claim no input of their own.
 */
export function useDismissOnGlobalInteraction(
  active: boolean,
  onDismiss: () => void,
): void {
  useEffect(() => {
    if (!active) return
    function dismiss() {
      onDismiss()
    }
    window.addEventListener('mousedown', dismiss)
    window.addEventListener('keydown', dismiss)
    window.addEventListener('scroll', dismiss, true)
    return () => {
      window.removeEventListener('mousedown', dismiss)
      window.removeEventListener('keydown', dismiss)
      window.removeEventListener('scroll', dismiss, true)
    }
  }, [active, onDismiss])
}
