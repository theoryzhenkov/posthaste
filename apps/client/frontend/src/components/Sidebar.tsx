/**
 * Left-pane sidebar with smart mailbox and source mailbox navigation.
 *
 * Renders normalized account, mailbox, and smart mailbox read models, handles
 * `j`/`k` keyboard navigation (which selects the adjacent mailbox immediately
 * when the sidebar pane has focus), and wires drag-to-reorder for smart
 * mailboxes and accounts.
 *
 * @spec docs/L1-ui#component-hierarchy
 * @spec docs/ui/L0#navigation-model
 */
import { useCallback, useMemo, useState } from 'react'

import { useActivePane, useFocusedPaneHandler } from './keyboard/usePane'
import { useMailboxNavigationReadModels } from '../mailboxNavigationReadModels'
import {
  sortSmartMailboxes,
  visibleSmartMailboxes,
  visibleSourceMailboxes,
} from './sidebar/model'
import { useMailboxGroups } from './sidebar/useMailboxGroups'
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
  const groups = useMailboxGroups()

  const [mailboxesCollapsed, setMailboxesCollapsed] = useState(false)
  const [sourcesCollapsed, setSourcesCollapsed] = useState(false)
  // Per-source collapse is owned here (not in SourceSection) so `j`/`k` only ever
  // land on rows that are actually visible.
  const [collapsedSourceIds, setCollapsedSourceIds] = useState<
    ReadonlySet<string>
  >(() => new Set())
  // Per-group collapse is owned here for the same reason: a collapsed Group must
  // hide its member mailboxes from the `j`/`k` walk (mirrors per-source collapse).
  const [collapsedGroupIds, setCollapsedGroupIds] = useState<
    ReadonlySet<string>
  >(() => new Set())

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

  const toggleGroupCollapsed = useCallback((groupId: string) => {
    setCollapsedGroupIds((prev) => {
      const next = new Set(prev)
      if (next.has(groupId)) {
        next.delete(groupId)
      } else {
        next.add(groupId)
      }
      return next
    })
  }, [])

  // Flat, in-DOM-order list of navigable rows, honoring every collapse state.
  const navItems = useMemo(() => {
    const items: { key: SidebarNavKey; activate: () => void }[] = []
    if (!mailboxesCollapsed) {
      // Walk in DOM order: ungrouped smart mailboxes first, then each expanded
      // Group's members — a collapsed smart Group hides its members from the
      // walk (mirrors the source section), so `j`/`k` only lands on visible rows.
      for (const smartMailbox of visibleSmartMailboxes(
        sortedSmartMailboxes,
        groups,
        collapsedGroupIds,
      )) {
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
        // Walk in DOM order: ungrouped mailboxes first, then each expanded
        // Group's members — a collapsed Group hides its members from the walk
        // (mirrors per-source collapse), so `j`/`k` only lands on visible rows.
        for (const mailbox of visibleSourceMailboxes(
          source.mailboxes,
          groups,
          collapsedGroupIds,
        )) {
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
    collapsedGroupIds,
    collapsedSourceIds,
    groups,
    mailboxesCollapsed,
    onSelectSmartMailbox,
    onSelectSourceMailbox,
    sortedSmartMailboxes,
    sources,
    sourcesCollapsed,
  ])

  // `j`/`k` select immediately — there is no separate roving cursor. The selected
  // mailbox's own highlight is the cursor, so navigation needs no extra `Enter`
  // and is always visible; stepping is relative to the current selection.
  const selectionKey = sidebarSelectionKey(selectedView)

  useFocusedPaneHandler('sidebar', (event) => {
    if (event.metaKey || event.ctrlKey || event.altKey) return false
    const selectAdjacent = (direction: 1 | -1): boolean => {
      const keys = navItems.map((item) => item.key)
      const nextKey = moveRovingKey(keys, selectionKey, direction)
      const next = navItems.find((item) => item.key === nextKey)
      if (!next) return false
      next.activate()
      return true
    }
    switch (event.key) {
      case 'j':
      case 'ArrowDown':
        event.preventDefault()
        return selectAdjacent(1)
      case 'k':
      case 'ArrowUp':
        event.preventDefault()
        return selectAdjacent(-1)
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
              isPaneActive={isSidebarActive}
              collapsedGroupIds={collapsedGroupIds}
              onOpenSmartMailboxSettings={onOpenSmartMailboxSettings}
              onSelectSmartMailbox={onSelectSmartMailbox}
              onReorder={reorderSmartMailboxes}
              onToggle={() => setMailboxesCollapsed((prev) => !prev)}
              onToggleGroupCollapsed={toggleGroupCollapsed}
            />
            <AccountsSection
              collapsed={sourcesCollapsed}
              selectedView={selectedView}
              isPaneActive={isSidebarActive}
              collapsedSourceIds={collapsedSourceIds}
              collapsedGroupIds={collapsedGroupIds}
              sources={sources}
              onOpenAccountSettings={onOpenAccountSettings}
              onSelectSourceMailbox={onSelectSourceMailbox}
              onSyncSource={onSyncSource}
              onReorder={reorderAccounts}
              onToggle={() => setSourcesCollapsed((prev) => !prev)}
              onToggleSourceCollapsed={toggleSourceCollapsed}
              onToggleGroupCollapsed={toggleGroupCollapsed}
            />
          </>
        )}
      </nav>
    </aside>
  )
}
