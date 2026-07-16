/**
 * New-mail OS notification policy — the ARRIVAL GATE between the domain-event
 * stream and the OS banner.
 *
 * When does a `message.updated` event become a banner?
 *
 *  - NEW ARRIVALS ONLY: `payload.changes.arrived === true` (the same wire flag
 *    the mailbox-count invalidation keys on) AND `payload.created === true`.
 *    A user's own mutation echo (mark-read, tag flip) carries no `arrived`
 *    flag; moving an EXISTING message into a mailbox flips `arrived` but not
 *    `created` — neither notifies.
 *  - Never for the user's own mail: a message that arrives already read
 *    (`$seen` — e.g. the self-sent copy appended to Sent) or as a draft
 *    (`$draft` — compose autosave) is skipped.
 *  - Never during an account's INITIAL sync: while the account has never
 *    completed a sync (`runtime.lastSyncAt == null`), every "arrival" is
 *    backfill, not news.
 *  - BURST-COALESCED: arrivals collect in a sliding debounce window (reusing
 *    the count-invalidation window constant, capped at
 *    {@link NEW_MAIL_MAX_COALESCE_MS} total deferral so a steady trickle still
 *    flushes); one flush posts ONE banner — a single sender + subject, or an
 *    "N new messages" summary.
 *  - BACKFILL-SUPPRESSED: a flush holding more than
 *    {@link NEW_MAIL_BACKFILL_THRESHOLD} messages is a sync storm (repair
 *    sync, big catch-up after downtime), not news — dropped entirely.
 *  - FOCUS-AWARE: no banner while the app window is focused (checked at flush
 *    time, when the banner would actually show).
 *  - TOGGLE-GATED: the NotificationsPane `newMail` toggle (default ON, like
 *    the pane's own fallback) gates delivery; the `sound` toggle rides along
 *    on the posted banner.
 *
 * The coordinator is pure policy with an injected notifier so the gate is
 * unit-testable; `notifyNewMailFromEvent` is the production wiring used by the
 * domain-cache `message.updated` handler.
 */
import type { QueryClient } from '@tanstack/react-query'

import type {
  AccountOverview,
  AppSettings,
  DomainEvent,
  MessageSummary,
  Notifications,
} from '../api/types'
import { COUNT_INVALIDATION_WINDOW_MS } from '../domain-cache/mailboxCounts'
import { queryKeys } from '../queryKeys'
import { postOsNotification } from './osNotifier'

/** Sliding burst window — the count-debounce pattern's window, reused. */
export const NEW_MAIL_BURST_WINDOW_MS = COUNT_INVALIDATION_WINDOW_MS

/** Hard cap on total coalescing deferral: a steady sub-window trickle must not
 * postpone the banner forever. */
export const NEW_MAIL_MAX_COALESCE_MS = 10 * NEW_MAIL_BURST_WINDOW_MS

/** A burst bigger than this is treated as sync backfill and never notifies. */
export const NEW_MAIL_BACKFILL_THRESHOLD = 25

/** How many "Sender — Subject" lines a multi-message summary body lists. */
const SUMMARY_BODY_LINES = 3

const MAX_BODY_LINE_LENGTH = 120

/** One OS banner, already formatted; `sound` mirrors the pane's Sounds toggle. */
export interface NewMailBanner {
  title: string
  body: string
  sound: boolean
}

export interface NewMailArrivalDeps {
  /** Deliver one banner (the OS notifier; mocked in tests). */
  post: (banner: NewMailBanner) => void
  /** Current pane prefs; absent fields default ON (the pane's own fallback). */
  getPreferences: () => Notifications | null | undefined
  /** Is the app window focused right now? Focused → no banner. */
  isAppFocused: () => boolean
  /** Has this account never completed a sync (initial backfill in flight)? */
  isAccountInInitialSync: (accountId: string) => boolean
  windowMs?: number
  maxCoalesceMs?: number
  backfillThreshold?: number
  now?: () => number
}

export interface NewMailArrivalCoordinator {
  onMessageUpdated: (event: DomainEvent) => void
  /** Drop any pending burst and its timer (window teardown / tests). */
  dispose: () => void
}

interface PendingArrival {
  sender: string | null
  subject: string | null
}

function changeFlag(payload: Record<string, unknown>, key: string): boolean {
  const changes = payload.changes
  return (
    typeof changes === 'object' &&
    changes !== null &&
    (changes as Record<string, unknown>)[key] === true
  )
}

/** Extract a notifiable arrival, or `null` when the event must not notify. */
function arrivalFromEvent(event: DomainEvent): PendingArrival | null {
  const payload = event.payload
  // Only a genuinely NEW message that just gained mailbox membership. Echoes
  // of the user's own mutations either lack `arrived` (keyword flips) or lack
  // `created` (moves of an existing message).
  if (payload.created !== true || !changeFlag(payload, 'arrived')) {
    return null
  }
  const keywords = Array.isArray(payload.keywords) ? payload.keywords : []
  if (keywords.includes('$seen') || keywords.includes('$draft')) {
    return null
  }
  const projection =
    typeof payload.projection === 'object' && payload.projection !== null
      ? (payload.projection as Partial<MessageSummary>)
      : null
  // Belt and braces for payloads carrying a projection but no flat keywords.
  if (projection?.isRead === true) {
    return null
  }
  return {
    sender: projection?.fromName ?? projection?.fromEmail ?? null,
    subject: projection?.subject ?? null,
  }
}

