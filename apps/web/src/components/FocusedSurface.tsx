import { useEffect } from 'react'
import { useQuery } from '@tanstack/react-query'

import { fetchAccounts } from '@/api/client'
import type { MessageSummary } from '@/api/types'
import type { SurfaceDescriptor } from '@/surfaces'
import { queryKeys } from '@/queryKeys'
import { closeCurrentSurfaceWindow, isTauriRuntime } from '@/desktop'
import { MessageDetail } from './MessageDetail'
import { SettingsPanel } from './SettingsPanel'

interface FocusedSurfaceProps {
  surface: SurfaceDescriptor
  canClose?: boolean
  onClose?: () => void
  onSearch?: (query: string, append?: boolean) => void
  onSelectMessage?: (message: MessageSummary) => void
}

export function FocusedSurface({
  surface,
  canClose = true,
  onClose,
  onSearch,
  onSelectMessage,
}: FocusedSurfaceProps) {
  const accountsQuery = useQuery({
    queryKey: queryKeys.accounts,
    queryFn: fetchAccounts,
    enabled: surface.kind === 'settings',
  })

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key !== 'Escape' || !canClose) {
        return
      }
      event.preventDefault()
      if (onClose) {
        onClose()
      } else {
        void closeCurrentSurfaceWindow()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [canClose, onClose])

  if (surface.kind === 'settings') {
    return (
      <SettingsPanel
        accounts={accountsQuery.data ?? []}
        activeAccountId={null}
        initialAccountId={surface.params.accountId}
        initialCategory={surface.params.category}
        initialSmartMailboxId={surface.params.smartMailboxId}
        onActiveAccountChange={() => {}}
        onClose={
          canClose
            ? (onClose ?? (() => void closeCurrentSurfaceWindow()))
            : undefined
        }
        shell="overlay"
      />
    )
  }

  return (
    <MessageDetail
      selection={surface.params}
      onSearch={onSearch}
      onSelectMessage={onSelectMessage ?? (() => {})}
    />
  )
}

export function FocusedSurfaceDocument({
  surface,
}: {
  surface: SurfaceDescriptor
}) {
  useEffect(() => {
    if (!isTauriRuntime()) {
      return
    }

    function handleKeyDown(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'w') {
        event.preventDefault()
        void closeCurrentSurfaceWindow()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [])

  return (
    <main className="h-full min-h-0 bg-background text-foreground">
      <FocusedSurface surface={surface} />
    </main>
  )
}
