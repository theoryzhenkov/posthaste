import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type UIEvent as ReactUIEvent,
} from 'react'

import { getAction, type ActionContext, type ActionServices } from '@/commands'
import type { MessageSummary } from '@/gen'
import type {
  CommandPaletteEntry,
  SearchCandidate,
} from '@/components/palette/search/types'
import { SYSTEM_KEYWORDS } from '@/domain/vocabulary'
import type { useMailClientHandlers } from '@/app/mail/useMailClientHandlers'
import type { EmailActions } from '@/data/hooks/useEmailActions'
import { useMailboxNavigationReadModels } from '@/data/models/mailboxNavigation'
import type { MailSelection } from '@/data/models/selection'
import { validateSearchQuery } from '@/domain/search'
import { normalizeAppliedSearchQuery } from '@/domain/searchQuery'

import { CommandPaletteList } from './CommandPaletteList'
import {
  COMMAND_PANEL_STORAGE_KEY,
  commandPaletteEntryValue,
  NO_COMMAND_PALETTE_SELECTION,
  resolvePaletteEnter,
} from './model'
import { useCommandPaletteSearch } from './useCommandPaletteSearch'
import { usePaletteActions } from './usePaletteActions'
import { FloatingPanel } from '../floating/FloatingPanel'
import { Command, CommandInput } from '../ui/overlay/command'

interface CommandPaletteProps {
  /** Domain mutations — the `email` half of the palette's {@link ActionServices}. */
  actions: EmailActions
  /** App/handler bundle — the `app` half of {@link ActionServices} (compose,
   *  settings, shortcuts, reply, tag editor, snooze placeholder). */
  app: ReturnType<typeof useMailClientHandlers>
  /** Role of the current view, gating contextual palette actions. */
  viewRole: string | null
  /** Open straight into a parameterized action's pick-step (the keyboard
   *  chord → picker path, e.g. `m` → the mailbox picker). */
  initialActionId?: string | null
  selectedMessage: MailSelection | null
  selectedMessageData: MessageSummary | undefined
  onApplySearch: (query: string) => void
  onClose: () => void
  onSelectMessage: (selection: MailSelection) => void
  onSelectSmartMailbox: (smartMailboxId: string, name: string) => void
  onSelectSourceMailbox: (
    sourceId: string,
    mailboxId: string,
    name: string,
  ) => void
}

/** The pick-step section label / input placeholder for an action id. */
function paramStepFor(
  actionId: string,
): { actionId: string; label: string } | null {
  const def = getAction(actionId)
  if (!def?.resolveParams) return null
  return {
    actionId,
    label: typeof def.title === 'string' ? def.title : actionId,
  }
}

