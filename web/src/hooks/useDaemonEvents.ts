import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { buildEventsUrl } from "../api/client";
import type { DomainEvent } from "../api/types";
import {
  applyKeywordEventPatch,
  findConversationIdForMessage,
  mailKeys,
  shouldSuppressLocalEcho,
} from "../mailState";

const EVENT_CURSOR_STORAGE_KEY = "mail:last-event-seq";
export const MAIL_DOMAIN_EVENT_NAME = "mail:domain-event";

function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === "string");
}

function payloadConversationId(payload: DomainEvent["payload"]): string | null {
  return typeof payload.conversationId === "string" ? payload.conversationId : null;
}

function dispatchDomainEvent(payload: DomainEvent) {
  window.dispatchEvent(
    new CustomEvent<DomainEvent>(MAIL_DOMAIN_EVENT_NAME, { detail: payload }),
  );
}

export function useDaemonEvents() {
  const queryClient = useQueryClient();

  useEffect(() => {
    const storedSeq = window.sessionStorage.getItem(EVENT_CURSOR_STORAGE_KEY);
    const afterSeq = storedSeq ? Number.parseInt(storedSeq, 10) : null;
    const source = new EventSource(
      buildEventsUrl({ afterSeq: Number.isFinite(afterSeq) ? afterSeq : null }),
    );

    source.onmessage = (event) => {
      let payload: DomainEvent;
      try {
        payload = JSON.parse(event.data) as DomainEvent;
      } catch (error) {
        console.warn("Ignoring malformed daemon event", error, event.data);
        return;
      }

      window.sessionStorage.setItem(EVENT_CURSOR_STORAGE_KEY, String(payload.seq));

      if (shouldSuppressLocalEcho(payload)) {
        return;
      }

      const target =
        payload.messageId && payload.accountId
          ? { messageId: payload.messageId, sourceId: payload.accountId }
          : null;

      switch (payload.topic) {
        case "message.arrived": {
          queryClient.invalidateQueries({ queryKey: ["sidebar"] });
          queryClient.invalidateQueries({ queryKey: ["smart-mailboxes"] });
          dispatchDomainEvent(payload);
          break;
        }
        case "message.keywords_changed": {
          queryClient.invalidateQueries({ queryKey: ["sidebar"] });
          queryClient.invalidateQueries({ queryKey: ["smart-mailboxes"] });

          const keywords = payload.payload.keywords;
          const patched =
            target && isStringArray(keywords)
              ? applyKeywordEventPatch(queryClient, target, keywords)
              : false;

          if (target && !patched) {
            queryClient.invalidateQueries({
              queryKey: mailKeys.message(target.sourceId, target.messageId),
            });
            const conversationId = findConversationIdForMessage(queryClient, target);
            if (conversationId) {
              queryClient.invalidateQueries({
                queryKey: mailKeys.conversation(conversationId),
              });
              queryClient.invalidateQueries({
                queryKey: mailKeys.conversationSummary(conversationId),
              });
            }
          }

          dispatchDomainEvent(payload);
          break;
        }
        case "message.mailboxes_changed": {
          queryClient.invalidateQueries({ queryKey: ["sidebar"] });
          queryClient.invalidateQueries({ queryKey: ["smart-mailboxes"] });
          if (target) {
            queryClient.invalidateQueries({
              queryKey: mailKeys.message(target.sourceId, target.messageId),
            });
            const conversationId = findConversationIdForMessage(queryClient, target);
            if (conversationId) {
              queryClient.invalidateQueries({
                queryKey: mailKeys.conversation(conversationId),
              });
              queryClient.invalidateQueries({
                queryKey: mailKeys.conversationSummary(conversationId),
              });
            }
          }
          dispatchDomainEvent(payload);
          break;
        }
        case "message.updated": {
          if (target) {
            queryClient.invalidateQueries({
              queryKey: mailKeys.message(target.sourceId, target.messageId),
            });
            const conversationId =
              payloadConversationId(payload.payload) ??
              findConversationIdForMessage(queryClient, target);
            if (conversationId) {
              queryClient.invalidateQueries({
                queryKey: mailKeys.conversation(conversationId),
              });
              queryClient.invalidateQueries({
                queryKey: mailKeys.conversationSummary(conversationId),
              });
            }
          }
          dispatchDomainEvent(payload);
          break;
        }
        default: {
          dispatchDomainEvent(payload);
        }
      }
    };

    return () => {
      source.close();
    };
  }, [queryClient]);
}
