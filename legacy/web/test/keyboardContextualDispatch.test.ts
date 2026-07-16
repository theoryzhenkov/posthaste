/**
 * PLAN-L2 Slice 5 — end-to-end keyboard path through `dispatchMailKey`.
 *
 * Wires the registry tier exactly as `KeyboardController` does (a hook built from
 * `resolveKeyboardAction` + `runResolvedWithConfirm`) and drives it through the
 * real dispatcher, proving:
 *   - In Trash, `#` routes to delete-permanently and PROMPTS (fail-before: the
 *     old if-chain called `onTrash`/move-to-trash instead).
 *   - Outside Trash, `#` moves to trash instantly.
 *   - When the registry has no available action, dispatch falls back to the
 *     native selection-scoped handlers (nothing is lost).
 */
import { describe, expect, it, mock } from 'bun:test'

import { setupDomEnvironment } from './dom-env'
import {
  resolveKeyboardAction,
  runResolvedWithConfirm,
  type ActionConfirm,
} from '../src/actions/keyboard'
import type {
  ActionContext,
  ActionServices,
  MessageTarget,
} from '../src/actions/types'
import type { MessageSummary, SourceMessageRef } from '../src/api/types'
import { SYSTEM_KEYWORDS } from '../src/domainVocabulary'
import type { EmailActions } from '../src/hooks/useEmailActions'
import {
  dispatchMailKey,
  PANE_ORDER,
  type KeyboardDispatchContext,
  type RegistryKeyHook,
} from '../src/components/keyboard/dispatch'

setupDomEnvironment()

const REF: SourceMessageRef = { sourceId: 's1', messageId: 'm1' }

function makeSpyActions() {
  return {
    toggleRead: mock(() => {}),
    toggleFlag: mock(() => {}),
    archive: mock(() => {}),
    moveToInbox: mock(() => {}),
    trash: mock(() => {}),
    deletePermanently: mock(() => {}),
    discardDraft: mock(() => {}),
    isPending: false,
  }
}

function summary(over: Partial<MessageSummary> = {}): MessageSummary {
  return {
    id: 'm1',
    sourceId: 's1',
    sourceName: 'Acct',
    sourceThreadId: 't1',
    conversationId: 'c1',
    subject: 'Hi',
    fromName: 'A',
    fromEmail: 'a@x.test',
    to: [],
    preview: null,
    receivedAt: '2026-01-01T00:00:00Z',
    hasAttachment: false,
    isRead: false,
    isFlagged: false,
    mailboxIds: ['mb1'],
    keywords: [],
    draftId: null,
    ...over,
  }
}

function target(over: Partial<MessageSummary> = {}): MessageTarget {
  const s = summary(over)
  return {
    ref: REF,
    summary: s,
    isDraft: s.keywords.includes(SYSTEM_KEYWORDS.Draft),
    draftId: s.draftId,
    conversationId: s.conversationId,
  }
}

/** Build the registry hook the way KeyboardController does, capturing any
 *  confirm request so the test can assert the prompt and later accept it. */
function makeHook(
  viewRole: string | null,
  services: ActionServices,
  onConfirmRequest: (c: ActionConfirm, run: () => void) => void,
  tgt: MessageTarget | null = target(),
): RegistryKeyHook {
  return {
    match: (event) => {
      const keyboardCtx: ActionContext = {
        targets: tgt ? [tgt] : [],
        viewRole,
        activePane: 'list',
        surface: 'keyboard',
        inputOwner: 'mail',
        hasPendingMutation: false,
        connection: 'unknown',
      }
      const resolved = resolveKeyboardAction(event, keyboardCtx, services)
      if (!resolved) return null
      return {
        id: resolved.def.id,
        run: () => runResolvedWithConfirm(resolved, onConfirmRequest),
      }
    },
  }
}

