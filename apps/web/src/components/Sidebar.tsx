/**
 * Left-pane sidebar with smart mailbox and source mailbox navigation.
 *
 * Renders normalized account, mailbox, and smart mailbox read models, owns the
 * `j`/`k` roving cursor used when the sidebar pane has keyboard focus, and wires
 * drag-to-reorder for smart mailboxes and accounts.
 *
 * @spec docs/L1-ui#component-hierarchy
 * @spec docs/ui/L0#navigation-model
 */
import { useCallback, useMemo, useState } from 'react'

import { useActivePane, useFocusedPaneHandler } from './keyboard/usePane'
import { useMailboxNavigationReadModels } from '../mailboxNavigationReadModels'
import { sortSmartMailboxes } from './sidebar/model'
import { useSidebarReorder } from './sidebar/useSidebarReorder'
import {
  moveRovingKey,
  sidebarSelectionKey,
  smartNavKey,
  sourceNavKey,
  type SidebarNavKey,
} from './sidebar/roving'
import {
  AccountsSection,
  SidebarError,
  SidebarLoading,
  SmartMailboxSection,
} from './sidebar/SidebarContent'

/**
 * Discriminated union representing the current sidebar selection.
 * @spec docs/ui/L0#navigation-model
 */
export type SidebarSelection =
  | { kind: 'smart-mailbox'; id: string; name: string }
  | {
      kind: 'source-mailbox'
      sourceId: string
      mailboxId: string
      name: string
    }

/** @spec docs/L1-ui#component-hierarchy */
interface SidebarProps {
  selectedView: SidebarSelection | null
  onOpenAccountSettings: (sourceId: string) => void
  onOpenSmartMailboxSettings: (smartMailboxId: string) => void
  onSelectSmartMailbox: (smartMailboxId: string, name: string) => void
  onSelectSourceMailbox: (
    sourceId: string,
    mailboxId: string,
    name: string,
  ) => void
  onSyncSource: (sourceId: string) => void
}

/**
 * Sidebar navigation with smart mailbox and source mailbox sections.
 *
 * @spec docs/L1-ui#component-hierarchy
 * @spec docs/ui/L0#navigation-model
 */
export function Sidebar({
  selectedView,
  onOpenAccountSettings,
  onOpenSmartMailboxSettings,
  onSelectSmartMailbox,
  onSelectSourceMailbox,
  onSyncSource,
}: SidebarProps) {
  const { error, isLoading, refetchBootstrap, smartMailboxes, sources } =
    useMailboxNavigationReadModels()
  const { reorderSmartMailboxes, reorderAccounts } = useSidebarReorder()

  const [mailboxesCollapsed, setMailboxesCollapsed] = useState(false)
  const [sourcesCollapsed, setSourcesCollapsed] = useState(false)
  // Per-source collapse is owned here (not in SourceSection) so the roving
  // cursor only ever lands on rows that are actually visible.
  const [collapsedSourceIds, setCollapsedSourceIds] = useState<
    ReadonlySet<string>
  >(() => new Set())
  const [focusedKey, setFocusedKey] = useState<SidebarNavKey | null>(null)

  const { activePane } = useActivePane()
  const isSidebarActive = activePane === 'sidebar'

  const sortedSmartMailboxes = useMemo(
    () => sortSmartMailboxes(smartMailboxes),
    [smartMailboxes],
  )

  const toggleSourceCollapsed = useCallback((sourceId: string) => {
    setCollapsedSourceIds((prev) => {
      const next = new Set(prev)
      if (next.has(sourceId)) {
        next.delete(sourceId)
      } else {
        next.add(sourceId)
      }
      return next
    })
  }, [])

  // Flat, in-DOM-order list of navigable rows, honoring every collapse state.
  const navItems = useMemo(() => {
    const items: { key: SidebarNavKey; activate: () => void }[] = []
    if (!mailboxesCollapsed) {
      for (const smartMailbox of sortedSmartMailboxes) {
        items.push({
          key: smartNavKey(smartMailbox.id),
          activate: () =>
            onSelectSmartMailbox(smartMailbox.id, smartMailbox.name),
        })
      }
    }
    if (!sourcesCollapsed) {
      for (const source of sources) {
        if (collapsedSourceIds.has(source.id)) continue
        for (const mailbox of source.mailboxes) {
          items.push({
            key: sourceNavKey(source.id, mailbox.id),
            activate: () =>
              onSelectSourceMailbox(
                source.id,
                mailbox.id,
                `${source.name} / ${mailbox.name}`,
              ),
          })
        }
      }
    }
    return items
  }, [
    collapsedSourceIds,
    mailboxesCollapsed,
    onSelectSmartMailbox,
    onSelectSourceMailbox,
    sortedSmartMailboxes,
    sources,
    sourcesCollapsed,
  ])

  // The cursor defaults to the current view's row until the user moves it, so
  // the first `j`/`k` steps from where they already are.
  const selectionKey = sidebarSelectionKey(selectedView)
  const baseKey = focusedKey ?? selectionKey
  const highlightKey = isSidebarActive ? baseKey : null

  useFocusedPaneHandler('sidebar', (event) => {
    if (event.metaKey || event.ctrlKey || event.altKey) return false
    const keys = navItems.map((item) => item.key)
    switch (event.key) {
      case 'j':
      case 'ArrowDown':
        event.preventDefault()
        setFocusedKey(moveRovingKey(keys, baseKey, 1))
        return true
      case 'k':
      case 'ArrowUp':
        event.preventDefault()
        setFocusedKey(moveRovingKey(keys, baseKey, -1))
        return true
      case 'Enter':
      case ' ': {
        const index = baseKey ? keys.indexOf(baseKey) : -1
        if (index === -1) return false
        event.preventDefault()
        navItems[index].activate()
        return true
      }
      default:
        return false
    }
  })

  return (
    <aside className="flex h-full min-h-0 min-w-0 flex-col bg-sidebar text-sidebar-foreground">
      <nav className="ph-scroll min-h-0 flex-1 overflow-y-auto px-2 pb-4 pt-3">
        {isLoading && <SidebarLoading />}
        {error && <SidebarError onRetry={() => void refetchBootstrap()} />}
        {!isLoading && !error && (
          <>
            <SmartMailboxSection
              collapsed={mailboxesCollapsed}
              mailboxes={sortedSmartMailboxes}
              selectedView={selectedView}
              focusedKey={highlightKey}
              onOpenSmartMailboxSettings={onOpenSmartMailboxSettings}
              onSelectSmartMailbox={onSelectSmartMailbox}
              onReorder={reorderSmartMailboxes}
              onToggle={() => setMailboxesCollapsed((prev) => !prev)}
            />
            <AccountsSection
              collapsed={sourcesCollapsed}
              selectedView={selectedView}
              focusedKey={highlightKey}
              collapsedSourceIds={collapsedSourceIds}
              sources={sources}
              onOpenAccountSettings={onOpenAccountSettings}
              onSelectSourceMailbox={onSelectSourceMailbox}
              onSyncSource={onSyncSource}
              onReorder={reorderAccounts}
              onToggle={() => setSourcesCollapsed((prev) => !prev)}
              onToggleSourceCollapsed={toggleSourceCollapsed}
            />
          </>
        )}
      </nav>
    </aside>
  )
}