function truncate(text: string): string {
  return text.length > MAX_BODY_LINE_LENGTH
    ? `${text.slice(0, MAX_BODY_LINE_LENGTH - 1)}…`
    : text
}

/** Format one flushed burst as a single banner. */
function bannerForBurst(
  burst: readonly PendingArrival[],
  sound: boolean,
): NewMailBanner {
  if (burst.length === 1) {
    const [arrival] = burst
    return {
      title: arrival.sender ?? 'New mail',
      body: truncate(arrival.subject ?? '(no subject)'),
      sound,
    }
  }
  const lines = burst
    .slice(0, SUMMARY_BODY_LINES)
    .map((arrival) =>
      truncate(
        `${arrival.sender ?? 'Unknown sender'} — ${arrival.subject ?? '(no subject)'}`,
      ),
    )
  if (burst.length > SUMMARY_BODY_LINES) {
    lines.push(`…and ${burst.length - SUMMARY_BODY_LINES} more`)
  }
  return {
    title: `${burst.length} new messages`,
    body: lines.join('\n'),
    sound,
  }
}

export function createNewMailArrivalCoordinator(
  deps: NewMailArrivalDeps,
): NewMailArrivalCoordinator {
  const windowMs = deps.windowMs ?? NEW_MAIL_BURST_WINDOW_MS
  const maxCoalesceMs = deps.maxCoalesceMs ?? NEW_MAIL_MAX_COALESCE_MS
  const backfillThreshold =
    deps.backfillThreshold ?? NEW_MAIL_BACKFILL_THRESHOLD
  const now = deps.now ?? Date.now

  let pending: PendingArrival[] = []
  let timer: ReturnType<typeof setTimeout> | null = null
  let windowOpenedAt: number | null = null

  function clearTimer() {
    if (timer !== null) {
      clearTimeout(timer)
      timer = null
    }
  }

  function flush() {
    clearTimer()
    windowOpenedAt = null
    const burst = pending
    pending = []
    if (burst.length === 0) {
      return
    }
    const prefs = deps.getPreferences()
    if ((prefs?.newMail ?? true) === false) {
      return
    }
    // Focus is checked when the banner would show, not when the burst opened.
    if (deps.isAppFocused()) {
      return
    }
    // A storm this size is backfill (repair sync / catch-up), never news.
    if (burst.length > backfillThreshold) {
      return
    }
    deps.post(bannerForBurst(burst, prefs?.sound ?? true))
  }

  return {
    onMessageUpdated(event) {
      // Master-toggle check up front so a disabled pane accumulates nothing.
      if ((deps.getPreferences()?.newMail ?? true) === false) {
        return
      }
      const arrival = arrivalFromEvent(event)
      if (arrival === null) {
        return
      }
      if (deps.isAccountInInitialSync(event.accountId)) {
        return
      }
      pending.push(arrival)
      const at = now()
      windowOpenedAt ??= at
      clearTimer()
      if (at - windowOpenedAt >= maxCoalesceMs) {
        flush()
        return
      }
      timer = setTimeout(flush, windowMs)
    },
    dispose() {
      clearTimer()
      pending = []
      windowOpenedAt = null
    },
  }
}

// ---------------------------------------------------------------------------
// Production wiring (domain-cache `message.updated` handler entry point).
// ---------------------------------------------------------------------------

// Per-queryClient coordinator (WeakMap: a disposed test QueryClient drops its
// coordinator with it) — same lifetime pattern as the count-debounce windows.
const coordinators = new WeakMap<QueryClient, NewMailArrivalCoordinator>()

function isAppWindowFocused(): boolean {
  // The listener runs in the main window's renderer; a focused secondary
  // Posthaste window (compose, settings) still counts as "unfocused" here and
  // may banner — acceptable standard behavior, noted in the pane docs.
  return (
    typeof document !== 'undefined' &&
    document.visibilityState === 'visible' &&
    document.hasFocus()
  )
}

function defaultDeps(queryClient: QueryClient): NewMailArrivalDeps {
  return {
    post: postOsNotification,
    getPreferences: () =>
      queryClient.getQueryData<AppSettings>(queryKeys.settings)?.notifications,
    isAppFocused: isAppWindowFocused,
    isAccountInInitialSync: (accountId) => {
      const accounts = queryClient.getQueryData<AccountOverview[]>(
        queryKeys.accounts,
      )
      const account = accounts?.find((entry) => entry.id === accountId)
      // Unknown account (cold cache): do not suppress — a missed suppression
      // is one stray banner; the burst threshold still catches storms.
      return account ? account.runtime.lastSyncAt === null : false
    },
  }
}

/**
 * Feed one `message.updated` event through the arrival gate, lazily creating
 * the per-queryClient coordinator with the production deps.
 */
export function notifyNewMailFromEvent(
  queryClient: QueryClient,
  event: DomainEvent,
): void {
  let coordinator = coordinators.get(queryClient)
  if (!coordinator) {
    coordinator = createNewMailArrivalCoordinator(defaultDeps(queryClient))
    coordinators.set(queryClient, coordinator)
  }
  coordinator.onMessageUpdated(event)
}
