import { useQuery } from "@tanstack/react-query";
import { useCallback, useEffect, useRef } from "react";
import { fetchEmails } from "../api/client";
import type { Email } from "../api/types";
import { MessageRow } from "./MessageRow";

interface MessageListProps {
  mailboxId: string | null;
  selectedEmailId: string | null;
  onSelectEmail: (id: string) => void;
}

export function MessageList({
  mailboxId,
  selectedEmailId,
  onSelectEmail,
}: MessageListProps) {
  const containerRef = useRef<HTMLDivElement>(null);

  const {
    data: emails,
    isLoading,
    error,
  } = useQuery({
    queryKey: ["emails", mailboxId],
    queryFn: () => fetchEmails(mailboxId!),
    enabled: mailboxId !== null,
  });

  const navigateEmail = useCallback(
    (direction: 1 | -1) => {
      if (!emails || emails.length === 0) return;

      const currentIndex = emails.findIndex(
        (e: Email) => e.id === selectedEmailId,
      );
      let nextIndex: number;

      if (currentIndex === -1) {
        nextIndex = direction === 1 ? 0 : emails.length - 1;
      } else {
        nextIndex = currentIndex + direction;
      }

      if (nextIndex >= 0 && nextIndex < emails.length) {
        onSelectEmail(emails[nextIndex].id);
      }
    },
    [emails, selectedEmailId, onSelectEmail],
  );

  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      const target = e.target as HTMLElement;
      // Don't capture if typing in an input
      if (target.tagName === "INPUT" || target.tagName === "TEXTAREA") return;

      switch (e.key) {
        case "j":
        case "ArrowDown":
          e.preventDefault();
          navigateEmail(1);
          break;
        case "k":
        case "ArrowUp":
          e.preventDefault();
          navigateEmail(-1);
          break;
        case "Enter":
          // Select already handled by navigateEmail setting the ID
          break;
        case "/":
          // Placeholder: focus search bar when implemented
          e.preventDefault();
          break;
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [navigateEmail]);

  if (!mailboxId) {
    return (
      <div className="message-list message-list--empty">
        <p className="message-list__placeholder">Select a mailbox</p>
      </div>
    );
  }

  return (
    <div className="message-list" ref={containerRef}>
      <div className="message-list__header">
        <input
          type="text"
          className="message-list__search"
          placeholder="Search emails..."
          disabled
        />
      </div>
      <div className="message-list__items">
        {isLoading && (
          <p className="message-list__placeholder">Loading emails...</p>
        )}
        {error && (
          <p className="message-list__placeholder message-list__placeholder--error">
            Failed to load emails
          </p>
        )}
        {emails && emails.length === 0 && (
          <p className="message-list__placeholder">No emails in this mailbox</p>
        )}
        {emails?.map((email: Email) => (
          <MessageRow
            key={email.id}
            email={email}
            isSelected={email.id === selectedEmailId}
            onSelect={() => onSelectEmail(email.id)}
          />
        ))}
      </div>
    </div>
  );
}
