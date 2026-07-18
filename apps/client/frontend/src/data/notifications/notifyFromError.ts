/**
 * Map an error from a query/mutation into a user-facing notification.
 *
 * Keeps the notification center comprehensive without flooding it: errors are
 * deduplicated by kind/message, and plainly-expected outcomes (unknown id)
 * are skipped. The typed kinds are the `ApiErrorKind` vocabulary carried by
 * `MailApiError`; there is no local-storage-corruption kind — the backend
 * owns all mail state, so a failing store surfaces as `unavailable`/
 * `internal` like any other backend fault.
 */
import { MailApiError } from '@/data/transport/client'

import { pushNotification } from './store'

function errorCode(error: unknown): string | undefined {
  return error instanceof MailApiError ? error.kind : undefined
}

function errorMessage(error: unknown): string {
  if (error instanceof Error && error.message) {
    return error.message
  }
  return String(error)
}

/** Push a notification for a failed operation, if it is worth surfacing. */
export function notifyFromError(error: unknown, context?: string): void {
  const code = errorCode(error)

  // Expected, non-actionable outcomes that should not appear as notifications.
  if (code === 'unknownId') {
    return
  }

  const message = errorMessage(error)
  pushNotification({
    severity: 'error',
    dedupeKey: `error:${code ?? 'unknown'}:${context ?? ''}:${message}`,
    title: context ?? 'Something went wrong',
    message,
  })
}
