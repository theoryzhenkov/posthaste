/**
 * Map an error from a query/mutation into a user-facing notification.
 *
 * Keeps the notification center comprehensive without flooding it: errors are
 * deduplicated by code/message, plainly-expected outcomes (not found) are
 * skipped, and database corruption gets a dedicated repair action.
 */
import { ApiError } from '@/api/errors'
import {
  canRepairLocalDatabase,
  repairLocalDatabaseAndRestart,
} from '@/desktopRepair'

import { pushNotification } from './store'

function errorCode(error: unknown): string | undefined {
  return error instanceof ApiError ? error.code : undefined
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

  if (code === 'storage_corrupted') {
    pushNotification({
      severity: 'error',
      dedupeKey: 'storage_corrupted',
      title: 'Local database is corrupted',
      message:
        'Posthaste can rebuild the local cache and re-sync. Your accounts and passwords are safe.',
      action: canRepairLocalDatabase()
        ? { label: 'Repair & restart', run: repairLocalDatabaseAndRestart }
        : undefined,
    })
    return
  }

  // Expected, non-actionable outcomes that should not appear as notifications.
  if (code === 'not_found') {
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
