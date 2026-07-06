import { jsonRequest, request } from './core'

import type { CreateMailboxInput, Mailbox, PatchMailboxInput } from '../types'

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
