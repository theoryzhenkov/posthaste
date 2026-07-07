import type {
  CreateSmartMailboxInput,
  SmartMailbox,
  MailQueryCondition,
  MailQueryGroup,
  MailQueryRule,
  MailQueryRuleGroup,
  MailQueryRuleNode,
  SmartMailboxSummary,
  UpdateSmartMailboxInput,
} from '../types'
import type { AssertTrue, Conforms, Wire } from './core'

// The wire emits the rule-tree discriminant (`type`) only on the union
// `MailQueryRuleNode` (= group-member | condition-member), whereas the
// curated layer folds the discriminant into each node variant.
export type _SmartMailboxGroup = AssertTrue<
  Conforms<MailQueryGroup, Wire['MailQueryGroup']>
>
export type _SmartMailboxCondition = AssertTrue<
  Conforms<
    MailQueryCondition,
    Extract<Wire['MailQueryRuleNode'], { type: 'condition' }>
  >
>
export type _SmartMailboxRuleGroup = AssertTrue<
  Conforms<
    MailQueryRuleGroup,
    Extract<Wire['MailQueryRuleNode'], { type: 'group' }>
  >
>
export type _SmartMailboxRuleNode = AssertTrue<
  Conforms<MailQueryRuleNode, Wire['MailQueryRuleNode']>
>
export type _SmartMailboxRule = AssertTrue<
  Conforms<MailQueryRule, Wire['MailQueryRule']>
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
