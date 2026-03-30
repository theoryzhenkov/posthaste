import { useMutation, useQueryClient } from "@tanstack/react-query";
import { performEmailAction } from "../api/client";
import type { Email, EmailAction } from "../api/types";

export type EmailActions = ReturnType<typeof useEmailActions>;

export function useEmailActions(selectedMailboxId: string | null) {
  const queryClient = useQueryClient();

  const mutation = useMutation({
    mutationFn: ({
      emailId,
      action,
    }: {
      emailId: string;
      action: EmailAction;
    }) => performEmailAction(emailId, action),
    onSuccess: (_data, { emailId }) => {
      queryClient.invalidateQueries({
        queryKey: ["emails", selectedMailboxId],
      });
      queryClient.invalidateQueries({ queryKey: ["email", emailId] });
      queryClient.invalidateQueries({ queryKey: ["mailboxes"] });
    },
  });

  return {
    markRead: (emailId: string) =>
      mutation.mutate({ emailId, action: { action: "markRead" } }),
    markUnread: (emailId: string) =>
      mutation.mutate({ emailId, action: { action: "markUnread" } }),
    toggleRead: (email: Email) =>
      email.isRead
        ? mutation.mutate({ emailId: email.id, action: { action: "markUnread" } })
        : mutation.mutate({ emailId: email.id, action: { action: "markRead" } }),
    flag: (emailId: string) =>
      mutation.mutate({ emailId, action: { action: "flag" } }),
    unflag: (emailId: string) =>
      mutation.mutate({ emailId, action: { action: "unflag" } }),
    toggleFlag: (email: Email) =>
      email.isFlagged
        ? mutation.mutate({ emailId: email.id, action: { action: "unflag" } })
        : mutation.mutate({ emailId: email.id, action: { action: "flag" } }),
    archive: (emailId: string) =>
      mutation.mutate({ emailId, action: { action: "archive" } }),
    trash: (emailId: string) =>
      mutation.mutate({ emailId, action: { action: "trash" } }),
    deletePermanently: (emailId: string) =>
      mutation.mutate({ emailId, action: { action: "delete" } }),
    isPending: mutation.isPending,
  };
}
