import { useQuery } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useRef } from "react";
import { fetchEmails } from "../api/client";
import type { Email } from "../api/types";
import type { EmailActions } from "../hooks/useEmailActions";
import { MessageRow } from "./MessageRow";

interface MessageListProps {
  mailboxId: string | null;
  selectedEmailId: string | null;
  onSelectEmail: (id: string) => void;
  actions: EmailActions;
}

export function MessageList({
  mailboxId,
  selectedEmailId,
  onSelectEmail,
  actions,
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

  const selectedEmail = useMemo(
    () => emails?.find((e: Email) => e.id === selectedEmailId) ?? null,
    [emails, selectedEmailId],
  );

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
        case "u":
          if (selectedEmail) actions.toggleRead(selectedEmail);
          break;
        case "s":
          if (selectedEmail) actions.toggleFlag(selectedEmail);
          break;
        case "e":
          if (selectedEmail) actions.archive(selectedEmail.id);
          break;
        case "#":
        case "Backspace":
          if (selectedEmail) actions.trash(selectedEmail.id);
          break;
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [navigateEmail, selectedEmail, actions]);

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
