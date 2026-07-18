/**
 * Navigation command definitions: opening a message (row, conversation,
 * focused window) and closing the focused surface.
 *
 * Registration order is the resolver's within-section tiebreak — the three
 * `open` entries keep their historical order.
 */
import { MailOpen, Maximize2, MessagesSquare, X } from 'lucide-react'
import { conversationViewQuery } from '../../domain/searchQuery'
import { registerActions } from '../registry'
import type { ActionDefinition } from '../types'
import { notDraftOnHeader, primaryTarget, requireTarget } from './shared'

const navigationActions: readonly ActionDefinition[] = [
  {
    id: 'message.open',
    section: 'open',
    title: 'Open',
    icon: MailOpen,
    keywords: 'open message',
    surfaces: ['context-menu', 'palette'],
    // Row-scoped in the menu (`services.row`, bound per row); the palette
    // falls back to the app selection handler so "Open" works on the focused
    // message too. Absent both bindings (e.g. the email-only parity harness)
    // the entry stays hidden.
    isAvailable: (_ctx, s) => Boolean(s.row ?? s.app),
    isEnabled: requireTarget,
    run: (ctx, s) => {
      const summary = primaryTarget(ctx)?.summary
      if (!summary) return
      if (s.row) {
        s.row.open(summary)
        return
      }
      s.app?.handleSelectMessage(summary)
    },
  },
  {
    id: 'message.view-conversation',
    section: 'open',
    title: 'View conversation',
    icon: MessagesSquare,
    keywords: 'conversation thread view show',
    // Palette too (owner gap): falls back to the app search handler with the
    // same conversation query the `gc` keyboard goto applies.
    surfaces: ['context-menu', 'palette'],
    isAvailable: (_ctx, s) => Boolean(s.row ?? s.app),
    isEnabled: requireTarget,
    run: (ctx, s) => {
      const target = primaryTarget(ctx)
      if (!target) return
      if (s.row && target.summary) {
        s.row.viewConversation(target.summary)
        return
      }
      if (target.conversationId) {
        s.app?.handleSearch(conversationViewQuery(target.conversationId))
      }
    },
  },
  {
    // "Open message" in its own window — the header's Maximize affordance,
    // also reachable from the palette and the keyboard `o`.
    id: 'message.open-focused',
    section: 'open',
    title: 'Open message',
    icon: Maximize2,
    keywords: 'open message window focus maximize',
    surfaces: ['palette', 'detail-header', 'keyboard'],
    shortcut: { key: 'o' },
    isAvailable: (ctx, s) =>
      notDraftOnHeader(ctx) &&
      (ctx.surface === 'detail-header'
        ? Boolean(s.detail?.openFocusedMessage)
        : Boolean(s.app)),
    isEnabled: requireTarget,
    run: (_ctx, s) =>
      (s.detail?.openFocusedMessage ?? s.app?.handleOpenFocusedMessage)?.(),
  },
  {
    // Escape closes the focused surface (settings/message/compose host).
    // Bound by the surface hosts via the dispatcher's `surfaceHost` scope
    // service; a host that cannot close simply does not bind, so the chord
    // falls through (replacing SurfaceHost/FocusedSurface's own listeners).
    id: 'surface.close',
    section: 'navigate',
    title: 'Close surface',
    icon: X,
    keywords: 'close surface escape',
    surfaces: ['keyboard'],
    shortcut: { key: 'escape', inEditable: true },
    isAvailable: (ctx, s) =>
      ctx.inputOwner === 'surface' && Boolean(s.surfaceHost),
    run: (_ctx, s) => s.surfaceHost?.close(),
  },
]

registerActions(navigationActions)
