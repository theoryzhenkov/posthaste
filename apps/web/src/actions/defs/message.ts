/**
 * Message action definitions (PLAN-L2, Slice 1).
 *
 * A faithful port of the pure, role-gated context-menu actions that live in
 * `actions/contextualActions.ts` today — same labels, icons, destructive flags,
 * and `viewRole`/draft gating — expressed as {@link ActionDefinition}s that
 * delegate to `services.email` (never reimplementing the domain logic).
 *
 * Registration order below is meaningful: the resolver uses it as the
 * within-section tiebreak, so it mirrors the old builder's push order (the two
 * `open` entries, then `toggle-read`/`toggle-flag`, then `archive`,
 * `move-to-inbox`, and the mutually-exclusive trash/delete/discard trio) to
 * preserve menu ordering.
 *
 * Slice 2 folds in the two row-scoped `open` / `view-conversation` entries the
 * old shim owned: they delegate to `services.row` (bound per row by
 * `MessageRow`) and gate their availability on that binding, so they surface in
 * the context menu but stay absent on every non-row surface. Still omitted here:
 * palette-only enrichments and a `confirm` on `delete-permanently` (today it
 * runs unconfirmed — normalized in a later slice). Behavior is preserved exactly.
 *
 * @spec docs/eph/PLAN-L2-action-registry.md
 */
import {
  Archive,
  Clock3,
  Eye,
  EyeOff,
  Inbox,
  MailOpen,
  MessagesSquare,
  Reply,
  Star,
  Tag,
  Trash2,
} from 'lucide-react'
import { registerActions } from '../registry'
import type { ActionContext, ActionDefinition, MessageTarget } from '../types'

/** Roles from which a message is "removed" and can be restored to the inbox
 *  (mirrors `contextualActions.isRestorableRole`). */
function isRestorableRole(role: string | null): boolean {
  return role === 'trash' || role === 'archive' || role === 'junk'
}

/** The subject a single-target action operates on. Slice-1 surfaces bind exactly
 *  one target; `?? undefined` keeps the accessors total for the empty case. */
function primaryTarget(ctx: ActionContext): MessageTarget | undefined {
  return ctx.targets[0]
}

/** The subject passed to keyword-toggle handlers. Prefer the summary (carries
 *  the keyword state the handler derives from); fall back to a `MailSelection`
 *  built from the ref so the handler's cache path resolves state. */
function toggleSubject(t: MessageTarget) {
  return t.summary ?? { ...t.ref, conversationId: t.conversationId ?? '' }
}

/** Base enablement shared by every message action: at least one target. In the
 *  context menu a target always exists, so these stay enabled — matching today.
 *  It exists so the palette (later slice) can render "select a message first". */
function requireTarget(ctx: ActionContext) {
  return ctx.targets.length > 0 || { reason: 'Select a message first' }
}

