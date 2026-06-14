import { jsonRequest, request } from './core'

import type {
  AppSettings,
  AutomationRulePreviewInput,
  AutomationRulePreviewResponse,
  ReadRequest,
  ReadResponse,
} from '../types'

export async function fetchSettings(): Promise<AppSettings> {
  return request<AppSettings>('/settings')
}

/** @spec docs/L1-api#endpoint-table */
export async function patchSettings(
  input: Partial<AppSettings>,
): Promise<AppSettings> {
  return jsonRequest<AppSettings>('/settings', 'PATCH', input)
}

/** @spec docs/L1-api#read-calls */
export async function read(request: ReadRequest): Promise<ReadResponse> {
  return jsonRequest<ReadResponse>('/read', 'POST', request)
}

/** @spec docs/L1-api#application-settings */
export async function previewAutomationRule(
  input: AutomationRulePreviewInput,
): Promise<AutomationRulePreviewResponse> {
  return jsonRequest<AutomationRulePreviewResponse>(
    '/automation-rules:preview',
    'POST',
    input,
  )
}
