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
    <nav className="flex h-full min-h-0 flex-col border-r border-border bg-[var(--bg-elev)] px-2 py-4">
      <div className="flex items-center gap-2 px-3 pb-5">
        <span className="flex size-[22px] items-center justify-center rounded-[5px] bg-brand-coral font-mono text-[11px] font-bold text-brand-coral-foreground">
          P
        </span>
        <h2 className="text-[17px] font-semibold leading-none text-foreground">
          Settings
        </h2>
      </div>

      <div className="flex min-h-0 flex-1 flex-col gap-1">
        {items.map((item) => {
          const Icon = item.icon;
          const isActive = item.id === activeId;
          return (
            <button
              key={item.id}
              type="button"
              onClick={() => onSelect(item.id)}
              className={cn(
                "group flex h-[34px] items-center gap-2 rounded-[5px] px-2 text-left text-[13px] transition-colors",
                isActive
                  ? "bg-[var(--list-selection)] text-[var(--list-selection-foreground)]"
                  : "text-foreground/86 hover:bg-[var(--list-hover)] hover:text-foreground",
              )}
            >
              <span
                className={cn(
                  "flex size-5 shrink-0 items-center justify-center transition-colors",
                  isActive
                    ? "text-[var(--list-selection-foreground)]"
                    : "text-muted-foreground group-hover:text-foreground",
                )}
              >
                <Icon size={14} strokeWidth={1.6} />
              </span>
              <span className="min-w-0 flex-1">
                <span className="block truncate font-medium">{item.label}</span>
              </span>
            </button>
          );
        })}
      </div>

      <div className="px-3 pt-4 font-mono text-[10px] text-muted-foreground/65">
        v1.0.0 · JMAP 0.3
      </div>
    </nav>
  );
}
