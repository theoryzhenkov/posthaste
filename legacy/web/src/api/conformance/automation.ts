import type {
  AutomationAction,
  AutomationRule,
  AutomationRulePreviewInput,
  AutomationRulePreviewResponse,
} from '../types'
import type { AssertTrue, Conforms, Wire } from './core'

export type _AutomationAction = AssertTrue<
  Conforms<AutomationAction, Wire['AutomationAction']>
>
export type _AutomationRule = AssertTrue<
  Conforms<AutomationRule, Wire['AutomationRule']>
>
export type _AutomationRulePreviewInput = AssertTrue<
  Conforms<AutomationRulePreviewInput, Wire['PreviewAutomationRuleRequest']>
>
export type _AutomationRulePreviewResponse = AssertTrue<
  Conforms<AutomationRulePreviewResponse, Wire['AutomationRulePreviewResponse']>
>
