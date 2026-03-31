import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { buildEventsUrl } from "../api/client";
import type { DomainEvent } from "../api/types";

export function useDaemonEvents(accountId: string) {
  const queryClient = useQueryClient();

  useEffect(() => {
    const source = new EventSource(buildEventsUrl(accountId));
    source.onmessage = (event) => {
      const payload = JSON.parse(event.data) as DomainEvent;
      queryClient.invalidateQueries({ queryKey: ["mailboxes", accountId] });
      queryClient.invalidateQueries({ queryKey: ["messages", accountId] });
      if (payload.messageId) {
        queryClient.invalidateQueries({
          queryKey: ["message", accountId, payload.messageId],
        });
      }
    };
    return () => source.close();
  }, [accountId, queryClient]);
}
