import { useQuery } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useRef } from "react";
import { fetchMessages } from "../api/client";
import type { MessageSummary } from "../api/types";
import type { EmailActions } from "../hooks/useEmailActions";
import { MessageRow } from "./MessageRow";

interface MessageListProps {
  accountId: string;
  mailboxId: string | null;
  selectedEmailId: string | null;
  onSelectEmail: (id: string) => void;
  actions: EmailActions;
}

export function MessageList({
  accountId,
  mailboxId,
  selectedEmailId,
  onSelectEmail,
  actions,
}: MessageListProps) {
  const containerRef = useRef<HTMLDivElement>(null);

  const {
    data: messages,
    isLoading,
    error,
  } = useQuery({
    queryKey: ["messages", accountId, mailboxId],
    queryFn: () => fetchMessages(mailboxId, accountId),
  });

  const selectedMessage = useMemo(
    () =>
      messages?.find((message: MessageSummary) => message.id === selectedEmailId) ??
      null,
    [messages, selectedEmailId],
  );
  const unreadCount = useMemo(
    () => messages?.filter((message: MessageSummary) => !message.isRead).length ?? 0,
    [messages],
  );
  const starredCount = useMemo(
    () =>
      messages?.filter((message: MessageSummary) => message.isFlagged).length ?? 0,
    [messages],
  );

  const navigateMessage = useCallback(
    (direction: 1 | -1) => {
      if (!messages || messages.length === 0) return;

      const currentIndex = messages.findIndex(
        (message: MessageSummary) => message.id === selectedEmailId,
      );
      const nextIndex =
        currentIndex === -1
          ? direction === 1
            ? 0
            : messages.length - 1
          : currentIndex + direction;

      if (nextIndex >= 0 && nextIndex < messages.length) {
        onSelectEmail(messages[nextIndex].id);
      }
    },
    [messages, onSelectEmail, selectedEmailId],
  );

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      const target = event.target as HTMLElement;
      if (target.tagName === "INPUT" || target.tagName === "TEXTAREA") return;

      switch (event.key) {
        case "j":
        case "ArrowDown":
          event.preventDefault();
          navigateMessage(1);
          break;
        case "k":
        case "ArrowUp":
          event.preventDefault();
          navigateMessage(-1);
          break;
        case "u":
          if (selectedMessage) actions.toggleRead(selectedMessage);
          break;
        case "s":
          if (selectedMessage) actions.toggleFlag(selectedMessage);
          break;
        case "e":
          if (selectedMessage) actions.archive(selectedMessage.id);
          break;
        case "#":
        case "Backspace":
          if (selectedMessage) actions.trash(selectedMessage.id);
          break;
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [actions, navigateMessage, selectedMessage]);

  if (!mailboxId) {
    return (
      <div className="message-list message-list--empty">
        <div className="message-list__empty-state">
          <p className="message-list__eyebrow">thread index</p>
          <p className="message-list__placeholder">
            Select a mailbox to open your message stream.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="message-list" ref={containerRef}>
      <div className="message-list__header">
        <div className="message-list__toolbar">
          <div>
            <p className="message-list__eyebrow">thread index</p>
            <h2 className="message-list__title">Mailbox overview</h2>
          </div>
        </div>
        <input
          type="text"
          className="message-list__search"
          placeholder="Filter mail (coming soon)"
          disabled
        />
        <div className="message-list__stats">
          <span>{messages?.length ?? 0} total</span>
          <span>{unreadCount} unread</span>
          <span>{starredCount} starred</span>
        </div>
        <div className="message-list__columns" aria-hidden="true">
          <span>sender</span>
          <span>subject</span>
          <span>state</span>
          <span>received</span>
        </div>
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
        {messages && messages.length === 0 && (
          <p className="message-list__placeholder">No emails in this mailbox</p>
        )}
        {messages?.map((message: MessageSummary) => (
          <MessageRow
            key={message.id}
            email={message}
            isSelected={message.id === selectedEmailId}
            onSelect={() => onSelectEmail(message.id)}
          />
        ))}
      </div>
    </div>
  );
}
