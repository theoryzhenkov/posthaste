import { useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { fetchConversation, fetchMessage } from "../api/client";
import type { MessageSummary, SourceMessageRef } from "../api/types";
import { cn } from "../lib/utils";
import { mergeConversationView } from "../mailState";
import { formatRelativeTime } from "../utils/relativeTime";
import { EmailFrame } from "./EmailFrame";

interface MessageSelection extends SourceMessageRef {
  conversationId: string;
}

interface MessageDetailProps {
  selection: MessageSelection | null;
  onSelectMessage: (message: MessageSummary) => void;
}

/** Filter out JMAP system keywords (starting with $). */
function userTags(keywords: string[]): string[] {
  return keywords.filter((kw) => !kw.startsWith("$"));
}

function dedupeConversationMessages(messages: MessageSummary[]): MessageSummary[] {
  const uniqueMessages = new Map<string, MessageSummary>();
  for (const message of messages) {
    uniqueMessages.set(`${message.sourceId}:${message.id}`, message);
  }
  return [...uniqueMessages.values()].sort((left, right) => {
    if (left.receivedAt !== right.receivedAt) {
      return left.receivedAt.localeCompare(right.receivedAt);
    }
    return left.id.localeCompare(right.id);
  });
}

export function MessageDetail({
  selection,
  onSelectMessage,
}: MessageDetailProps) {
  const queryClient = useQueryClient();
  const conversationQuery = useQuery({
    queryKey: ["conversation", selection?.conversationId],
    queryFn: () => fetchConversation(selection!.conversationId),
    enabled: selection !== null,
  });

  const messageQuery = useQuery({
    queryKey: ["message", selection?.sourceId, selection?.messageId],
    queryFn: () => fetchMessage(selection!.messageId, selection!.sourceId),
    enabled: selection !== null,
  });

  useEffect(() => {
    if (!conversationQuery.data) {
      return;
    }
    mergeConversationView(queryClient, conversationQuery.data);
  }, [conversationQuery.data, queryClient]);

  if (!selection) {
    return (
      <div className="flex items-center justify-center bg-background">
        <p className="text-sm text-muted-foreground">Select a message</p>
      </div>
    );
  }

  if (conversationQuery.isLoading || messageQuery.isLoading) {
    return (
      <div className="flex items-center justify-center bg-background">
        <p className="text-sm text-muted-foreground">Loading...</p>
      </div>
    );
  }

  const conversation = conversationQuery.data;
  const message = messageQuery.data;

  if (conversationQuery.error || messageQuery.error || !conversation || !message) {
    return (
      <div className="flex items-center justify-center bg-background">
        <p className="text-sm text-destructive">Failed to load conversation</p>
      </div>
    );
  }

  const senderName = message.fromName ?? message.fromEmail ?? "Unknown sender";
  const senderEmail = message.fromEmail ?? "";
  const tags = userTags(message.keywords);
  const threadMessages = dedupeConversationMessages(conversation.messages);

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden bg-background">
      {/* Sticky metadata header */}
      <div className="shrink-0 border-b border-border px-4 py-3 space-y-2">
        {/* Subject */}
        <h2 className="text-base font-semibold leading-tight tracking-tight">
          {conversation.subject ?? message.subject ?? "(no subject)"}
        </h2>

        {/* From + date */}
        <div className="flex items-baseline justify-between gap-4">
          <div className="min-w-0">
            <span className="text-sm font-medium">{senderName}</span>
            {senderEmail && senderName !== senderEmail && (
              <span className="ml-1.5 text-xs text-muted-foreground">&lt;{senderEmail}&gt;</span>
            )}
          </div>
          <span className="shrink-0 font-mono text-xs tabular-nums text-muted-foreground">
            {formatRelativeTime(message.receivedAt)}
          </span>
        </div>

        {/* Tags */}
        {tags.length > 0 && (
          <div className="flex flex-wrap gap-1.5">
            {tags.map((tag) => (
              <span
                className="rounded border border-border px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wider text-muted-foreground"
                key={tag}
              >
                {tag}
              </span>
            ))}
          </div>
        )}

        {/* Thread switcher */}
        {threadMessages.length > 1 && (
          <div className="flex flex-wrap gap-1.5 border-t border-border pt-2">
            {threadMessages.map((item, index) => (
              <button
                key={`${item.sourceId}:${item.id}`}
                className={cn(
                  "rounded border border-border px-2 py-1 text-left text-xs transition-colors",
                  "hover:bg-accent",
                  item.sourceId === selection.sourceId &&
                    item.id === selection.messageId &&
                    "bg-accent border-primary",
                )}
                onClick={() => onSelectMessage(item)}
                type="button"
              >
                <span className="mr-1.5 font-mono text-[10px] text-muted-foreground">
                  #{index + 1}
                </span>
                <span className="font-medium">
                  {item.fromName ?? item.fromEmail ?? "Unknown"}
                </span>
                <span className="ml-1.5 font-mono text-[10px] tabular-nums text-muted-foreground">
                  {formatRelativeTime(item.receivedAt)}
                </span>
              </button>
            ))}
          </div>
        )}
      </div>

      {/* Email body */}
      <div className="min-h-0 flex-1 overflow-hidden p-4">
        {message.bodyHtml ? (
          <div className="h-full overflow-hidden border border-border bg-card">
            <EmailFrame html={message.bodyHtml} />
          </div>
        ) : message.bodyText ? (
          <pre className="h-full overflow-auto whitespace-pre-wrap border border-border p-4 font-mono text-sm leading-relaxed text-foreground/90">
            {message.bodyText}
          </pre>
        ) : (
          <p className="h-full overflow-auto border border-border p-4 text-sm text-muted-foreground">
            {message.preview ?? "No content available."}
          </p>
        )}
      </div>
    </div>
  );
}
