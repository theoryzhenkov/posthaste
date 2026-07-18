/**
 * The single owner of the mail surface's keydown listener.
 *
 * Replaces the former pair of global listeners (app shortcuts + message-list
 * navigation) with one dispatcher that tracks the focused pane and routes
 * within-pane keys (`j`/`k`) to whichever pane registered a handler. Panes opt
 * in with `useFocusedPaneHandler`; cross-cutting keys are handled by
 * {@link dispatchMailKey}.
 *
 */
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react'

import {
  firstMatchingChord,
  resolveKeyboardAction,
  runResolvedWithConfirm,
  type ActionConfirm,
  type ActionContext,
  type ActionServices,
  type MessageTarget,
} from '@/commands'

import { KeyboardContext, type KeyboardContextValue } from '@/components/keyboard/context'
import type { PaneId } from '@/domain/vocabulary'

import {
  dispatchMailKey,
  PANE_ORDER,
  type KeyboardDispatchContext,
  type PaneKeyHandler,
} from './dispatch'
import { KeyboardConfirmDialog } from '@/components/keyboard/KeyboardConfirmDialog'
import type { GotoPrefix, GotoRole } from '../goto/goto'

/** How long a half-typed goto prefix (`g`, `gq`) waits for its next key. */
const PREFIX_TIMEOUT_MS = 1500

/** Callbacks + flags the dispatcher needs; mirrors the prior shortcut hooks. */
export interface KeyboardControllerProps {
  effectiveSurfaceOpen: boolean
  overlayOwnsInput: boolean
  hasSelectedMessage: boolean
  hasSearchQuery: boolean
  onOpenCommandPalette: () => void
  onUndo: () => void
  onRedo: () => void
  onClearSelectedMessage: () => void
  onClearSearchQuery: () => void
  /** A PARAMETERIZED action's chord (e.g. `m` → move-to-mailbox) can't run
   *  bare — this opens the command palette in that action's pick-step. */
  onOpenActionPicker: (actionId: string) => void
  onGoto: (role: GotoRole, options: { forceSmart: boolean }) => void
  onGotoConversation: () => void
  /** Role of the current view — gates the contextual keyboard action tier
   *  (e.g. `#` ⇒ delete-permanently only in Trash). */
  viewRole: string | null
  /** The focused message as a resolver target, or `null` when none is selected.
   *  Built by MailClient from the selected message's detail. */
  keyboardTarget: MessageTarget | null
  /** Domain + app handler bundle the resolved actions delegate to. */
  actionServices: ActionServices
  children: ReactNode
}

