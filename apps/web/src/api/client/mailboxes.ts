import { jsonRequest, request } from './core'

import type {
  CreateMailboxInput,
  DeleteMailboxInput,
  Mailbox,
  PatchMailboxInput,
} from '../types'

/** @spec docs/L1-api#endpoint-table */
export async function fetchMailboxes(accountId: string): Promise<Mailbox[]> {
  return request<Mailbox[]>(
    `/sources/${encodeURIComponent(accountId)}/mailboxes`,
  )
}

/**
 * Create a new top-level mailbox on a source; returns the refreshed list.
 * @spec docs/eph/RFC-L2-mailbox-management
 */
export async function createMailbox(
  accountId: string,
  input: CreateMailboxInput,
): Promise<Mailbox[]> {
  return jsonRequest<Mailbox[]>(
    `/sources/${encodeURIComponent(accountId)}/mailboxes`,
    'POST',
    input,
  )
}

/**
 * Delete a mailbox; returns the source's refreshed list. `removeEmails` is the
 * confirm-with-count safety flag — omitting it (false) makes the server refuse a
 * non-empty mailbox with 409 `mailbox_not_empty`.
 * @spec docs/eph/RFC-L2-mailbox-management
 */
export async function deleteMailbox(
  accountId: string,
  mailboxId: string,
  input: DeleteMailboxInput,
): Promise<Mailbox[]> {
  const query = input.removeEmails ? '?removeEmails=true' : ''
  return request<Mailbox[]>(
    `/sources/${encodeURIComponent(accountId)}/mailboxes/${encodeURIComponent(mailboxId)}${query}`,
    { method: 'DELETE' },
  )
}

/** @spec docs/L1-api#endpoint-table */
export async function patchMailbox(
  accountId: string,
  mailboxId: string,
  input: PatchMailboxInput,
): Promise<Mailbox[]> {
  return jsonRequest<Mailbox[]>(
    `/sources/${encodeURIComponent(accountId)}/mailboxes/${encodeURIComponent(mailboxId)}`,
    'PATCH',
    input,
  )
}
