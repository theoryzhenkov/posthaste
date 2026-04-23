import {
  Archive,
  Keyboard,
  Search,
  Settings,
  Star,
  Trash2,
  X,
} from "lucide-react";
import type { RefObject, ReactNode } from "react";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { cn } from "../lib/utils";

interface ActionBarProps {
  isFlagged: boolean;
  isMessageSelected: boolean;
  isSettingsOpen: boolean;
  searchInputRef: RefObject<HTMLInputElement | null>;
  searchQuery: string;
  onArchive: () => void;
  onClearSearch: () => void;
  onSearchQueryChange: (query: string) => void;
  onShowShortcuts: () => void;
  onToggleFlag: () => void;
  onToggleSettings: () => void;
  onTrash: () => void;
}

interface ActionButtonProps {
  children: ReactNode;
  disabled?: boolean;
  isActive?: boolean;
  label?: string;
  onClick: () => void;
  shortcut?: string;
  title: string;
}

function ActionButton({
  children,
  disabled,
  isActive,
  label,
  onClick,
  shortcut,
  title,
}: ActionButtonProps) {
  return (
    <Button
      aria-pressed={isActive}
      className={cn(
        "h-7 rounded-md px-2 text-muted-foreground hover:bg-panel-muted hover:text-foreground",
        !label && "w-7 px-0",
        isActive && "bg-brand-coral-soft text-brand-coral",
      )}
      disabled={disabled}
      onClick={onClick}
      size={label ? "sm" : "icon-sm"}
      title={title}
      type="button"
      variant="ghost"
    >
      {children}
      {label && <span className="text-xs">{label}</span>}
      {shortcut && label && (
        <kbd className="ml-1 rounded border border-border/70 bg-background/70 px-1 font-mono text-[10px] text-muted-foreground">
          {shortcut}
        </kbd>
      )}
    </Button>
  );
}

export function ActionBar({
  isFlagged,
  isMessageSelected,
  isSettingsOpen,
  searchInputRef,
  searchQuery,
  onArchive,
  onClearSearch,
  onSearchQueryChange,
  onShowShortcuts,
  onToggleFlag,
  onToggleSettings,
  onTrash,
}: ActionBarProps) {
  return (
    <header className="flex h-[var(--density-toolbar-height)] shrink-0 items-center gap-2 border-b border-border bg-chrome px-3 text-chrome-foreground shadow-[var(--shadow-pane)]">
      <div className="flex min-w-0 items-center gap-2">
        <div className="flex size-6 shrink-0 items-center justify-center rounded-md bg-brand-coral text-[11px] font-bold text-brand-coral-foreground">
          PH
        </div>
        <span className="mr-1 text-sm font-semibold select-none">PostHaste</span>
      </div>

      <div className="h-5 w-px bg-border" />

      <div className="flex items-center gap-0.5">
        <ActionButton
          disabled={!isMessageSelected}
          label="Archive"
          onClick={onArchive}
          shortcut="e"
          title="Archive selected conversation (e)"
        >
          <Archive size={16} strokeWidth={1.5} />
        </ActionButton>
        <ActionButton
          disabled={!isMessageSelected}
          label="Trash"
          onClick={onTrash}
          shortcut="#"
          title="Move selected conversation to trash (#)"
        >
          <Trash2 size={16} strokeWidth={1.5} />
        </ActionButton>
        <ActionButton
          disabled={!isMessageSelected}
          isActive={isFlagged}
          onClick={onToggleFlag}
          title="Toggle flag"
        >
          <Star
            size={16}
            strokeWidth={1.5}
            className={isFlagged ? "fill-current" : undefined}
          />
        </ActionButton>
      </div>

      <div className="flex-1" />

      <div className="relative flex w-[min(26rem,36vw)] min-w-48 items-center">
        <Search
          size={14}
          strokeWidth={1.5}
          className="pointer-events-none absolute left-2.5 text-muted-foreground"
        />
        <Input
          ref={searchInputRef}
          type="text"
          value={searchQuery}
          onChange={(event) => onSearchQueryChange(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              onClearSearch();
              searchInputRef.current?.blur();
            }
          }}
          placeholder="Search mail"
          className="h-7 rounded-md border-border bg-panel pl-7 pr-7 font-mono text-xs shadow-none placeholder:font-sans"
        />
        {searchQuery && (
          <button
            type="button"
            className="absolute right-2 text-muted-foreground transition-colors hover:text-foreground"
            onClick={onClearSearch}
            title="Clear search"
          >
            <X size={14} strokeWidth={1.5} />
          </button>
        )}
      </div>

      <div className="flex items-center gap-0.5">
        <ActionButton onClick={onShowShortcuts} title="Keyboard shortcuts (?)">
          <Keyboard size={16} strokeWidth={1.5} />
        </ActionButton>
        <ActionButton
          isActive={isSettingsOpen}
          onClick={onToggleSettings}
          title="Settings"
        >
          <Settings size={16} strokeWidth={1.5} />
        </ActionButton>
      </div>
    </header>
  );
}
