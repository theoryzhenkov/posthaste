/**
 * Sync progress presentation — the single client-side mapping from the
 * supervisor's live `SyncProgress` into one user-facing line and, when the
 * provider gives us enough to be honest about it, a determinate fraction.
 *
 * The backend already phrases `detail` for people ("Fetching mailbox",
 * "Planning mailbox sync"), so this does not re-word it; it appends which
 * mailbox the cycle is on and where that sits in the run. `detail` is a plain
 * string over the wire, so an empty one falls back to a phrase derived from
 * the closed `stage` vocabulary.
 *
 * Counts are provider-dependent and that asymmetry is deliberate: the IMAP
 * gateway walks mailboxes and reports index/count, so its bar is determinate,
 * while the JMAP gateway reports stage and detail only. When the position is
 * absent `percent` is null and the bar stays indeterminate rather than
 * inventing motion the sync cannot back up.
 */
import type { SyncProgress, SyncProgressStage } from '@/gen'

export interface SyncProgressView {
  /** One line, e.g. `Fetching mailbox — Inbox (2 of 7)`. */
  label: string
  /** 0–100 when the provider reports mailbox position, otherwise null. */
  percent: number | null
}

/** Used only when the server sends an empty `detail`. */
const STAGE_LABEL: Record<SyncProgressStage, string> = {
  connecting: 'Connecting',
  discovering: 'Checking for changes',
  planning: 'Planning the sync',
  fetching: 'Fetching mail',
  storing: 'Saving mail',
  waiting: 'Waiting for the server',
}

/**
 * Mailbox position as a percentage, or null when the provider does not report
 * one. `mailboxIndex` is 1-based, so the fraction matches the "(2 of 7)" text
 * the label shows rather than trailing it by one.
 */
function mailboxPercent(progress: SyncProgress): number | null {
  const { mailboxIndex, mailboxCount } = progress
  if (
    mailboxIndex === undefined ||
    mailboxCount === undefined ||
    // A count of zero has no fraction to show, and a bad index would render a
    // bar that moves backwards or overflows the track.
    mailboxCount <= 0 ||
    mailboxIndex <= 0 ||
    mailboxIndex > mailboxCount
  ) {
    return null
  }
  return Math.round((mailboxIndex / mailboxCount) * 100)
}

export function syncProgressView(progress: SyncProgress): SyncProgressView {
  const detail = progress.detail.trim() || STAGE_LABEL[progress.stage]
  const parts = [detail]

  if (progress.mailboxName) {
    parts.push(progress.mailboxName)
  }

  const label = parts.join(' — ')
  const { mailboxIndex, mailboxCount } = progress
  const position =
    mailboxIndex !== undefined && mailboxCount !== undefined && mailboxCount > 0
      ? ` (${mailboxIndex} of ${mailboxCount})`
      : ''

  return { label: `${label}${position}`, percent: mailboxPercent(progress) }
}