export function CommandPalette({
  actions,
  app,
  viewRole,
  initialActionId,
  selectedMessage,
  selectedMessageData,
  onApplySearch,
  onClose,
  onSelectMessage,
  onSelectSmartMailbox,
  onSelectSourceMailbox,
}: CommandPaletteProps) {
  const [query, setQuery] = useState('')
  // Two-step flow (parameterized actions): non-null while the palette shows an
  // action's OPTION list instead of the root command/search list. Seeded when a
  // keyboard chord opened the palette directly into a picker.
  const [paramStep, setParamStep] = useState<{
    actionId: string
    label: string
  } | null>(() => (initialActionId ? paramStepFor(initialActionId) : null))

  // The ActionContext for the palette surface: the focused message becomes the
  // single target, so registry actions resolve exactly as the context menu does
  // (contextual availability, disabled-with-reason). Rebuilt per render — cheap.
  const actionContext = useMemo<ActionContext>(() => {
    const targets = selectedMessage
      ? [
          {
            ref: {
              sourceId: selectedMessage.sourceId,
              messageId: selectedMessage.messageId,
            },
            summary: selectedMessageData,
            isDraft:
              selectedMessageData?.keywords.includes(SYSTEM_KEYWORDS.Draft) ??
              false,
            draftId: selectedMessageData?.draftId ?? null,
            conversationId: selectedMessage.conversationId,
          },
        ]
      : []
    return {
      targets,
      viewRole,
      activePane: 'list',
      surface: 'palette',
      inputOwner: 'overlay',
      hasPendingMutation: actions.isPending,
      connection: 'unknown',
    }
  }, [actions.isPending, selectedMessage, selectedMessageData, viewRole])

  // The mailbox read model (the sidebar's source), bound as the palette's
  // `ActionServices.mailboxes` so parameterized mailbox actions (Move to…)
  // resolve their options here too.
  const readModels = useMailboxNavigationReadModels()
  const services = useMemo<ActionServices>(
    () => ({
      email: actions,
      app,
      mailboxes: {
        list: (sourceId: string) =>
          readModels.sources.find((source) => source.id === sourceId)
            ?.mailboxes ?? [],
      },
    }),
    [actions, app, readModels.sources],
  )

  // Providers read ctx/services through stable getters so the provider list
  // never re-creates on selection changes (which would restart every search).
  const contextRef = useRef(actionContext)
  const servicesRef = useRef(services)
  useEffect(() => {
    contextRef.current = actionContext
  }, [actionContext])
  useEffect(() => {
    servicesRef.current = services
  }, [services])
  const getActionContext = useCallback(() => contextRef.current, [])
  const getActionServices = useCallback(() => servicesRef.current, [])

  const { activeSelectedIndex, itemRows, search, selectedValue } =
    useCommandPaletteSearch({
      hasSelectedMessage: selectedMessage !== null,
      query,
      getActionContext,
      getActionServices,
      paramStep,
    })

  function handleQueryChange(value: string) {
    setQuery(value)
    search.select(null)
  }

  /** Enter a parameterized action's pick-step: swap the provider set and reset
   *  the query so the user types the TARGET (e.g. a mailbox name). */
  function enterParamStep(actionId: string) {
    const step = paramStepFor(actionId)
    if (!step) return
    setParamStep(step)
    setQuery('')
    search.select(null)
  }

  /** Pop back from the pick-step to the root command list. */
  function exitParamStep() {
    setParamStep(null)
    setQuery('')
    search.select(null)
  }

  function closeWithoutApplyingQuery() {
    search.cancel()
    onClose()
  }

  // The typed query never touches the underlying mail view while typing — it
  // only filters palette candidates — so a selected message stays in scope. The
  // app-wide mail-view filter is applied here, only on Shift+Enter; a plain
  // Enter navigates into the in-pane results instead (see handlePaletteKeyDown).
  function applyCurrentQuery() {
    const normalized = normalizeAppliedSearchQuery(query)
    if (!normalized || validateSearchQuery(query).state !== 'valid') return
    onApplySearch(normalized)
    onClose()
  }

  const nav = useMemo(
    () => ({
      onApplySearch,
      onSelectMessage,
      onSelectSmartMailbox,
      onSelectSourceMailbox,
      replaceQuery: (nextQuery: string) => {
        setQuery(nextQuery)
        search.select(null)
      },
      openActionParams: enterParamStep,
    }),
    // enterParamStep is a plain closure over stable setters + `search`, which
    // is already a dependency.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [
      onApplySearch,
      onSelectMessage,
      onSelectSmartMailbox,
      onSelectSourceMailbox,
      search,
    ],
  )
  const executeAction = usePaletteActions({ actionContext, services, nav })

  function runEntry(entry: CommandPaletteEntry) {
    // Disabled registry rows are inert — skip on Enter/click.
    if (entry.disabled) return
    executeAction(entry.action)
    if (entry.closeOnSelect !== false) {
      onClose()
    }
  }

  function runCandidate(candidate: SearchCandidate) {
    runEntry(candidate.entry)
  }

  function handlePaletteKeyDown(event: ReactKeyboardEvent<HTMLDivElement>) {
    const isDownKey =
      event.key === 'ArrowDown' ||
      (event.key === 'j' && (activeSelectedIndex !== -1 || event.ctrlKey))
    const isUpKey =
      event.key === 'ArrowUp' ||
      (event.key === 'k' && (activeSelectedIndex !== -1 || event.ctrlKey))

    if (event.key === 'Escape') {
      event.preventDefault()
      // In a pick-step, Escape backs out to the root list; a second Escape
      // closes — mirroring nested-menu affordances.
      if (paramStep) {
        exitParamStep()
        return
      }
      closeWithoutApplyingQuery()
      return
    }
    // Backspace on an empty pick-step query also pops back to the root list.
    if (event.key === 'Backspace' && paramStep && query === '') {
      event.preventDefault()
      exitParamStep()
      return
    }
    if (isDownKey || isUpKey) {
      event.preventDefault()
      moveSelection(isDownKey ? 1 : -1)
      return
    }
    if (event.key === 'Enter') {
      event.preventDefault()
      const action = resolvePaletteEnter({
        shiftKey: event.shiftKey,
        hasHighlightedItem: activeSelectedIndex !== -1,
        hasItems: itemRows.length > 0,
      })
      switch (action) {
        case 'apply':
          // The pick-step query filters OPTIONS, never the mail view.
          if (!paramStep) applyCurrentQuery()
          break
        case 'run':
          runCandidate(itemRows[activeSelectedIndex].candidate)
          break
        case 'navigate':
          moveSelection(1)
          break
        case 'none':
          break
      }
    }
  }

  function moveSelection(delta: 1 | -1) {
    if (itemRows.length === 0) {
      search.select(null)
      return
    }
    if (delta === -1 && activeSelectedIndex === -1) {
      search.select(itemRows.at(-1)?.candidate.id ?? null)
      return
    }
    const nextIndex =
      delta === 1
        ? activeSelectedIndex === -1
          ? 0
          : Math.min(activeSelectedIndex + 1, itemRows.length - 1)
        : activeSelectedIndex - 1
    search.select(nextIndex < 0 ? null : itemRows[nextIndex].candidate.id)
  }

  function handleListScroll(event: ReactUIEvent<HTMLDivElement>) {
    const node = event.currentTarget
    const distanceToEnd = node.scrollHeight - node.scrollTop - node.clientHeight
    if (distanceToEnd > 120) return
    for (const [providerId, state] of search.session.providerStates) {
      if (state.nextCursor) {
        search.loadMore(providerId)
        break
      }
    }
  }

  return (
    <Command
      shouldFilter={false}
      loop={false}
      value={selectedValue}
      className="contents"
      onValueChange={(value) => {
        if (value === NO_COMMAND_PALETTE_SELECTION) {
          search.select(null)
          return
        }
        const next = itemRows.find(
          (row) => commandPaletteEntryValue(row.candidate) === value,
        )
        search.select(next?.candidate.id ?? null)
      }}
      onKeyDown={handlePaletteKeyDown}
    >
      <FloatingPanel
        panelLabel="command palette"
        layer="overlay"
        storageKey={COMMAND_PANEL_STORAGE_KEY}
        closeIgnoreSelector="[data-command-search-trigger='true']"
        sizePreset="command"
        header={
          <CommandInput
            autoFocus
            value={query}
            onValueChange={handleQueryChange}
            placeholder={
              paramStep
                ? paramStep.label
                : 'Search messages, contacts, commands...'
            }
            wrapperClassName="min-w-0 flex-1 h-12 px-3"
          />
        }
        onClose={closeWithoutApplyingQuery}
      >
        <CommandPaletteList
          itemRowsLength={itemRows.length}
          rows={search.session.rows}
          onRunCandidate={runCandidate}
          onScroll={handleListScroll}
          onSelectNone={() => search.select(null)}
        />
      </FloatingPanel>
    </Command>
  )
}
