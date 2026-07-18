/**
 * Compose command definitions: starting a composition (new, reply, reply-all,
 * forward, edit-draft) and sending the active one.
 *
 * Registration order is the resolver's within-section tiebreak; within
 * `compose-reply` the reply/forward/edit entries precede `app.compose`,
 * preserving the historical palette order.
 */
import { Forward, PenSquare, Pencil, Reply, ReplyAll, Send } from 'lucide-react'
import { registerActions } from '../registry'
import type { ActionDefinition } from '../types'
import { hasDraftTarget, requireTarget } from './shared'

const composeActions: readonly ActionDefinition[] = [
  {
    // Reply: delegates to the header binding when present (works in the focused
    // message window too), else the app handler (palette/keyboard). ⌘R is a
    // registry chord — `aboveOverlay` + `inEditable` place it in the mail
    // dispatcher's modifier-chord tier; `shift: false` keeps ⌘⇧R exclusively
    // reply-all's.
    id: 'message.reply',
    section: 'compose-reply',
    title: 'Reply',
    icon: Reply,
    keywords: 'reply respond answer',
    surfaces: ['palette', 'detail-header', 'keyboard'],
    shortcut: {
      key: 'r',
      mod: true,
      shift: false,
      inEditable: true,
      aboveOverlay: true,
    },
    // Never on a draft (you edit a draft, not reply to it); hidden where no
    // execution binding exists (e.g. the dispatcher's target-less scopes).
    isAvailable: (ctx, s) => !hasDraftTarget(ctx) && Boolean(s.detail ?? s.app),
    isEnabled: requireTarget,
    run: (_ctx, s) => (s.detail?.reply ?? s.app?.handleReply)?.(),
  },
  {
    id: 'message.reply-all',
    section: 'compose-reply',
    title: 'Reply All',
    icon: ReplyAll,
    keywords: 'reply all respond everyone',
    surfaces: ['palette', 'detail-header', 'keyboard'],
    shortcut: {
      key: 'r',
      mod: true,
      shift: true,
      inEditable: true,
      aboveOverlay: true,
    },
    isAvailable: (ctx, s) => !hasDraftTarget(ctx) && Boolean(s.detail ?? s.app),
    isEnabled: requireTarget,
    run: (_ctx, s) => (s.detail?.replyAll ?? s.app?.handleReplyAll)?.(),
  },
  {
    id: 'message.forward',
    section: 'compose-reply',
    title: 'Forward',
    icon: Forward,
    keywords: 'forward send along',
    surfaces: ['palette', 'detail-header'],
    isAvailable: (ctx) => !hasDraftTarget(ctx),
    isEnabled: requireTarget,
    run: (_ctx, s) => (s.detail?.forward ?? s.app?.handleForward)?.(),
  },
  {
    // "Edit draft" — availability-driven (every target must be a draft), also
    // reachable from the palette.
    id: 'message.edit-draft',
    section: 'compose-reply',
    title: 'Edit draft',
    icon: Pencil,
    keywords: 'edit draft compose continue',
    surfaces: ['palette', 'detail-header'],
    isAvailable: (ctx, s) =>
      ctx.targets.length > 0 &&
      ctx.targets.every((t) => t.isDraft) &&
      Boolean(s.detail?.editDraft ?? s.app),
    isEnabled: requireTarget,
    run: (_ctx, s) => (s.detail?.editDraft ?? s.app?.handleEditDraft)?.(),
  },
  {
    id: 'app.compose',
    section: 'compose-reply',
    title: 'Compose new message',
    icon: PenSquare,
    keywords: 'compose new message draft',
    surfaces: ['palette', 'keyboard'],
    // Modifier-chord tier: ⌘N fires even while typing or above an overlay.
    shortcut: { key: 'n', mod: true, inEditable: true, aboveOverlay: true },
    // Gated on the app bundle so the dispatcher's app-less scopes never claim
    // the chord vacuously.
    isAvailable: (_ctx, s) => Boolean(s.app),
    run: (_ctx, s) => s.app?.handleCompose(),
  },
  {
    // ⌘Enter sends the ACTIVE composer. Bound via the dispatcher's `compose`
    // scope service while a compose form is mounted (replacing the overlay's
    // own window listener); `inEditable` because the sender is, by
    // definition, typing.
    id: 'compose.send',
    section: 'compose-reply',
    title: 'Send',
    icon: Send,
    keywords: 'send message now',
    surfaces: ['keyboard'],
    shortcut: { key: 'enter', mod: true, inEditable: true },
    isAvailable: (_ctx, s) => Boolean(s.compose),
    run: (_ctx, s) => s.compose?.send(),
  },
]

registerActions(composeActions)
