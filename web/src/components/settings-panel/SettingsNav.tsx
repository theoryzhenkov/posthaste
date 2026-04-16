/** Vertical nav rail for the settings panel. */
import type { LucideIcon } from "lucide-react";
import { cn } from "../../lib/utils";

export interface SettingsNavItem<T extends string> {
  id: T;
  label: string;
  icon: LucideIcon;
}

export function SettingsNav<T extends string>({
  items,
  activeId,
  onSelect,
}: {
  items: ReadonlyArray<SettingsNavItem<T>>;
  activeId: T;
  onSelect: (id: T) => void;
}) {
  return (
    <nav className="flex h-full min-h-0 flex-col gap-0.5 border-r border-border bg-background/40 p-2">
      {items.map((item) => {
        const Icon = item.icon;
        const isActive = item.id === activeId;
        return (
          <button
            key={item.id}
            type="button"
            onClick={() => onSelect(item.id)}
            className={cn(
              "flex items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left text-sm transition-colors",
              isActive
                ? "bg-accent text-accent-foreground"
                : "text-muted-foreground hover:bg-accent/50 hover:text-foreground",
            )}
          >
            <Icon size={15} strokeWidth={1.75} />
            <span className="truncate">{item.label}</span>
          </button>
        );
      })}
    </nav>
  );
}
