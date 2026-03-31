import type { MessageSummary } from "../api/types";
import { formatRelativeTime } from "../utils/relativeTime";

interface MessageRowProps {
  email: MessageSummary;
  isSelected: boolean;
  onSelect: () => void;
}

function senderInitials(senderDisplay: string): string {
  return senderDisplay
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase() ?? "")
    .join("");
}

export function MessageRow({ email, isSelected, onSelect }: MessageRowProps) {
  const senderDisplay = email.fromName ?? email.fromEmail ?? "Unknown";
  const initials = senderInitials(senderDisplay);

  return (
    <button
      className={`message-row ${isSelected ? "message-row--selected" : ""} ${!email.isRead ? "message-row--unread" : ""}`}
      onClick={onSelect}
      type="button"
    >
      <div className="message-row__sender-line">
        {!email.isRead && <span className="message-row__unread-dot" />}
        <span className="message-row__sender">{senderDisplay}</span>
        <span className="message-row__sender-id">{initials || "?"}</span>
      </div>
      <div className="message-row__summary">
        <span className="message-row__subject">
          {email.subject ?? "(no subject)"}
        </span>
        {email.preview && (
          <span className="message-row__preview">{email.preview}</span>
        )}
      </div>
      <div className="message-row__flags">
        {email.isFlagged && (
          <span className="message-row__flag" title="Flagged">
            starred
          </span>
        )}
        {email.hasAttachment && (
          <span className="message-row__flag" title="Has attachment">
            attachment
          </span>
        )}
      </div>
      <span className="message-row__date">
        {formatRelativeTime(email.receivedAt)}
      </span>
    </button>
  );
}