function baseCtx(
  over: Partial<KeyboardDispatchContext>,
): KeyboardDispatchContext {
  const noop = () => {}
  return {
    effectiveSurfaceOpen: false,
    overlayOwnsInput: false,
    hasSelectedMessage: true,
    hasSearchQuery: false,
    activePane: 'list',
    availablePanes: PANE_ORDER,
    focusPane: noop,
    resolvePaneHandler: () => undefined,
    pendingPrefix: null,
    setPendingPrefix: noop,
    onGoto: noop,
    onGotoConversation: noop,
    onOpenCommandPalette: noop,
    onOpenSettings: noop,
    onCompose: noop,
    onReply: noop,
    onReplyAll: noop,
    onToggleFlag: noop,
    onUndo: noop,
    onRedo: noop,
    onArchive: noop,
    onTrash: noop,
    onOpenTagEditor: noop,
    onOpenFocusedMessage: noop,
    onClearSelectedMessage: noop,
    onClearSearchQuery: noop,
    onToggleShortcuts: noop,
    ...over,
  }
}

function hashEvent(): KeyboardEvent {
  return {
    key: '#',
    metaKey: false,
    ctrlKey: false,
    shiftKey: true,
    altKey: false,
    target: null,
    preventDefault: () => {},
  } as unknown as KeyboardEvent
}

describe('dispatchMailKey — contextual # via the registry tier', () => {
  // spec: docs/eph/PLAN-L2-action-registry.md
  it('in TRASH, # delete-permanently PROMPTS and does NOT move-to-trash', () => {
    const email = makeSpyActions()
    const services: ActionServices = { email: email as unknown as EmailActions }
    let captured: (() => void) | null = null
    const onTrash = mock(() => {})
    const hook = makeHook('trash', services, (_c, run) => {
      captured = run
    })
    dispatchMailKey(hashEvent(), baseCtx({ onTrash, registryHook: hook }))

    // The old behavior (native onTrash / move-to-trash) must NOT happen.
    expect(onTrash).not.toHaveBeenCalled()
    expect(email.trash).not.toHaveBeenCalled()
    // A confirm was requested; the delete only runs on accept.
    expect(captured).not.toBeNull()
    expect(email.deletePermanently).not.toHaveBeenCalled()
    captured!()
    expect(email.deletePermanently).toHaveBeenCalledWith(REF)
  })

  it('OUTSIDE trash, # moves to trash instantly (no prompt, no native onTrash)', () => {
    const email = makeSpyActions()
    const services: ActionServices = { email: email as unknown as EmailActions }
    const onTrash = mock(() => {})
    const requestConfirm = mock(() => {})
    const hook = makeHook('inbox', services, requestConfirm)
    dispatchMailKey(hashEvent(), baseCtx({ onTrash, registryHook: hook }))

    expect(email.trash).toHaveBeenCalledWith(REF)
    expect(requestConfirm).not.toHaveBeenCalled()
    // Registry consumed the event — native fallback did not double-fire.
    expect(onTrash).not.toHaveBeenCalled()
    expect(email.deletePermanently).not.toHaveBeenCalled()
  })

  it('falls back to native onTrash when the registry has no match (draft)', () => {
    const email = makeSpyActions()
    const services: ActionServices = { email: email as unknown as EmailActions }
    const onTrash = mock(() => {})
    const draft = target({ keywords: [SYSTEM_KEYWORDS.Draft] })
    const hook = makeHook('inbox', services, () => {}, draft)
    dispatchMailKey(hashEvent(), baseCtx({ onTrash, registryHook: hook }))

    // No registry action for a draft → native handler runs (draft-discard split).
    expect(onTrash).toHaveBeenCalledTimes(1)
    expect(email.trash).not.toHaveBeenCalled()
    expect(email.deletePermanently).not.toHaveBeenCalled()
  })

  it('with no registryHook at all, the native handler still fires (back-compat)', () => {
    const onTrash = mock(() => {})
    dispatchMailKey(hashEvent(), baseCtx({ onTrash }))
    expect(onTrash).toHaveBeenCalledTimes(1)
  })
})
