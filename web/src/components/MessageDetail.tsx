import { useQuery } from "@tanstack/react-query";
import { fetchMessage } from "../api/client";
import type { EmailActions } from "../hooks/useEmailActions";
import { formatRelativeTime } from "../utils/relativeTime";
import { EmailFrame } from "./EmailFrame";

interface MessageDetailProps {
  accountId: string;
  emailId: string | null;
  actions: EmailActions;
}

function senderInitials(senderDisplay: string): string {
  return senderDisplay
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase() ?? "")
    .join("");
}

export function MessageDetail({
  accountId,
  emailId,
  actions,
}: MessageDetailProps) {
  const {
    data: message,
    isLoading,
    error,
  } = useQuery({
    queryKey: ["message", accountId, emailId],
    queryFn: () => fetchMessage(emailId!, accountId),
    enabled: emailId !== null,
  });

  if (!emailId) {
    return (
      <div className="message-detail message-detail--empty">
        <p className="message-detail__placeholder">
          Select an email to read it
        </p>
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="message-detail message-detail--empty">
        <p className="message-detail__placeholder">Loading...</p>
      </div>
    );
  }

  if (error || !message) {
    return (
      <div className="message-detail message-detail--empty">
        <p className="message-detail__placeholder message-detail__placeholder--error">
          Failed to load email
        </p>
      </div>
    );
  }

  const senderDisplay = message.fromName
    ? `${message.fromName} <${message.fromEmail ?? ""}>`
    : (message.fromEmail ?? "Unknown sender");
  const tags = [
    message.isFlagged ? "Starred" : null,
    message.hasAttachment ? "Attachment" : null,
    ...message.keywords,
  ].filter(Boolean) as string[];
  const senderShort = senderInitials(message.fromName ?? message.fromEmail ?? "U");

  return (
    <div className="message-detail">
      <div className="message-detail__header">
        <p className="message-detail__eyebrow">message</p>
        <div className="message-detail__headline">
          <h2 className="message-detail__subject">
            {message.subject ?? "(no subject)"}
          </h2>
          <span className="message-detail__sender-chip">{senderShort || "?"}</span>
        </div>
        <div className="message-detail__meta-grid">
          <div className="message-detail__meta-item">
            <span className="message-detail__meta-label">from</span>
            <span className="message-detail__from">{senderDisplay}</span>
          </div>
          <div className="message-detail__meta-item">
            <span className="message-detail__meta-label">received</span>
            <span className="message-detail__date">
              {formatRelativeTime(message.receivedAt)}
            </span>
          </div>
          {message.threadId && (
            <div className="message-detail__meta-item">
              <span className="message-detail__meta-label">thread</span>
              <span>{message.threadId}</span>
            </div>
          )}
          {message.mailboxIds.length > 0 && (
            <div className="message-detail__meta-item">
              <span className="message-detail__meta-label">mailboxes</span>
              <span>{message.mailboxIds.join(", ")}</span>
            </div>
          )}
          {message.rawMessage && (
            <div className="message-detail__meta-item">
              <span className="message-detail__meta-label">raw file</span>
              <span>{message.rawMessage.path}</span>
            </div>
          )}
        </div>
        <div className="message-detail__tags">
          {tags.map((tag) => (
            <span className="message-detail__tag" key={tag}>
              {tag}
            </span>
          ))}
        </div>
      </div>
      <div className="message-detail__actions">
        <div className="message-detail__actions-group">
          <button
            className="message-detail__action"
            onClick={() => actions.toggleRead(message)}
            title={message.isRead ? "Mark unread" : "Mark read"}
            type="button"
          >
            {message.isRead ? "mark unread" : "mark read"}
          </button>
          <button
            className="message-detail__action"
            onClick={() => actions.toggleFlag(message)}
            title={message.isFlagged ? "Unflag" : "Flag"}
            type="button"
          >
            {message.isFlagged ? "unstar" : "star"}
          </button>
          <button
            className="message-detail__action"
            onClick={() => actions.archive(message.id)}
            title="Archive"
            type="button"
          >
            archive
          </button>
          <button
            className="message-detail__action message-detail__action--danger"
            onClick={() => actions.trash(message.id)}
            title="Move to Trash"
            type="button"
          >
            trash
          </button>
        </div>
      </div>
      <div className="message-detail__body">
        {message.bodyHtml ? (
          <div className="message-detail__body-card">
            <EmailFrame html={message.bodyHtml} />
          </div>
        ) : message.bodyText ? (
          <pre className="message-detail__text">{message.bodyText}</pre>
        ) : (
          <p className="message-detail__fallback">
            {message.preview ?? "No content available."}
          </p>
        )}
      </div>
    </div>
  );
}