export const messageActions: readonly ActionDefinition[] = [
  {
    id: 'message.open',
    section: 'open',
    title: 'Open',
    icon: MailOpen,
    keywords: 'open message',
    surfaces: ['context-menu'],
    // Row-scoped: only meaningful when the surface binds `services.row`. This
    // keeps the entry out of every non-row `resolveActions` call (e.g. the
    // parity harness's email-only services) while surfacing it in the menu.
    isAvailable: (_ctx, s) => Boolean(s.row),
    isEnabled: requireTarget,
    run: (ctx, s) => {
      const summary = primaryTarget(ctx)?.summary
      if (summary) s.row?.open(summary)
    },
  },
  {
    id: 'message.view-conversation',
    section: 'open',
    title: 'View conversation',
    icon: MessagesSquare,
    keywords: 'conversation thread view',
    surfaces: ['context-menu'],
    isAvailable: (_ctx, s) => Boolean(s.row),
    isEnabled: requireTarget,
    run: (ctx, s) => {
      const summary = primaryTarget(ctx)?.summary
      if (summary) s.row?.viewConversation(summary)
    },
  },
  {
    id: 'message.toggle-read',
    section: 'state',
    title: (ctx: ActionContext) =>
      primaryTarget(ctx)?.summary?.isRead ? 'Mark unread' : 'Mark read',
    icon: (ctx: ActionContext) =>
      primaryTarget(ctx)?.summary?.isRead ? EyeOff : Eye,
    keywords: 'read unread seen mark',
    surfaces: ['context-menu', 'palette'],
    isEnabled: requireTarget,
    run: (ctx, s) =>
      ctx.targets.forEach((t) => s.email.toggleRead(toggleSubject(t))),
  },
  {
    id: 'message.toggle-flag',
    section: 'state',
    title: (ctx: ActionContext) =>
      primaryTarget(ctx)?.summary?.isFlagged ? 'Unflag' : 'Flag',
    // Star (not the palette's old wrong `Tag` icon, PLAN §1.2) — one chosen icon.
    icon: Star,
    keywords: 'flag unflag star',
    surfaces: ['context-menu', 'palette'],
    shortcut: { key: 'l', mod: true, shift: true },
    isEnabled: requireTarget,
    run: (ctx, s) =>
      ctx.targets.forEach((t) => s.email.toggleFlag(toggleSubject(t))),
  },
  {
    // Palette-only reply: delegates to the app handler (operates on the focused
    // selection), so the palette gains a working Reply that respects the
    // selection instead of the old always-shown static entry.
    id: 'message.reply',
    section: 'compose-reply',
    title: 'Reply',
    icon: Reply,
    keywords: 'reply respond answer',
    surfaces: ['palette'],
    shortcut: { key: 'r', mod: true },
    isEnabled: requireTarget,
    run: (_ctx, s) => s.app?.handleReply(),
  },
  {
    id: 'message.archive',
    section: 'move',
    title: 'Archive',
    icon: Archive,
    keywords: 'archive',
    surfaces: ['context-menu', 'palette', 'keyboard'],
    shortcut: { key: 'e' },
    isAvailable: (ctx) =>
      ctx.viewRole !== 'archive' && ctx.viewRole !== 'trash',
    isEnabled: requireTarget,
    run: (ctx, s) => ctx.targets.forEach((t) => s.email.archive(t.ref)),
  },
  {
    id: 'message.move-to-inbox',
    section: 'move',
    title: 'Move to Inbox',
    icon: Inbox,
    keywords: 'move inbox restore',
    surfaces: ['context-menu', 'palette'],
    isAvailable: (ctx) => isRestorableRole(ctx.viewRole),
    isEnabled: requireTarget,
    run: (ctx, s) => ctx.targets.forEach((t) => s.email.moveToInbox(t.ref)),
  },
  {
    id: 'message.move-to-trash',
    section: 'move',
    title: 'Move to Trash',
    icon: Trash2,
    destructive: true,
    keywords: 'trash delete move',
    surfaces: ['context-menu', 'palette', 'keyboard'],
    // Same chord as delete-permanently below; `isAvailable` disambiguates them
    // (trash-view ⇒ delete-permanently, elsewhere ⇒ this). Stays instant —
    // move-to-trash is reversible via the undo toast.
    shortcut: [{ key: '#' }, { key: 'backspace' }],
    // Not offered in Trash, and never on drafts (D127: a draft is discarded,
    // never trashed).
    isAvailable: (ctx) =>
      ctx.viewRole !== 'trash' && !ctx.targets.some((t) => t.isDraft),
    isEnabled: requireTarget,
    run: (ctx, s) => ctx.targets.forEach((t) => s.email.trash(t.ref)),
  },
  {
    id: 'message.delete-permanently',
    section: 'move',
    title: 'Delete permanently',
    icon: Trash2,
    destructive: true,
    // Irreversible: the keyboard tier PROMPTS via this metadata before running
    // (no silent permanent-delete from a keystroke). The context menu / palette
    // route through the same gate.
    confirm: {
      title: 'Delete permanently?',
      description: 'This message will be destroyed. This cannot be undone.',
      confirmLabel: 'Delete',
    },
    keywords: 'delete permanently destroy',
    surfaces: ['context-menu', 'palette', 'keyboard'],
    // Same `#`/Backspace chord as move-to-trash; availability (trash-view only,
    // never a draft) is what makes the resolver pick this one inside Trash.
    shortcut: [{ key: '#' }, { key: 'backspace' }],
    // Trash view only, and never on drafts.
    isAvailable: (ctx) =>
      ctx.viewRole === 'trash' && !ctx.targets.some((t) => t.isDraft),
    isEnabled: requireTarget,
    run: (ctx, s) =>
      ctx.targets.forEach((t) => s.email.deletePermanently(t.ref)),
  },
  {
    id: 'message.discard-draft',
    section: 'move',
    title: 'Discard draft',
    icon: Trash2,
    destructive: true,
    keywords: 'discard draft delete',
    surfaces: ['context-menu', 'palette'],
    // Every target must be a draft (D127: hard delete via the draft-delete op).
    isAvailable: (ctx) =>
      ctx.targets.length > 0 && ctx.targets.every((t) => t.isDraft),
    isEnabled: requireTarget,
    run: (ctx, s) =>
      ctx.targets.forEach((t) =>
        s.email.discardDraft({ ...t.ref, draftId: t.draftId }),
      ),
  },
  {
    // Palette-only "Tag" command (folds the old dedicated tagActions provider
    // into the registry): opens the tag editor for the focused message. The app
    // handler already no-ops without a selection; `requireTarget` renders it
    // disabled-with-reason in the palette instead of silently vanishing.
    id: 'message.tag',
    section: 'organize',
    title: 'Tag',
    icon: Tag,
    keywords: 'tag add remove label message',
    surfaces: ['palette', 'keyboard'],
    shortcut: { key: 't' },
    isEnabled: requireTarget,
    run: (_ctx, s) => s.app?.handleOpenTagEditor(),
  },
  {
    // Palette-only Snooze — preserves today's placeholder behavior (a "not
    // available yet" toast); it is ungated exactly as the old static entry was.
    id: 'message.snooze',
    section: 'organize',
    title: 'Snooze…',
    icon: Clock3,
    keywords: 'snooze later remind',
    surfaces: ['palette'],
    run: (_ctx, s) => s.app?.handlePlaceholderAction('Snooze'),
  },
]

registerActions(messageActions)
