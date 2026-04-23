/**
 * Right-pane message detail: metadata header, thread switcher, and email body.
 *
 * Loads both the conversation (for the thread switcher) and the selected
 * message detail (for the body). Messages are deduped by `(sourceId, messageId)`.
 *
 * @spec docs/L1-ui#messagedetail-and-emailframe
 */
import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertCircle, Mail, Paperclip } from "lucide-react";
import { buildMessageAttachmentUrl, fetchConversation, fetchMessage } from "../api/client";
import type { MessageAttachment, MessageSummary, SourceMessageRef } from "../api/types";
import { cn } from "../lib/utils";
import { mergeConversationView } from "../mailState";
import { formatRelativeTime } from "../utils/relativeTime";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Separator } from "./ui/separator";
import { EmailFrame } from "./EmailFrame";

/** @spec docs/L1-ui#messagedetail-and-emailframe */
interface MessageSelection extends SourceMessageRef {
  conversationId: string;
}

/** @spec docs/L1-ui#messagedetail-and-emailframe */
interface MessageDetailProps {
  selection: MessageSelection | null;
  onSelectMessage: (message: MessageSummary) => void;
  onSearch?: (query: string, append?: boolean) => void;
}

/** Filter out JMAP system keywords (starting with $). */
function userTags(keywords: string[]): string[] {
  return keywords.filter((kw) => !kw.startsWith("$"));
}

/**
 * Deduplicate and sort conversation messages by `(sourceId, messageId)`,
 * ordered by `receivedAt`.
 * @spec docs/L1-ui#messagedetail-and-emailframe
 */
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

function canPreviewAttachment(attachment: MessageAttachment): boolean {
  return (
    attachment.mimeType.startsWith("image/") ||
    attachment.mimeType === "application/pdf" ||
    attachment.mimeType.startsWith("text/")
  );
}

function formatAttachmentSize(size: number): string {
  if (size < 1024) {
    return `${size} B`;
  }
  if (size < 1024 * 1024) {
    return `${(size / 1024).toFixed(1)} KB`;
  }
  return `${(size / (1024 * 1024)).toFixed(1)} MB`;
}

function AttachmentPreview({
  attachment,
  messageId,
  sourceId,
}: {
  attachment: MessageAttachment;
  messageId: string;
  sourceId: string;
}) {
  const attachmentUrl = buildMessageAttachmentUrl(sourceId, messageId, attachment.id);

  if (attachment.mimeType.startsWith("image/")) {
    return (
      <div className="flex h-full items-center justify-center bg-card">
        <img
          alt={attachment.filename ?? "Attachment preview"}
          className="max-h-full max-w-full object-contain"
          src={attachmentUrl}
        />
      </div>
    );
  }

  return (
    <iframe
      className="h-full w-full border-0 bg-card"
      src={attachmentUrl}
      title={attachment.filename ?? "Attachment preview"}
    />
  );
}

/**
 * Message detail pane with sticky header, thread switcher, and email body.
 *
 * @spec docs/L1-ui#messagedetail-and-emailframe
 */
