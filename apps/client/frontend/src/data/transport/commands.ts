// Command verbs over POST /api/command. Every write goes through `runCommand`:
// the facade posts the typed intent with an idempotency id, and on acceptance
// every query react-query holds is invalidated so answers catch up to the
// returned generation — rows change because the backend's answer changed,
// never because the client edited a cache.

import { useQueryClient, type QueryClient } from '@tanstack/react-query'
import { useMemo } from 'react'
import { newId, type MailClient } from '@/data/transport/client'
import type {
  AccountId,
  AppSettings,
  Command,
  CommandAccepted,
  MailboxId,
  MessageId,
  SendMessageRequest,
} from '@/gen'
import { useMailClient } from '../context'

export async function runCommand(
  client: MailClient,
  queryClient: QueryClient,
  command: Command,
  id: string = newId(),
): Promise<CommandAccepted> {
  const accepted = await client.command(command, id)
  void queryClient.invalidateQueries()
  return accepted
}

/** The bound verb set components call; stable per client+queryClient pair. */
export function useCommands() {
  const client = useMailClient()
  const queryClient = useQueryClient()
  return useMemo(() => makeCommands(client, queryClient), [client, queryClient])
}

function makeCommands(client: MailClient, queryClient: QueryClient) {
  const run = (command: Command, id?: string) =>
    runCommand(client, queryClient, command, id)

  const setKeywords = (
    accountId: AccountId,
    messageId: MessageId,
    add: string[],
    remove: string[],
    opts?: {
      /** `false` keeps an IMPLICIT gesture (the auto-mark-read on opening a
       *  message) out of the account's undo history, so a deliberate action's
       *  toast/shell Undo is never hijacked by its read-state side effect. */
      recordUndo?: boolean
    },
  ) =>
    run({
      setKeywords: {
        accountId,
        messageId,
        change: { add, remove },
        ...(opts?.recordUndo === false ? { recordUndo: false } : {}),
      },
    })

  return {
    /** Escape hatch for intents without a dedicated verb yet. */
    run,

    setKeywords,
    markRead: (accountId: AccountId, messageId: MessageId) =>
      setKeywords(accountId, messageId, ['$seen'], []),
    markUnread: (accountId: AccountId, messageId: MessageId) =>
      setKeywords(accountId, messageId, [], ['$seen']),
    flag: (accountId: AccountId, messageId: MessageId) =>
      setKeywords(accountId, messageId, ['$flagged'], []),
    unflag: (accountId: AccountId, messageId: MessageId) =>
      setKeywords(accountId, messageId, [], ['$flagged']),

    /** Replaces the message's mailboxes outright (a move, not an add). */
    move: (accountId: AccountId, messageId: MessageId, mailboxIds: MailboxId[]) =>
      run({ replaceMailboxes: { accountId, messageId, change: { mailboxIds } } }),

    destroy: (accountId: AccountId, messageId: MessageId) =>
      run({ destroy: { accountId, messageId } }),

    createMailbox: (accountId: AccountId, name: string) =>
      run({ createMailbox: { accountId, name } }),
    renameMailbox: (accountId: AccountId, mailboxId: MailboxId, name: string) =>
      run({ renameMailbox: { accountId, mailboxId, name } }),
    /** Deletes a mailbox; `removeEmails` is the confirm-with-count safety
     * flag — a non-empty delete is refused with a conflict without it. */
    deleteMailbox: (
      accountId: AccountId,
      mailboxId: MailboxId,
      removeEmails: boolean,
    ) => run({ deleteMailbox: { accountId, mailboxId, removeEmails } }),
    setMailboxRole: (
      accountId: AccountId,
      mailboxId: MailboxId,
      role: string | null,
    ) => run({ setMailboxRole: { accountId, mailboxId, role } }),

    /** Moves the message to the account's archive mailbox, resolved by role. */
    archive: async (accountId: AccountId, messageId: MessageId) => {
      const accepted = await client.archive(accountId, messageId)
      void queryClient.invalidateQueries()
      return accepted
    },

    /** Moves the message to the account's trash mailbox, resolved by role. */
    trash: async (accountId: AccountId, messageId: MessageId) => {
      const accepted = await client.trash(accountId, messageId)
      void queryClient.invalidateQueries()
      return accepted
    },

    /** Moves the message to the account's mailbox carrying `role`. */
    moveToRole: async (accountId: AccountId, messageId: MessageId, role: string) => {
      const mailboxId = await client.mailboxWithRole(accountId, role)
      return run({
        replaceMailboxes: { accountId, messageId, change: { mailboxIds: [mailboxId] } },
      })
    },

    /** Submits the message; hold semantics (undo window, send-later) travel
     * inside the request. Returns the operation id to watch in
     * pending-operations. */
    send: async (
      accountId: AccountId,
      request: SendMessageRequest,
      opts?: { undoWindowSeconds?: number; sendAt?: string },
    ) => {
      const result = await client.send(accountId, request, opts)
      void queryClient.invalidateQueries()
      return result
    },

    /** Creates the draft on first save (minting its stable id), updates after. */
    saveDraft: async (accountId: AccountId, draft: SendMessageRequest) => {
      const result = await client.saveDraft(accountId, draft)
      void queryClient.invalidateQueries()
      return result
    },

    discardDraft: (accountId: AccountId, draftId: string) =>
      run({ discardDraft: { accountId, draftId } }),

    snooze: (accountId: AccountId, messageId: MessageId, until: string) =>
      run({ snooze: { accountId, messageId, until } }),
    unsnooze: (accountId: AccountId, messageId: MessageId) =>
      run({ unsnooze: { accountId, messageId } }),

    undo: (accountId: AccountId) => run({ undo: { accountId } }),
    redo: (accountId: AccountId) => run({ redo: { accountId } }),

    /** Writes the FULL settings document (read-modify-write against the
     * `appSettings` query). `forceBackfill` asks the backend to re-run
     * backfill-enabled automation rules against existing mail after saving. */
    updateSettings: (
      settings: AppSettings,
      opts?: { forceBackfill?: boolean },
    ) =>
      run({
        updateSettings: {
          settings,
          forceBackfill: opts?.forceBackfill ?? false,
        },
      }),
  }
}

export type MailCommands = ReturnType<typeof makeCommands>
