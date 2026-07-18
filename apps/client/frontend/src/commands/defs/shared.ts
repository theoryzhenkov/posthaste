/**
 * Predicates and accessors shared by the definition files (R1: second consumer
 * moved them here). All pure over `(ctx, services)`.
 */
import type { ListUnsubscribe, MessageDetail } from '../../data/transport/api/index'
import { MAILBOX_ROLES } from '../../domain/vocabulary'
import type { ActionContext, MessageTarget } from '../types'

/** Roles from which a message is "removed" and can be restored to the inbox. */
export function isRestorableRole(role: string | null): boolean {
  return (
    role === MAILBOX_ROLES.Trash ||
    role === MAILBOX_ROLES.Archive ||
    role === MAILBOX_ROLES.Junk
  )
}

/** The subject a single-target action operates on. `?? undefined` keeps the
 *  accessors total for the empty case. */
export function primaryTarget(ctx: ActionContext): MessageTarget | undefined {
  return ctx.targets[0]
}

/** The target's parsed List-Unsubscribe data. Only the DETAIL DTO carries it
 *  (`listUnsubscribe`), so a plain list-row summary yields `undefined` — which
 *  is the availability gate working as designed. */
export function unsubscribeTargets(
  ctx: ActionContext,
): ListUnsubscribe | undefined {
  const summary = primaryTarget(ctx)?.summary as
    | Partial<Pick<MessageDetail, 'listUnsubscribe'>>
    | undefined
  return summary?.listUnsubscribe ?? undefined
}

/** The subject passed to keyword-toggle handlers. Prefer the summary (carries
 *  the keyword state the handler derives from); fall back to a `MailSelection`
 *  built from the ref so the handler's cache path resolves state. */
export function toggleSubject(t: MessageTarget) {
  return t.summary ?? { ...t.ref, conversationId: t.conversationId ?? '' }
}

/** Base enablement shared by every message action: at least one target. In the
 *  context menu a target always exists, so these stay enabled — matching today.
 *  It exists so the palette can render "select a message first". */
export function requireTarget(ctx: ActionContext) {
  return ctx.targets.length > 0 || { reason: 'Select a message first' }
}

export function hasDraftTarget(ctx: ActionContext): boolean {
  return ctx.targets.some((t) => t.isDraft)
}

/** The detail header shows a draft-appropriate action set (edit + discard,
 *  never reply/move/flag). Everywhere else the action keeps its own gating —
 *  the context menu / palette still offer e.g. flagging a draft. */
export function notDraftOnHeader(ctx: ActionContext): boolean {
  return !(ctx.surface === 'detail-header' && hasDraftTarget(ctx))
}
