import { afterEach, describe, expect, it, spyOn } from 'bun:test'

import * as apiClient from '../src/api/client'
import type {
  AppSettings,
  AutomationRulePreviewResponse,
  Mailbox,
  SmartMailbox,
  MailQueryRule,
} from '../src/api/types'
import { resetRuntimeAdapterForTesting } from '../src/runtime/adapter'
import { runtimeMutations } from '../src/runtime/mutations'
import { runtimeViews } from '../src/runtime/views'

const emptyRule: MailQueryRule = {
  root: { operator: 'all', negated: false, nodes: [] },
}

const settings: AppSettings = {
  defaultAccountId: null,
  cachePolicy: {
    softCapBytes: 1,
    hardCapBytes: 2,
    cacheBodies: true,
    cacheRawMessages: false,
    cacheAttachments: true,
  },
  automationRules: [],
  automationDrafts: [],
}

const smartMailbox: SmartMailbox = {
  id: 'smart-1',
  name: 'Smart',
  position: 1,
  kind: 'user',
  defaultKey: null,
  parentId: null,
  rule: emptyRule,
  createdAt: '2026-04-28T12:00:00Z',
  updatedAt: '2026-04-28T12:00:00Z',
}

const mailbox: Mailbox = {
  id: 'inbox',
  name: 'Inbox',
  role: 'inbox',
  unreadEmails: 0,
  totalEmails: 0,
}

const preview: AutomationRulePreviewResponse = { total: 0, items: [] }

afterEach(() => {
  resetRuntimeAdapterForTesting()
})

describe('runtime settings and smart mailbox adapter', () => {
  it('wraps settings and automation HTTP behavior by default', async () => {
    const fetchSettingsSpy = spyOn(
      apiClient,
      'fetchSettings',
    ).mockResolvedValue(settings)
    const patchSettingsSpy = spyOn(
      apiClient,
      'patchSettings',
    ).mockResolvedValue(settings)
    const previewSpy = spyOn(
      apiClient,
      'previewAutomationRule',
    ).mockResolvedValue(preview)

    try {
      expect(await runtimeViews.settings.current()).toBe(settings)
      expect(
        await runtimeMutations.settings.patch({ defaultAccountId: 'primary' }),
      ).toBe(settings)
      expect(
        await runtimeMutations.settings.previewAutomationRule({
          condition: emptyRule,
        }),
      ).toBe(preview)
      expect(fetchSettingsSpy).toHaveBeenCalledWith()
      expect(patchSettingsSpy).toHaveBeenCalledWith({
        defaultAccountId: 'primary',
      })
      expect(previewSpy).toHaveBeenCalledWith({ condition: emptyRule })
    } finally {
      fetchSettingsSpy.mockRestore()
      patchSettingsSpy.mockRestore()
      previewSpy.mockRestore()
    }
  })

  it('wraps smart mailbox and source mailbox HTTP behavior by default', async () => {
    const createSpy = spyOn(apiClient, 'createSmartMailbox').mockResolvedValue(
      smartMailbox,
    )
    const fetchSpy = spyOn(apiClient, 'fetchSmartMailbox').mockResolvedValue(
      smartMailbox,
    )
    const updateSpy = spyOn(apiClient, 'updateSmartMailbox').mockResolvedValue(
      smartMailbox,
    )
    const deleteSpy = spyOn(apiClient, 'deleteSmartMailbox').mockResolvedValue({
      ok: true,
    })
    const resetSpy = spyOn(
      apiClient,
      'resetDefaultSmartMailboxes',
    ).mockResolvedValue([])
    const patchMailboxSpy = spyOn(apiClient, 'patchMailbox').mockResolvedValue([
      mailbox,
    ])

    try {
      expect(
        await runtimeMutations.smartMailboxes.create({
          name: 'Smart',
          rule: emptyRule,
        }),
      ).toBe(smartMailbox)
      expect(await runtimeViews.smartMailboxes.detail('smart-1')).toBe(
        smartMailbox,
      )
      expect(
        await runtimeMutations.smartMailboxes.update('smart-1', {
          position: 2,
        }),
      ).toBe(smartMailbox)
      expect(await runtimeMutations.smartMailboxes.delete('smart-1')).toEqual({
        ok: true,
      })
      expect(await runtimeMutations.smartMailboxes.resetDefaults()).toEqual([])
      expect(
        await runtimeMutations.mailboxes.patch('primary', 'inbox', {
          role: 'archive',
        }),
      ).toEqual([mailbox])
      expect(createSpy).toHaveBeenCalledWith({ name: 'Smart', rule: emptyRule })
      expect(fetchSpy).toHaveBeenCalledWith('smart-1')
      expect(updateSpy).toHaveBeenCalledWith('smart-1', { position: 2 })
      expect(deleteSpy).toHaveBeenCalledWith('smart-1')
      expect(resetSpy).toHaveBeenCalledWith()
      expect(patchMailboxSpy).toHaveBeenCalledWith('primary', 'inbox', {
        role: 'archive',
      })
    } finally {
      createSpy.mockRestore()
      fetchSpy.mockRestore()
      updateSpy.mockRestore()
      deleteSpy.mockRestore()
      resetSpy.mockRestore()
      patchMailboxSpy.mockRestore()
    }
  })
})
