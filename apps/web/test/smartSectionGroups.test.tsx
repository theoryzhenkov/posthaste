import { afterEach, describe, expect, it } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render } from '@testing-library/react'
import type { ReactNode } from 'react'

import type { AppSettings, SmartMailboxSummary } from '../src/api/types'
import { SmartMailboxSection } from '../src/components/sidebar/SidebarContent'
import { queryKeys } from '../src/queryKeys'
import {
  resetRuntimeAdapterForTesting,
  setRuntimeAdapterForTesting,
} from '../src/runtime/adapter'
import { createFakeRuntimeAdapter } from '../src/runtime/fakeAdapter'

import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

afterEach(() => {
  resetRuntimeAdapterForTesting()
})

const mkSmart = (id: string): SmartMailboxSummary => ({
  id,
  name: id,
  kind: 'user',
  defaultKey: null,
  role: null,
  parentId: null,
  unreadMessages: 0,
  totalMessages: 0,
  createdAt: '',
  updatedAt: '',
})

const settings: AppSettings = {
  defaultAccountId: null,
  cachePolicy: 'auto' as never,
  automationRules: [],
  automationDrafts: [],
  mailboxColors: [],
  tags: [],
  smartMailboxOrder: [],
  accountOrder: [],
  mailboxGroups: [
    { id: 'g-triage', name: 'Triage', mailboxIds: ['flagged'], order: 0 },
  ],
}

const mailboxes = [mkSmart('all'), mkSmart('flagged')]

function renderSection(collapsedGroupIds: ReadonlySet<string>) {
  setRuntimeAdapterForTesting(createFakeRuntimeAdapter())
  const qc = new QueryClient()
  qc.setQueryData(queryKeys.settings, settings)
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={qc}>{children}</QueryClientProvider>
  )
  return render(
    <SmartMailboxSection
      collapsed={false}
      mailboxes={mailboxes}
      selectedView={null}
      isPaneActive={false}
      collapsedGroupIds={collapsedGroupIds}
      onOpenSmartMailboxSettings={() => {}}
      onSelectSmartMailbox={() => {}}
      onReorder={() => {}}
      onToggle={() => {}}
      onToggleGroupCollapsed={() => {}}
    />,
    { wrapper },
  )
}

describe('SmartMailboxSection groups', () => {
  it('renders the Group header, its nested smart member, and the ungrouped remainder', () => {
    const view = renderSection(new Set())
    expect(view.getByText('Triage')).toBeTruthy()
    // Ungrouped smart mailbox renders flat.
    expect(view.getByText('all')).toBeTruthy()
    // Grouped smart member renders nested (expanded group).
    expect(view.getByText('flagged')).toBeTruthy()
  })

  it('collapsing a smart Group hides its members but keeps the header + ungrouped rows', () => {
    const view = renderSection(new Set(['g-triage']))
    expect(view.getByText('Triage')).toBeTruthy()
    expect(view.getByText('all')).toBeTruthy()
    expect(view.queryByText('flagged')).toBeNull()
  })
})
