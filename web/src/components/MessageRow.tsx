import type { Email } from "../api/types";
import { formatRelativeTime } from "../utils/relativeTime";

interface MessageRowProps {
  email: Email;
  isSelected: boolean;
  onSelect: () => void;
}

export function MessageRow({ email, isSelected, onSelect }: MessageRowProps) {
  const senderDisplay = email.fromName ?? email.fromEmail ?? "Unknown";

  return (
    <button
      className={`message-row ${isSelected ? "message-row--selected" : ""} ${!email.isRead ? "message-row--unread" : ""}`}
      onClick={onSelect}
      type="button"
    >
      <div className="message-row__header">
        <div className="message-row__sender-line">
          {!email.isRead && <span className="message-row__unread-dot" />}
          <span className="message-row__sender">{senderDisplay}</span>
          <div className="message-row__icons">
            {email.hasAttachment && (
              <span className="message-row__icon" title="Has attachment">
                📎
              </span>
            )}
            {email.isFlagged && (
              <span className="message-row__icon" title="Flagged">
                ⭐
              </span>
            )}
          </div>
        </div>
        <span className="message-row__date">
          {formatRelativeTime(email.receivedAt)}
        </span>
      </div>
      <div className="message-row__subject">
        {email.subject ?? "(no subject)"}
      </div>
      {email.preview && (
        <div className="message-row__preview">{email.preview}</div>
      )}
    </button>
  );
}
