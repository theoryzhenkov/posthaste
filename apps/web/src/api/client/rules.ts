/**
 * Automation-rules REST client (RFC-L2-scripting ruling 23).
 *
 * @spec docs/eph/RFC-L2-scripting#7-rulings
 */
import { jsonRequest, request } from './core'

import type { Rule, RulesListResponse, WritableRuleInput } from '../types'

/** GET /v1/rules — the merged ruleset (rules.toml + rules.d). */
export async function fetchRules(): Promise<Rule[]> {
  const response = await request<RulesListResponse>('/rules')
  return response.rules
}

/** POST /v1/rules — create a GUI-managed rule (exec is unrepresentable). */
export async function createRule(input: WritableRuleInput): Promise<Rule> {
  return jsonRequest<Rule>('/rules', 'POST', input)
}

/** PUT /v1/rules/{id} — replace a GUI-managed rule. */
export async function updateRule(
  id: string,
  input: WritableRuleInput,
): Promise<Rule> {
  return jsonRequest<Rule>(`/rules/${id}`, 'PUT', input)
}

/** DELETE /v1/rules/{id} — delete a GUI-managed rule (204, no body). */
export async function deleteRule(id: string): Promise<void> {
  await request<unknown>(`/rules/${id}`, { method: 'DELETE' })
}
