/**
 * Message action definitions (PLAN-L2, Slice 1).
 *
 * A faithful port of the pure, role-gated context-menu actions that live in
 * `actions/contextualActions.ts` today — same labels, icons, destructive flags,
 * and `viewRole`/draft gating — expressed as {@link ActionDefinition}s that
 * delegate to `services.email` (never reimplementing the domain logic).
 *
 * Registration order below is meaningful: the resolver uses it as the
 * within-section tiebreak, so it mirrors the old builder's push order
 * (`archive`, then `move-to-inbox`, then the mutually-exclusive
 * trash/delete/discard trio) to preserve menu ordering.
 *
 * Slice 1 deliberately does NOT include `open` / `view-conversation` (row-scoped
 * hooks, migrated in Slice 2), the palette-only enrichments, or a `confirm` on
 * `delete-permanently` (today it runs unconfirmed — normalized in a later
 * slice). Behavior is preserved exactly.
 *
 * @spec docs/eph/PLAN-L2-action-registry.md
 */
import { Archive, Eye, EyeOff, Inbox, Star, Trash2 } from 'lucide-react'
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
    id: 'message.toggle-read',
    section: 'state',
    title: (ctx: ActionContext) =>
      primaryTarget(ctx)?.summary?.isRead ? 'Mark unread' : 'Mark read',
    icon: (ctx: ActionContext) =>
      primaryTarget(ctx)?.summary?.isRead ? EyeOff : Eye,
    keywords: 'read unread seen mark',
    surfaces: ['context-menu'],
    isEnabled: requireTarget,
    run: (ctx, s) =>
      ctx.targets.forEach((t) => s.email.toggleRead(toggleSubject(t))),
  },
  {
    id: 'message.toggle-flag',
    section: 'state',
    title: (ctx: ActionContext) =>
      primaryTarget(ctx)?.summary?.isFlagged ? 'Unflag' : 'Flag',
    icon: Star,
    keywords: 'flag unflag star',
    surfaces: ['context-menu'],
    isEnabled: requireTarget,
    run: (ctx, s) =>
      ctx.targets.forEach((t) => s.email.toggleFlag(toggleSubject(t))),
  },
  {
    id: 'message.archive',
    section: 'move',
    title: 'Archive',
    icon: Archive,
    keywords: 'archive',
    surfaces: ['context-menu'],
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
    surfaces: ['context-menu'],
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
    surfaces: ['context-menu'],
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
    keywords: 'delete permanently destroy',
    surfaces: ['context-menu'],
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
    surfaces: ['context-menu'],
    // Every target must be a draft (D127: hard delete via the draft-delete op).
    isAvailable: (ctx) =>
      ctx.targets.length > 0 && ctx.targets.every((t) => t.isDraft),
    isEnabled: requireTarget,
    run: (ctx, s) =>
      ctx.targets.forEach((t) =>
        s.email.discardDraft({ ...t.ref, draftId: t.draftId }),
      ),
  },
]

registerActions(messageActions)
