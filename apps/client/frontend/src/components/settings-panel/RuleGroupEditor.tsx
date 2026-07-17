/**
 * Recursive rule group editor for building smart mailbox filter trees.
 *
 * Groups support `all`/`any` operators, optional negation, and can
 * contain both condition nodes and nested groups.
 *
 */
import type { MailQueryGroup } from '../../api/types'
import { cn } from '../../lib/utils'
import { Button } from '../ui/button'
import { Checkbox } from '../ui/checkbox'
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
  GROUP_OPERATOR_OPTIONS,
  parseGroupOperator,
} from './helpers'
import { ConditionEditor } from './rule-group/ConditionEditor'

/**
 * Recursive editor for a `MailQueryGroup` node.
 * Renders its own conditions inline and delegates nested groups recursively.
 */
export function RuleGroupEditor({
  group,
  onChange,
  onRemove,
  depth = 0,
}: {
  group: MailQueryGroup
  onChange: (group: MailQueryGroup) => void
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
