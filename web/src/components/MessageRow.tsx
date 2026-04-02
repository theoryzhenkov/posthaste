import { Star } from "lucide-react";
import type { ConversationSummary } from "../api/types";
import { cn } from "../lib/utils";
import { formatRelativeTime } from "../utils/relativeTime";

interface MessageRowProps {
  message: ConversationSummary;
  isSelected: boolean;
  onSelect: () => void;
}

export function MessageRow({ message, isSelected, onSelect }: MessageRowProps) {
  const senderDisplay = message.fromName ?? message.fromEmail ?? "Unknown";
  const hasUnread = message.unreadCount > 0;
  const messageCountLabel =
    message.messageCount > 1 ? `${message.messageCount} in thread` : null;

  return (
    <button
      className={cn(
        "grid h-full w-full grid-cols-[minmax(140px,0.8fr)_minmax(0,2fr)_80px] items-center gap-3",
        "border-b border-border px-3 py-2 text-left text-sm transition-colors",
        "hover:bg-accent/50",
        isSelected && "border-l-2 border-l-primary bg-accent",
        !isSelected && "border-l-2 border-l-transparent",
      )}
      onClick={onSelect}
      type="button"
    >
      {/* From column */}
      <div className="flex min-w-0 items-center gap-1.5">
        {hasUnread && <span className="size-1.5 shrink-0 rounded-full bg-primary" />}
        {message.isFlagged && (
          <Star size={12} className="shrink-0 fill-amber-400 text-amber-400" />
        )}
        <span
          className={cn(
            "truncate",
            hasUnread ? "font-semibold text-foreground" : "text-muted-foreground",
          )}
        >
          {senderDisplay}
        </span>
      </div>

      {/* Subject + preview */}
      <div className="min-w-0 overflow-hidden">
        <div className="flex items-center gap-2">
          <span
            className={cn(
              "truncate shrink-0",
              hasUnread ? "font-semibold" : "text-muted-foreground",
            )}
          >
            {message.subject ?? "(no subject)"}
          </span>
          {message.preview && (
            <span className="truncate text-xs text-muted-foreground">
              {message.preview}
            </span>
          )}
        </div>
        {messageCountLabel && (
          <div className="mt-0.5 text-[10px] font-mono uppercase tracking-wider text-muted-foreground/70">
            {messageCountLabel}
          </div>
        )}
      </div>

      {/* Date + source tag */}
      <div className="flex flex-col items-end gap-0.5">
        <span className="whitespace-nowrap font-mono text-xs tabular-nums text-muted-foreground">
          {formatRelativeTime(message.latestReceivedAt)}
        </span>
        <span className="font-mono text-[9px] uppercase tracking-wider text-muted-foreground/60">
          {message.latestSourceName}
        </span>
      </div>
    </button>
  );
}
