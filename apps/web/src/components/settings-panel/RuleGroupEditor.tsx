/**
 * Recursive rule group editor for building smart mailbox filter trees.
 *
 * Groups support `all`/`any` operators, optional negation, and can
 * contain both condition nodes and nested groups.
 *
 * @spec docs/L1-search#smart-mailbox-data-model
 */
import type { SmartMailboxCondition, SmartMailboxGroup } from '../../api/types'
import { cn } from '../../lib/utils'
import { Button } from '../ui/button'
import { Checkbox } from '../ui/checkbox'
import { Input } from '../ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../ui/select'
import {
  defaultCondition,
  defaultGroup,
  FIELD_OPTIONS,
  GROUP_OPERATOR_OPTIONS,
  operatorOptionsForField,
  parseField,
  parseGroupOperator,
  parseOperator,
} from './helpers'

/**
 * Recursive editor for a `SmartMailboxGroup` node.
 * Renders its own conditions inline and delegates nested groups recursively.
 *
 * @spec docs/L1-search#smart-mailbox-data-model
 */
export function RuleGroupEditor({
  group,
  onChange,
  onRemove,
  depth = 0,
}: {
  group: SmartMailboxGroup
  onChange: (group: SmartMailboxGroup) => void
  onRemove?: () => void
  depth?: number
}) {
  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2">
        <div className="flex flex-wrap items-center gap-2 text-[13px] leading-none">
          <span className="text-[12px] font-medium text-muted-foreground">
            Match
          </span>
          <label className="flex h-8 items-center justify-center gap-1.5 px-1 text-[12px] text-muted-foreground">
            <Checkbox
              checked={group.negated}
              onCheckedChange={(checked) =>
                onChange({ ...group, negated: checked === true })
              }
            />
            not
          </label>
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

        <div className="flex flex-wrap items-center gap-1.5">
          <Button
            size="sm"
            variant="outline"
            type="button"
            className="h-8 rounded-md border-border bg-background px-2 font-mono text-[12px]"
            aria-label="Add expression"
            onClick={() =>
              onChange({
                ...group,
                nodes: [...group.nodes, defaultCondition()],
              })
            }
          >
            +e
          </Button>
          <Button
            size="sm"
            variant="outline"
            type="button"
            className="h-8 rounded-md border-border bg-background px-2 font-mono text-[12px]"
            aria-label="Add group"
            onClick={() =>
              onChange({ ...group, nodes: [...group.nodes, defaultGroup()] })
            }
          >
            +g
          </Button>
          {onRemove && (
            <Button
              size="sm"
              variant="outline"
              type="button"
              className="h-8 rounded-md border-border bg-background px-2 font-mono text-[12px] text-muted-foreground hover:text-destructive"
              aria-label="Remove group"
              onClick={onRemove}
            >
              -
            </Button>
          )}
        </div>
      </div>

      <div className="space-y-3">
        {group.nodes.length === 0 && (
          <p className="rounded-md border border-dashed border-border-soft px-3 py-3 text-[12px] text-muted-foreground">
            No expressions yet. An empty group matches all messages.
          </p>
        )}
        {group.nodes.map((node, index) => (
          <div
            key={index}
            className={cn(
              'pt-3 first:pt-0',
              node.type === 'group' &&
                'border-l border-border-soft pl-4 first:pt-0',
            )}
          >
            {node.type === 'condition' ? (
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
                    nodes: group.nodes.filter(
                      (_, currentIndex) => currentIndex !== index,
                    ),
                  })
                }
              />
            ) : (
              <RuleGroupEditor
                group={node}
                depth={depth + 1}
                onRemove={() =>
                  onChange({
                    ...group,
                    nodes: group.nodes.filter(
                      (_, currentIndex) => currentIndex !== index,
                    ),
                  })
                }
                onChange={(child) =>
                  onChange({
                    ...group,
                    nodes: group.nodes.map((current, currentIndex) =>
                      currentIndex === index
                        ? { type: 'group', ...child }
                        : current,
                    ),
                  })
                }
              />
            )}
          </div>
        ))}
      </div>
    </div>
  )
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
  condition: SmartMailboxCondition
  onChange: (condition: SmartMailboxCondition) => void
  onRemove: () => void
}) {
  const operators = operatorOptionsForField(condition.field)
  const usesList = condition.operator === 'in'
  const isBooleanField =
    condition.field === 'isRead' ||
    condition.field === 'isFlagged' ||
    condition.field === 'hasAttachment'

  return (
    <div className="grid gap-2 sm:grid-cols-[72px_minmax(0,1fr)_auto] sm:items-center">
      <span className="text-[12px] font-medium text-muted-foreground">
        Where
      </span>

      <div className="grid gap-2 lg:grid-cols-[minmax(0,1.05fr)_auto_minmax(0,0.85fr)_minmax(0,1.1fr)] lg:items-center">
        <div className="grid gap-1 text-[13px]">
          <Select
            value={condition.field}
            onValueChange={(value) => {
              const field = parseField(value, condition.field)
              const nextOperator = operatorOptionsForField(field)[0]
              onChange({
                ...defaultCondition(field),
                operator: nextOperator,
              })
            }}
          >
            <SelectTrigger
              aria-label="Field"
              className="h-8 rounded-md border-border bg-background text-[13px] shadow-none"
            >
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

        <label className="flex h-8 items-center justify-center gap-1.5 px-1 text-[12px] text-muted-foreground">
          <Checkbox
            checked={condition.negated}
            onCheckedChange={(checked) =>
              onChange({ ...condition, negated: checked === true })
            }
          />
          not
        </label>

        <div className="grid gap-1 text-[13px]">
          <Select
            value={condition.operator}
            onValueChange={(value) => {
              const operator = parseOperator(
                value,
                condition.field,
                condition.operator,
              )
              onChange({
                ...condition,
                operator,
                value: operator === 'in' ? [] : isBooleanField ? false : '',
              })
            }}
          >
            <SelectTrigger
              aria-label="Operator"
              className="h-8 rounded-md border-border bg-background text-[13px] shadow-none"
            >
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
          {isBooleanField ? (
            <Select
              value={String(Boolean(condition.value))}
              onValueChange={(value) =>
                onChange({
                  ...condition,
                  value: value === 'true',
                })
              }
            >
              <SelectTrigger
                aria-label="Value"
                className="h-8 rounded-md border-border bg-background text-[13px] shadow-none"
              >
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
                  ? condition.value.join(', ')
                  : String(condition.value)
              }
              placeholder={usesList ? 'comma, separated, values' : 'value'}
              onChange={(event) =>
                onChange({
                  ...condition,
                  value: usesList
                    ? event.target.value
                        .split(',')
                        .map((value) => value.trim())
                        .filter(Boolean)
                    : event.target.value,
                })
              }
            />
          )}
        </div>
      </div>

      <div className="flex items-center justify-end">
        <Button
          size="sm"
          variant="outline"
          type="button"
          className="h-8 rounded-md border-border bg-background px-2 font-mono text-[12px] text-muted-foreground hover:text-destructive"
          aria-label="Remove expression"
          onClick={onRemove}
        >
          -
        </Button>
      </div>
    </div>
  )
}