export function KeyboardController({
  children,
  ...props
}: KeyboardControllerProps) {
  const [requestedPane, setRequestedPane] = useState<PaneId>('list')
  const handlersRef = useRef(new Map<PaneId, PaneKeyHandler>())

  // A keyboard-invoked destructive action (delete-permanently) parks its runner
  // here and renders a confirm dialog — a keystroke must never silently perform
  // an irreversible delete.
  const [pendingConfirm, setPendingConfirm] = useState<{
    confirm: ActionConfirm
    onConfirm: () => void
  } | null>(null)
  const requestConfirm = useCallback(
    (confirm: ActionConfirm, onConfirm: () => void) =>
      setPendingConfirm({ confirm, onConfirm }),
    [],
  )

  // The goto prefix lives in a ref (not state) so it is read/written
  // synchronously within a keydown — a fast `gi` must not race a re-render — and
  // self-clears after a pause so a stray `g` never sticks.
  const pendingPrefixRef = useRef<GotoPrefix>(null)
  const prefixTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const setPendingPrefix = useCallback((prefix: GotoPrefix) => {
    pendingPrefixRef.current = prefix
    if (prefixTimerRef.current !== null) clearTimeout(prefixTimerRef.current)
    prefixTimerRef.current =
      prefix === null
        ? null
        : setTimeout(() => {
            pendingPrefixRef.current = null
            prefixTimerRef.current = null
          }, PREFIX_TIMEOUT_MS)
  }, [])
  useEffect(
    () => () => {
      if (prefixTimerRef.current !== null) clearTimeout(prefixTimerRef.current)
    },
    [],
  )

  const availablePanes = PANE_ORDER

  // Guard against a stale requested pane (e.g. a persisted value) ever stranding
  // focus on a pane that isn't navigable.
  const activePane = availablePanes.includes(requestedPane)
    ? requestedPane
    : 'list'

  const focusPane = useCallback((pane: PaneId) => setRequestedPane(pane), [])

  const registerPaneHandler = useCallback(
    (pane: PaneId, handler: PaneKeyHandler) => {
      handlersRef.current.set(pane, handler)
      return () => {
        if (handlersRef.current.get(pane) === handler) {
          handlersRef.current.delete(pane)
        }
      }
    },
    [],
  )

  // The listener binds once; everything mutable is read through this ref so the
  // global handler is never torn down/re-added on each prop or focus change.
  const stateRef = useRef({ props, activePane, availablePanes })
  useEffect(() => {
    stateRef.current = { props, activePane, availablePanes }
  })

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      const {
        props: p,
        activePane: pane,
        availablePanes: panes,
      } = stateRef.current
      const ctx: KeyboardDispatchContext = {
        effectiveSurfaceOpen: p.effectiveSurfaceOpen,
        overlayOwnsInput: p.overlayOwnsInput,
        hasSelectedMessage: p.hasSelectedMessage,
        hasSearchQuery: p.hasSearchQuery,
        activePane: pane,
        availablePanes: panes,
        focusPane: setRequestedPane,
        resolvePaneHandler: (target) => handlersRef.current.get(target),
        pendingPrefix: pendingPrefixRef.current,
        setPendingPrefix,
        onGoto: p.onGoto,
        onGotoConversation: p.onGotoConversation,
        onOpenCommandPalette: p.onOpenCommandPalette,
        onUndo: p.onUndo,
        onRedo: p.onRedo,
        onClearSelectedMessage: p.onClearSelectedMessage,
        onClearSearchQuery: p.onClearSearchQuery,
        // The registry tier is rebuilt per keydown from the ref snapshot (never
        // render-scope state) so it never races focus/selection changes.
        registryHook: {
          match: (keyEvent) => {
            const keyboardCtx: ActionContext = {
              targets: p.keyboardTarget ? [p.keyboardTarget] : [],
              viewRole: p.viewRole,
              activePane: pane,
              surface: 'keyboard',
              inputOwner: 'mail',
              hasPendingMutation: p.actionServices.email?.isPending ?? false,
              connection: 'unknown',
            }
            // `includeDisabled`: a chord the table CLAIMS but cannot run right
            // now (e.g. ⌘R with no selection) must still swallow the event —
            // matching the legacy modifier-chord feel (and keeping ⌘R from the
            // browser); `run` below refuses the disabled resolution.
            const resolved = resolveKeyboardAction(
              keyEvent,
              keyboardCtx,
              p.actionServices,
              { includeDisabled: true },
            )
            if (!resolved) return null
            const chord = firstMatchingChord(resolved.def.shortcut, keyEvent)
            return {
              id: resolved.def.id,
              aboveOverlay: chord?.aboveOverlay ?? false,
              inEditable: chord?.inEditable ?? false,
              // Parameterized actions route to the palette pick-step (never a
              // silent no-op); destructive ones still gate on the confirm host.
              run: () => {
                if (!resolved.enabled) return
                runResolvedWithConfirm(resolved, requestConfirm, (r) =>
                  stateRef.current.props.onOpenActionPicker(r.def.id),
                )
              },
            }
          },
        },
      }
      dispatchMailKey(event, ctx)
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [requestConfirm, setPendingPrefix])

  const value = useMemo<KeyboardContextValue>(
    () => ({ activePane, focusPane, registerPaneHandler }),
    [activePane, focusPane, registerPaneHandler],
  )

  return (
    <KeyboardContext.Provider value={value}>
      {children}
      <KeyboardConfirmDialog
        confirm={pendingConfirm?.confirm ?? null}
        onConfirm={() => {
          pendingConfirm?.onConfirm()
          setPendingConfirm(null)
        }}
        onCancel={() => setPendingConfirm(null)}
      />
    </KeyboardContext.Provider>
  )
}
