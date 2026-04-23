import { X } from "lucide-react";
import { useEffect, useRef } from "react";
import type { AccountOverview } from "../api/types";
import { cn } from "../lib/utils";
import { SettingsPanel } from "./SettingsPanel";
import { Button } from "./ui/button";

interface SettingsOverlayProps {
  accounts: AccountOverview[];
  activeAccountId: string | null;
  initialCategory?: "general" | "accounts" | "mailboxes";
  onActiveAccountChange: (accountId: string | null) => void;
  onClose: () => void;
}

export function SettingsOverlay({
  accounts,
  activeAccountId,
  initialCategory,
  onActiveAccountChange,
  onClose,
}: SettingsOverlayProps) {
  const panelRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        onClose();
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  function handleBackdropClick(event: React.MouseEvent<HTMLDivElement>) {
    if (panelRef.current && !panelRef.current.contains(event.target as Node)) {
      onClose();
    }
  }

  return (
    <div
      className={cn(
        "fixed inset-0 z-50 flex items-center justify-center bg-[rgba(6,4,12,0.55)] p-4 backdrop-blur-[18px] backdrop-saturate-150 sm:p-8",
      )}
      onClick={handleBackdropClick}
    >
      <div
        ref={panelRef}
        className={cn(
          "relative flex h-full max-h-[min(47.5rem,92vh)] w-full max-w-[67.5rem] overflow-hidden rounded-[10px] border border-border bg-panel shadow-[0_32px_80px_rgba(0,0,0,0.55)]",
        )}
      >
        <Button
          aria-label="Close settings"
          className="absolute right-3 top-3 z-10"
          onClick={onClose}
          size="icon-sm"
          type="button"
          variant="ghost"
        >
          <X size={16} strokeWidth={1.5} />
        </Button>
        <SettingsPanel
          accounts={accounts}
          activeAccountId={activeAccountId}
          initialCategory={initialCategory}
          onActiveAccountChange={onActiveAccountChange}
          shell="overlay"
        />
      </div>
    </div>
  );
}
