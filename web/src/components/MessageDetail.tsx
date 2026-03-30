import { useQuery } from "@tanstack/react-query";
import { fetchEmail, fetchEmailBody } from "../api/client";
import { formatRelativeTime } from "../utils/relativeTime";
import { EmailFrame } from "./EmailFrame";

interface MessageDetailProps {
  emailId: string | null;
}

export function MessageDetail({ emailId }: MessageDetailProps) {
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
          {email.isFlagged && <span className="message-detail__tag">⭐ Flagged</span>}
          {email.hasAttachment && (
            <span className="message-detail__tag">📎 Attachment</span>
          )}
        </div>
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
