import { useEffect } from 'react'

import type { DomainEvent } from '@/api/types'
import { MAIL_DOMAIN_EVENT_NAME } from '@/hooks/useDaemonEvents'
import type { SidebarSelection } from '../Sidebar'
import { eventMayAffectView } from './model'

export function useDomainEventRefresh({
  enabled = true,
  isSearchBlocked,
  refetch,
  selectedView,
}: {
  enabled?: boolean
  isSearchBlocked: boolean
  refetch: () => void
  selectedView: SidebarSelection | null
}) {
  useEffect(() => {
    function handleDomainEvent(event: Event) {
      if (!enabled || isSearchBlocked) {
        return
      }
      const payload = (event as CustomEvent<DomainEvent>).detail
      if (!eventMayAffectView(payload, selectedView)) {
        return
      }
      refetch()
    }

    window.addEventListener(
      MAIL_DOMAIN_EVENT_NAME,
      handleDomainEvent as EventListener,
    )
    return () =>
      window.removeEventListener(
        MAIL_DOMAIN_EVENT_NAME,
        handleDomainEvent as EventListener,
      )
  }, [enabled, isSearchBlocked, refetch, selectedView])
}
