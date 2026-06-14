import { jsonRequest, request } from './core'

import type { Mailbox, PatchMailboxInput } from '../types'

/** @spec docs/L1-api#endpoint-table */
export async function fetchMailboxes(accountId: string): Promise<Mailbox[]> {
  return request<Mailbox[]>(
    `/sources/${encodeURIComponent(accountId)}/mailboxes`,
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