export function MessageDetail({
  selection,
  onSelectMessage,
  onSearch,
}: MessageDetailProps) {
  const queryClient = useQueryClient();
  const selectionKey = selection ? `${selection.sourceId}:${selection.messageId}` : null;
  const [attachmentSelection, setAttachmentSelection] = useState<{
    attachmentId: string | null;
    selectionKey: string | null;
  }>({ attachmentId: null, selectionKey: null });
  const selectedAttachmentId =
    attachmentSelection.selectionKey === selectionKey
      ? attachmentSelection.attachmentId
      : null;
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
      <div className="flex h-full flex-col items-center justify-center gap-3 bg-background">
        <Mail size={40} strokeWidth={1.5} className="text-muted-foreground/40" />
        <div className="text-center">
          <p className="text-sm font-medium text-muted-foreground">No conversation selected</p>
          <p className="mt-1 text-xs text-muted-foreground/60">Select a conversation to read it</p>
        </div>
      </div>
    );
  }

  if (conversationQuery.isLoading || messageQuery.isLoading) {
    return (
      <div className="flex h-full flex-col bg-background">
        <div className="shrink-0 border-b border-border px-4 py-3 space-y-3">
          <div className="h-5 w-3/4 animate-pulse rounded bg-muted" />
          <div className="flex items-center gap-3">
            <div className="h-3.5 w-32 animate-pulse rounded bg-muted" />
            <div className="h-3 w-20 animate-pulse rounded bg-muted/60" />
          </div>
        </div>
        <div className="flex-1 p-4 space-y-3">
          <div className="h-3 w-full animate-pulse rounded bg-muted/60" />
          <div className="h-3 w-5/6 animate-pulse rounded bg-muted/60" />
          <div className="h-3 w-4/6 animate-pulse rounded bg-muted/40" />
          <div className="h-3 w-3/4 animate-pulse rounded bg-muted/40" />
        </div>
      </div>
    );
  }

  const conversation = conversationQuery.data;
  const message = messageQuery.data;

  if (conversationQuery.error || messageQuery.error || !conversation || !message) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 bg-background">
        <AlertCircle size={32} strokeWidth={1.5} className="text-destructive/50" />
        <p className="text-sm text-destructive">Failed to load conversation</p>
        <button
          type="button"
          className="rounded border border-border px-3 py-1.5 text-xs text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          onClick={() => {
            void conversationQuery.refetch();
            void messageQuery.refetch();
          }}
        >
          Try again
        </button>
      </div>
    );
  }

  const senderName = message.fromName ?? message.fromEmail ?? "Unknown sender";
  const senderEmail = message.fromEmail ?? "";
  const tags = userTags(message.keywords);
  const threadMessages = dedupeConversationMessages(conversation.messages);
  const selectedAttachment =
    message.attachments.find((attachment) => attachment.id === selectedAttachmentId) ?? null;

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
            <button
              className="text-sm font-medium hover:text-primary hover:underline"
              onClick={(e) => onSearch?.(`from:${senderEmail || senderName}`, e.shiftKey)}
              title="Search emails from this sender"
            >
              {senderName}
            </button>
            {senderEmail && senderName !== senderEmail && (
              <button
                className="ml-1.5 text-xs text-muted-foreground hover:text-primary hover:underline"
                onClick={(e) => onSearch?.(`from:${senderEmail}`, e.shiftKey)}
                title="Search emails from this sender"
              >
                &lt;{senderEmail}&gt;
              </button>
            )}
          </div>
          <button
            className="shrink-0 font-mono text-xs tabular-nums text-muted-foreground hover:text-primary hover:underline"
            onClick={(e) => {
              const dateStr = new Date(message.receivedAt).toISOString().slice(0, 10);
              onSearch?.(`date:${dateStr}`, e.shiftKey);
            }}
            title="Search emails from this date"
          >
            {formatRelativeTime(message.receivedAt)}
          </button>
        </div>

        {/* Tags + attachment */}
        {(tags.length > 0 || message.hasAttachment) && (
          <div className="flex flex-wrap items-center gap-1.5">
            {tags.map((tag) => (
              <Badge
                variant="outline"
                className="cursor-pointer font-mono text-[10px] uppercase tracking-wider text-muted-foreground hover:border-primary hover:text-primary"
                key={tag}
                onClick={(e: React.MouseEvent) => onSearch?.(`tag:${tag}`, e.shiftKey)}
                title={`Search emails tagged "${tag}"`}
              >
                {tag}
              </Badge>
            ))}
            {message.hasAttachment && (
              <button
                className="inline-flex items-center gap-1 text-xs text-muted-foreground hover:text-primary hover:underline"
                onClick={(e) => onSearch?.("has:attachment", e.shiftKey)}
                title="Search emails with attachments"
              >
                <Paperclip size={12} />
                has attachment
              </button>
            )}
          </div>
        )}

        {message.attachments.length > 0 && (
          <div className="space-y-2">
            <Separator />
            <div className="flex flex-col gap-2">
              {message.attachments.map((attachment) => {
                const canPreview = canPreviewAttachment(attachment);
                const isSelected = attachment.id === selectedAttachmentId;
                const downloadUrl = buildMessageAttachmentUrl(
                  message.sourceId,
                  message.id,
                  attachment.id,
                  { download: true },
                );

                return (
                  <div
                    className="flex items-center justify-between gap-3 rounded-md border border-border bg-card/70 px-3 py-2"
                    key={attachment.id}
                  >
                    <div className="min-w-0">
                      <p className="truncate text-sm font-medium text-foreground">
                        {attachment.filename ?? "Unnamed attachment"}
                      </p>
                      <p className="font-mono text-[11px] text-muted-foreground">
                        {attachment.mimeType} · {formatAttachmentSize(attachment.size)}
                      </p>
                    </div>
                    <div className="flex shrink-0 items-center gap-2">
                      {canPreview && (
                        <Button
                          onClick={() =>
                            setAttachmentSelection({
                              attachmentId: isSelected ? null : attachment.id,
                              selectionKey,
                            })
                          }
                          size="sm"
                          type="button"
                          variant={isSelected ? "secondary" : "outline"}
                        >
                          {isSelected ? "Hide" : "View"}
                        </Button>
                      )}
                      <Button asChild size="sm" type="button" variant="outline">
                        <a download href={downloadUrl}>
                          Download
                        </a>
                      </Button>
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        )}

        {/* Thread switcher */}
        {threadMessages.length > 1 && (
          <div className="space-y-2">
            <Separator />
            <div className="flex flex-wrap gap-1.5">
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
          </div>
        )}
      </div>

      {/* Email body */}
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-hidden p-4">
        {selectedAttachment && (
          <div className="h-72 shrink-0 overflow-hidden rounded-md border border-border bg-card">
            <AttachmentPreview
              attachment={selectedAttachment}
              messageId={message.id}
              sourceId={message.sourceId}
            />
          </div>
        )}

        <div className="min-h-0 flex-1 overflow-hidden">
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
    </div>
  );
}
