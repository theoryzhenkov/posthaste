import type {
  SmartMailboxCondition,
  SmartMailboxGroup,
} from "../../api/types";
import { Button } from "../ui/button";
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

export function RuleGroupEditor({
  group,
  onChange,
}: {
  group: SmartMailboxGroup;
  onChange: (group: SmartMailboxGroup) => void;
}) {
  return (
    <div className="space-y-3 rounded-lg border border-border/70 bg-background/60 p-3">
      <div className="flex flex-wrap items-center gap-3">
        <label className="grid gap-1 text-sm">
          <span className="text-muted-foreground">Match</span>
          <select
            className="h-8 rounded-md border border-border bg-background px-2 text-sm"
            value={group.operator}
            onChange={(event) =>
              onChange({
                ...group,
                operator: parseGroupOperator(event.target.value, group.operator),
              })
            }
          >
            {GROUP_OPERATOR_OPTIONS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>

        <label className="mt-5 flex items-center gap-2 text-sm text-muted-foreground">
          <input
            type="checkbox"
            checked={group.negated}
            onChange={(event) => onChange({ ...group, negated: event.target.checked })}
          />
          Negate group
        </label>

        <div className="mt-5 flex gap-2">
          <Button
            size="xs"
            variant="outline"
            type="button"
            onClick={() => onChange({ ...group, nodes: [...group.nodes, defaultCondition()] })}
          >
            Add condition
          </Button>
          <Button
            size="xs"
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
          <p className="text-xs text-muted-foreground">
            No conditions yet. An empty group matches all messages.
          </p>
        )}
        {group.nodes.map((node, index) => (
          <div key={index} className="rounded border border-border bg-card/70 p-3">
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
                    size="xs"
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
      <div className="grid grid-cols-[minmax(0,1.2fr)_minmax(0,0.9fr)_minmax(0,1.2fr)_auto] gap-3">
        <label className="grid gap-1 text-sm">
          <span className="text-muted-foreground">Field</span>
          <select
            className="h-8 rounded-md border border-border bg-background px-2 text-sm"
            value={condition.field}
            onChange={(event) => {
              const field = parseField(event.target.value, condition.field);
              const nextOperator = operatorOptionsForField(field)[0];
              onChange({
                ...defaultCondition(field),
                operator: nextOperator,
              });
            }}
          >
            {FIELD_OPTIONS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>

        <label className="grid gap-1 text-sm">
          <span className="text-muted-foreground">Operator</span>
          <select
            className="h-8 rounded-md border border-border bg-background px-2 text-sm"
            value={condition.operator}
            onChange={(event) => {
              const operator = parseOperator(
                event.target.value,
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
            {operators.map((operator) => (
              <option key={operator} value={operator}>
                {operator}
              </option>
            ))}
          </select>
        </label>

        <label className="grid gap-1 text-sm">
          <span className="text-muted-foreground">Value</span>
          {isBooleanField ? (
            <select
              className="h-8 rounded-md border border-border bg-background px-2 text-sm"
              value={String(Boolean(condition.value))}
              onChange={(event) =>
                onChange({
                  ...condition,
                  value: event.target.value === "true",
                })
              }
            >
              <option value="true">true</option>
              <option value="false">false</option>
            </select>
          ) : (
            <input
              className="h-8 rounded-md border border-border bg-background px-2 text-sm"
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
        </label>

        <div className="flex items-end">
          <Button size="xs" variant="outline" type="button" onClick={onRemove}>
            Remove
          </Button>
        </div>
      </div>

      <label className="flex items-center gap-2 text-sm text-muted-foreground">
        <input
          type="checkbox"
          checked={condition.negated}
          onChange={(event) => onChange({ ...condition, negated: event.target.checked })}
        />
        Negate condition
      </label>
    </div>
  );
}
