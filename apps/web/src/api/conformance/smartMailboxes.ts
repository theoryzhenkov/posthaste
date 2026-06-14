import type {
  CreateSmartMailboxInput,
  SmartMailbox,
  SmartMailboxCondition,
  SmartMailboxGroup,
  SmartMailboxRule,
  SmartMailboxRuleGroup,
  SmartMailboxRuleNode,
  SmartMailboxSummary,
  UpdateSmartMailboxInput,
} from '../types'
import type { AssertTrue, Conforms, Wire } from './core'

// The wire emits the rule-tree discriminant (`type`) only on the union
// `SmartMailboxRuleNode` (= group-member | condition-member), whereas the
// curated layer folds the discriminant into each node variant.
export type _SmartMailboxGroup = AssertTrue<
  Conforms<SmartMailboxGroup, Wire['SmartMailboxGroup']>
>
export type _SmartMailboxCondition = AssertTrue<
  Conforms<
    SmartMailboxCondition,
    Extract<Wire['SmartMailboxRuleNode'], { type: 'condition' }>
  >
>
export type _SmartMailboxRuleGroup = AssertTrue<
  Conforms<
    SmartMailboxRuleGroup,
    Extract<Wire['SmartMailboxRuleNode'], { type: 'group' }>
  >
>
export type _SmartMailboxRuleNode = AssertTrue<
  Conforms<SmartMailboxRuleNode, Wire['SmartMailboxRuleNode']>
>
export type _SmartMailboxRule = AssertTrue<
  Conforms<SmartMailboxRule, Wire['SmartMailboxRule']>
>
export type _SmartMailbox = AssertTrue<
  Conforms<SmartMailbox, Wire['SmartMailbox']>
>
export type _SmartMailboxSummary = AssertTrue<
  Conforms<SmartMailboxSummary, Wire['SmartMailboxSummary']>
>
export type _CreateSmartMailboxInput = AssertTrue<
  Conforms<CreateSmartMailboxInput, Wire['CreateSmartMailboxRequest']>
>
export type _UpdateSmartMailboxInput = AssertTrue<
  Conforms<UpdateSmartMailboxInput, Wire['PatchSmartMailboxRequest']>
>
