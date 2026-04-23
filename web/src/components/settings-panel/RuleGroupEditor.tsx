/**
 * Recursive rule group editor for building smart mailbox filter trees.
 *
 * Groups support `all`/`any` operators, optional negation, and can
 * contain both condition nodes and nested groups.
 *
 * @spec docs/L1-search#smart-mailbox-data-model
 */
import type {
  SmartMailboxCondition,
  SmartMailboxGroup,
} from "../../api/types";
import { Button } from "../ui/button";
import { Checkbox } from "../ui/checkbox";
import { Input } from "../ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "../ui/select";
import {
  defaultCondition,
  defaultGroup,
  FIELD_OPTIONS,
  GROUP_OPERATOR_OPTIONS,
  operatorOptionsForField,
  parseField,
  parseGroupOperator,
  parseOperator,
} from "./helpers";

/**
 * Recursive editor for a `SmartMailboxGroup` node.
 * Renders its own conditions inline and delegates nested groups recursively.
 *
 * @spec docs/L1-search#smart-mailbox-data-model
 */
export function RuleGroupEditor({
  group,
  onChange,
}: {
  group: SmartMailboxGroup;
  onChange: (group: SmartMailboxGroup) => void;
}) {
  return (
    <div className="space-y-3 rounded-md border border-border-soft bg-bg-elev/55 p-3">
      <div className="flex flex-wrap items-end justify-between gap-2">
        <div className="grid gap-1.5 text-[13px]">
          <span className="text-[12px] font-medium text-muted-foreground">Match</span>
          <Select
            value={group.operator}
            onValueChange={(value) =>
              onChange({
                ...group,
                operator: parseGroupOperator(value, group.operator),
              })
            }
          >
            <SelectTrigger className="h-8 min-w-32 rounded-md border-border bg-background text-[13px] shadow-none">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {GROUP_OPERATOR_OPTIONS.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <label className="flex items-center gap-2 text-[13px] text-muted-foreground">
            <Checkbox
              checked={group.negated}
              onCheckedChange={(checked) =>
                onChange({ ...group, negated: checked === true })
              }
            />
            Negate group
          </label>

          <Button
            size="sm"
            variant="outline"
            type="button"
            onClick={() => onChange({ ...group, nodes: [...group.nodes, defaultCondition()] })}
          >
            Add condition
          </Button>
          <Button
            size="sm"
            variant="outline"
            type="button"
            onClick={() => onChange({ ...group, nodes: [...group.nodes, defaultGroup()] })}
          >
            Add group
          </Button>
        </div>
      </div>

      <div className="space-y-2">
        {group.nodes.length === 0 && (
          <p className="rounded-md border border-dashed border-border-soft bg-background/55 px-3 py-2.5 text-[12px] text-muted-foreground">
            No conditions yet. An empty group matches all messages.
          </p>
        )}
        {group.nodes.map((node, index) => (
          <div
            key={index}
            className="rounded-md border border-border-soft bg-background/80 p-3"
          >
            {node.type === "condition" ? (
              <ConditionEditor
                condition={node}
                onChange={(condition) =>
                  onChange({
                    ...group,
                    nodes: group.nodes.map((current, currentIndex) =>
                      currentIndex === index ? condition : current,
                    ),
                  })
                }
                onRemove={() =>
                  onChange({
                    ...group,
                    nodes: group.nodes.filter((_, currentIndex) => currentIndex !== index),
                  })
                }
              />
            ) : (
              <div className="space-y-3">
                <div className="flex justify-end">
                  <Button
                    size="sm"
                    variant="outline"
                    type="button"
                    onClick={() =>
                      onChange({
                        ...group,
                        nodes: group.nodes.filter((_, currentIndex) => currentIndex !== index),
                      })
                    }
                  >
                    Remove group
                  </Button>
                </div>
                <RuleGroupEditor
                  group={node}
                  onChange={(child) =>
                    onChange({
                      ...group,
                      nodes: group.nodes.map((current, currentIndex) =>
                        currentIndex === index ? { type: "group", ...child } : current,
                      ),
                    })
                  }
                />
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

/**
 * Single condition row editor: field, operator, value, and negate toggle.
 * @spec docs/L1-search#smart-mailbox-data-model
 */
function ConditionEditor({
  condition,
  onChange,
  onRemove,
}: {
  condition: SmartMailboxCondition;
  onChange: (condition: SmartMailboxCondition) => void;
  onRemove: () => void;
}) {
  const operators = operatorOptionsForField(condition.field);
  const usesList = condition.operator === "in";
  const isBooleanField =
    condition.field === "isRead" ||
    condition.field === "isFlagged" ||
    condition.field === "hasAttachment";

  return (
    <div className="space-y-3">
      <div className="grid gap-2 lg:grid-cols-[minmax(0,1.2fr)_minmax(0,0.9fr)_minmax(0,1.2fr)_auto]">
        <div className="grid gap-1 text-[13px]">
          <span className="text-[12px] font-medium text-muted-foreground">Field</span>
          <Select
            value={condition.field}
            onValueChange={(value) => {
              const field = parseField(value, condition.field);
              const nextOperator = operatorOptionsForField(field)[0];
              onChange({
                ...defaultCondition(field),
                operator: nextOperator,
              });
            }}
          >
            <SelectTrigger className="h-8 rounded-md border-border bg-background text-[13px] shadow-none">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {FIELD_OPTIONS.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <div className="grid gap-1 text-[13px]">
          <span className="text-[12px] font-medium text-muted-foreground">
            Operator
          </span>
          <Select
            value={condition.operator}
            onValueChange={(value) => {
              const operator = parseOperator(
                value,
                condition.field,
                condition.operator,
              );
              onChange({
                ...condition,
                operator,
                value: operator === "in" ? [] : isBooleanField ? false : "",
              });
            }}
          >
            <SelectTrigger className="h-8 rounded-md border-border bg-background text-[13px] shadow-none">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {operators.map((operator) => (
                <SelectItem key={operator} value={operator}>
                  {operator}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <div className="grid gap-1 text-[13px]">
          <span className="text-[12px] font-medium text-muted-foreground">Value</span>
          {isBooleanField ? (
            <Select
              value={String(Boolean(condition.value))}
              onValueChange={(value) =>
                onChange({
                  ...condition,
                  value: value === "true",
                })
              }
            >
              <SelectTrigger className="h-8 rounded-md border-border bg-background text-[13px] shadow-none">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="true">true</SelectItem>
                <SelectItem value="false">false</SelectItem>
              </SelectContent>
            </Select>
          ) : (
            <Input
              className="h-8 rounded-md border-border bg-background text-[13px] shadow-none"
              value={
                Array.isArray(condition.value)
                  ? condition.value.join(", ")
                  : String(condition.value)
              }
              placeholder={usesList ? "comma, separated, values" : "value"}
              onChange={(event) =>
                onChange({
                  ...condition,
                  value: usesList
                    ? event.target.value
                        .split(",")
                        .map((value) => value.trim())
                        .filter(Boolean)
                    : event.target.value,
                })
              }
            />
          )}
        </div>

        <div className="flex items-end">
          <Button size="sm" variant="outline" type="button" onClick={onRemove}>
            Remove
          </Button>
        </div>
      </div>

      <label className="flex items-center gap-2 text-[13px] text-muted-foreground">
        <Checkbox
          checked={condition.negated}
          onCheckedChange={(checked) =>
            onChange({ ...condition, negated: checked === true })
          }
        />
        Negate condition
      </label>
    </div>
  );
}
