import { useQuery } from "@tanstack/react-query";
import { fetchEmail, fetchEmailBody } from "../api/client";
import type { EmailActions } from "../hooks/useEmailActions";
import { formatRelativeTime } from "../utils/relativeTime";
import { EmailFrame } from "./EmailFrame";

interface MessageDetailProps {
  emailId: string | null;
  actions: EmailActions;
  onReply: (emailId: string) => void;
  onReplyAll: (emailId: string) => void;
  onForward: (emailId: string) => void;
}

export function MessageDetail({
  emailId,
  actions,
  onReply,
  onReplyAll,
  onForward,
}: MessageDetailProps) {
  const {
    data: email,
    isLoading,
    error,
  } = useQuery({
    queryKey: ["email", emailId],
    queryFn: () => fetchEmail(emailId!),
    enabled: emailId !== null,
  });

  const { data: body, isLoading: bodyLoading } = useQuery({
    queryKey: ["emailBody", emailId],
    queryFn: () => fetchEmailBody(emailId!),
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

  if (error || !email) {
    return (
      <div className="message-detail message-detail--empty">
        <p className="message-detail__placeholder message-detail__placeholder--error">
          Failed to load email
        </p>
      </div>
    );
  }

  const senderDisplay = email.fromName
    ? `${email.fromName} <${email.fromEmail ?? ""}>`
    : (email.fromEmail ?? "Unknown sender");

  return (
    <div className="message-detail">
      <div className="message-detail__header">
        <h2 className="message-detail__subject">
          {email.subject ?? "(no subject)"}
        </h2>
        <div className="message-detail__meta">
          <span className="message-detail__from">{senderDisplay}</span>
          <span className="message-detail__date">
            {formatRelativeTime(email.receivedAt)}
          </span>
        </div>
        <div className="message-detail__tags">
          {email.isFlagged && (
            <span className="message-detail__tag">⭐ Flagged</span>
          )}
          {email.hasAttachment && (
            <span className="message-detail__tag">📎 Attachment</span>
          )}
        </div>
      </div>
      <div className="message-detail__actions">
        <button onClick={() => onReply(email.id)} title="Reply">
          Reply
        </button>
        <button onClick={() => onReplyAll(email.id)} title="Reply All">
          Reply All
        </button>
        <button onClick={() => onForward(email.id)} title="Forward">
          Forward
        </button>
        <span className="message-detail__actions-separator" />
        <button
          onClick={() => actions.toggleRead(email)}
          title={email.isRead ? "Mark unread" : "Mark read"}
        >
          {email.isRead ? "Mark Unread" : "Mark Read"}
        </button>
        <button
          onClick={() => actions.toggleFlag(email)}
          title={email.isFlagged ? "Unflag" : "Flag"}
        >
          {email.isFlagged ? "Unflag" : "Flag"}
        </button>
        <button onClick={() => actions.archive(email.id)} title="Archive">
          Archive
        </button>
        <button onClick={() => actions.trash(email.id)} title="Move to Trash">
          Trash
        </button>
      </div>
      <div className="message-detail__body">
        {bodyLoading ? (
          <p className="message-detail__loading">Loading email body...</p>
        ) : body?.html ? (
          <EmailFrame html={body.html} />
        ) : body?.text ? (
          <pre className="message-detail__text">{body.text}</pre>
        ) : (
          <p>{email.preview ?? "No content available."}</p>
        )}
      </div>
    </div>
  );
}
