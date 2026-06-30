/**
 * The single owner of the mail surface's keydown listener.
 *
 * Replaces the former pair of global listeners (app shortcuts + message-list
 * navigation) with one dispatcher that tracks the focused pane and routes
 * within-pane keys (`j`/`k`) to whichever pane registered a handler. Panes opt
 * in with `useFocusedPaneHandler`; cross-cutting keys are handled by
 * {@link dispatchMailKey}.
 *
 * @spec docs/L0-ui#navigation-model
 * @spec docs/L1-ui#keyboard-shortcuts
 */
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react'

import { KeyboardContext, type KeyboardContextValue } from './context'
import {
  dispatchMailKey,
  PANE_ORDER,
  type KeyboardDispatchContext,
  type PaneId,
  type PaneKeyHandler,
} from './dispatch'
import type { GotoPrefix, GotoRole } from './goto'

/** How long a half-typed goto prefix (`g`, `gq`) waits for its next key. */
const PREFIX_TIMEOUT_MS = 1500

/** Callbacks + flags the dispatcher needs; mirrors the prior shortcut hooks. */
export interface KeyboardControllerProps {
  effectiveSurfaceOpen: boolean
  overlayOwnsInput: boolean
  isMessageDetailOpen: boolean
  hasSelectedMessage: boolean
  hasSearchQuery: boolean
  onOpenCommandPalette: () => void
  onOpenSettings: () => void
  onCompose: () => void
  onReply: () => void
  onReplyAll: () => void
  onToggleFlag: () => void
  onUndo: () => void
  onRedo: () => void
  onArchive: () => void
  onTrash: () => void
  onOpenTagEditor: () => void
  onOpenFocusedMessage: () => void
  onClearSelectedMessage: () => void
  onClearSearchQuery: () => void
  onToggleShortcuts: () => void
  onGoto: (role: GotoRole, options: { forceSmart: boolean }) => void
  children: ReactNode
}

export function KeyboardController({
  children,
  ...props
}: KeyboardControllerProps) {
  const [requestedPane, setRequestedPane] = useState<PaneId>('list')
  const handlersRef = useRef(new Map<PaneId, PaneKeyHandler>())

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

  const availablePanes = useMemo(
    () =>
      PANE_ORDER.filter(
        (pane) => pane !== 'detail' || props.isMessageDetailOpen,
      ),
    [props.isMessageDetailOpen],
  )

  // Derive the effective pane so a closed detail pane never strands focus,
  // without a setState-in-effect round trip.
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
        resolvePaneHandler: (target) =>
          handlersRef.current.get(target) ??
          (target === 'detail' ? handlersRef.current.get('list') : undefined),
        pendingPrefix: pendingPrefixRef.current,
        setPendingPrefix,
        onGoto: p.onGoto,
        onOpenCommandPalette: p.onOpenCommandPalette,
        onOpenSettings: p.onOpenSettings,
        onCompose: p.onCompose,
        onReply: p.onReply,
        onReplyAll: p.onReplyAll,
        onToggleFlag: p.onToggleFlag,
        onUndo: p.onUndo,
        onRedo: p.onRedo,
        onArchive: p.onArchive,
        onTrash: p.onTrash,
        onOpenTagEditor: p.onOpenTagEditor,
        onOpenFocusedMessage: p.onOpenFocusedMessage,
        onClearSelectedMessage: p.onClearSelectedMessage,
        onClearSearchQuery: p.onClearSearchQuery,
        onToggleShortcuts: p.onToggleShortcuts,
      }
      dispatchMailKey(event, ctx)
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [setPendingPrefix])

  const value = useMemo<KeyboardContextValue>(
    () => ({ activePane, focusPane, registerPaneHandler }),
    [activePane, focusPane, registerPaneHandler],
  )

  return (
    <KeyboardContext.Provider value={value}>
      {children}
    </KeyboardContext.Provider>
  )
}
