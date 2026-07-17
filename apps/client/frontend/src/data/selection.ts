// Ephemeral selection types shared by list, detail, palette, and keyboard
// surfaces. This is UI state — what the user has focused — not mail state;
// everything mail-shaped behind these references is a query answer.

/** Selected message reference used by list and detail views. */
export type MailSelection = {
  sourceId: string
  messageId: string
  conversationId: string
}

/** Current sidebar selection — either a smart mailbox or a source+mailbox pair. */
export type MailViewSelection =
  | { kind: 'smart-mailbox'; id: string }
  | { kind: 'source-mailbox'; sourceId: string; mailboxId: string }
  | null
