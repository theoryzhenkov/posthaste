import { useMutation, useQueryClient } from "@tanstack/react-query";
import { performMessageCommand } from "../api/client";
import type { Mailbox, MessageSummary } from "../api/types";

export type EmailActions = ReturnType<typeof useEmailActions>;

function requiredMailboxByRole(
  mailboxes: Mailbox[] | undefined,
  role: string,
): Mailbox {
  const mailbox = mailboxes?.find((candidate) => candidate.role === role);
  if (!mailbox) {
    throw new Error(`Missing mailbox with role ${role}`);
  }
  return mailbox;
}

export function useEmailActions(accountId: string, selectedMailboxId: string | null) {
  const queryClient = useQueryClient();

  const mutation = useMutation({
    mutationFn: ({
      emailId,
      command,
    }: {
      emailId: string;
      command:
        | { kind: "setKeywords"; add: string[]; remove: string[] }
        | { kind: "replaceMailboxes"; mailboxIds: string[] }
        | { kind: "destroy" };
    }) => performMessageCommand(emailId, command, accountId),
    onSuccess: (_data, { emailId }) => {
      queryClient.invalidateQueries({
        queryKey: ["messages", accountId, selectedMailboxId],
      });
      queryClient.invalidateQueries({ queryKey: ["message", accountId, emailId] });
      queryClient.invalidateQueries({ queryKey: ["mailboxes", accountId] });
    },
  });

  const mailboxes = queryClient.getQueryData<Mailbox[]>(["mailboxes", accountId]);

  return {
    markRead: (emailId: string) =>
      mutation.mutate({
        emailId,
        command: { kind: "setKeywords", add: ["$seen"], remove: [] },
      }),
    markUnread: (emailId: string) =>
      mutation.mutate({
        emailId,
        command: { kind: "setKeywords", add: [], remove: ["$seen"] },
      }),
    toggleRead: (email: MessageSummary) =>
      email.isRead
        ? mutation.mutate({
            emailId: email.id,
            command: { kind: "setKeywords", add: [], remove: ["$seen"] },
          })
        : mutation.mutate({
            emailId: email.id,
            command: { kind: "setKeywords", add: ["$seen"], remove: [] },
          }),
    flag: (emailId: string) =>
      mutation.mutate({
        emailId,
        command: { kind: "setKeywords", add: ["$flagged"], remove: [] },
      }),
    unflag: (emailId: string) =>
      mutation.mutate({
        emailId,
        command: { kind: "setKeywords", add: [], remove: ["$flagged"] },
      }),
    toggleFlag: (email: MessageSummary) =>
      email.isFlagged
        ? mutation.mutate({
            emailId: email.id,
            command: { kind: "setKeywords", add: [], remove: ["$flagged"] },
          })
        : mutation.mutate({
            emailId: email.id,
            command: { kind: "setKeywords", add: ["$flagged"], remove: [] },
          }),
    archive: (emailId: string) =>
      mutation.mutate({
        emailId,
        command: {
          kind: "replaceMailboxes",
          mailboxIds: [requiredMailboxByRole(mailboxes, "archive").id],
        },
      }),
    trash: (emailId: string) =>
      mutation.mutate({
        emailId,
        command: {
          kind: "replaceMailboxes",
          mailboxIds: [requiredMailboxByRole(mailboxes, "trash").id],
        },
      }),
    deletePermanently: (emailId: string) =>
      mutation.mutate({ emailId, command: { kind: "destroy" } }),
    isPending: mutation.isPending,
  };
}
