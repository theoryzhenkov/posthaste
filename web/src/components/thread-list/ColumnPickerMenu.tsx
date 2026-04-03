import {
  ContextMenu,
  ContextMenuCheckboxItem,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "../ui/context-menu";
import { ALL_COLUMNS, type ColumnId, getColumnDef } from "./columns";

interface ColumnPickerMenuProps {
  activeColumns: ColumnId[];
  onToggle: (columnId: ColumnId) => void;
  onReset: () => void;
  children: React.ReactNode;
}

export function ColumnPickerMenu({
  activeColumns,
  onToggle,
  onReset,
  children,
}: ColumnPickerMenuProps) {
  const activeSet = new Set(activeColumns);

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
      <ContextMenuContent>
        {ALL_COLUMNS.map((id) => {
          const def = getColumnDef(id);
          return (
            <ContextMenuCheckboxItem
              key={id}
              checked={activeSet.has(id)}
              onCheckedChange={() => onToggle(id)}
            >
              {def.label}
            </ContextMenuCheckboxItem>
          );
        })}
        <ContextMenuSeparator />
        <ContextMenuItem onSelect={onReset}>
          Revert to Default
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}
