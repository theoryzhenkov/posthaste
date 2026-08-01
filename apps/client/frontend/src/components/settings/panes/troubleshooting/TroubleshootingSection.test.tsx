/**
 * The one thing this pane can get wrong that types cannot catch: presenting a
 * cheap repair and a destructive one as if they were interchangeable. A user
 * whose older mail shows a blank Cc row is exactly the user who will click
 * whichever sounds most thorough, so the ordering and the wording are the
 * safety mechanism and are asserted here.
 */
import { describe, expect, test } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { renderToStaticMarkup } from 'react-dom/server'

import { MailClientProvider } from '@/data/context'
import { MailClient, type EventSourceLike } from '@/data/transport/client'
import { createStore } from '@/lib/store'
import {
  PlatformServicesProvider,
  type PlatformServices,
} from '@/lib/platform/services'

import { TroubleshootingSection } from './TroubleshootingSection'

const noop = async () => {}

function services(canFactoryReset: boolean): PlatformServices {
  return {
    openSurface: () => {},
    openSurfaceInSeparateWindow: noop,
    openExternalUrl: noop,
    updates: { check: async () => null },
    repair: {
      canFactoryReset: () => canFactoryReset,
      factoryResetAndRestart: noop,
      repairLocalDatabaseAndRestart: noop,
    },
    developerTools: createStore(false),
    diagnostics: { getInfo: async () => null, revealLogFolder: noop },
  }
}

function render(canFactoryReset = true) {
  const client = new MailClient({
    baseUrl: '',
    token: 'test',
    fetchImpl: async () => Response.json({ generation: 1 }),
    // `bun test` has no DOM, so the facade's live stream needs a stand-in;
    // this suite only renders, and never opens it.
    eventSourceFactory: () =>
      ({
        onopen: null,
        onmessage: null,
        onerror: null,
        close: () => {},
      }) as EventSourceLike,
  })
  return renderToStaticMarkup(
    <QueryClientProvider client={new QueryClient()}>
      <MailClientProvider client={client}>
        <PlatformServicesProvider value={services(canFactoryReset)}>
          <TroubleshootingSection />
        </PlatformServicesProvider>
      </MailClientProvider>
    </QueryClientProvider>,
  )
}

describe('troubleshooting actions', () => {
  test('offers the network-free detail rebuild', () => {
    const markup = render()
    expect(markup).toContain('Rebuild message details')
    expect(markup).toContain('Cc, Bcc, Reply-To')
  })

  test('lists the cheap repair before the destructive one', () => {
    const markup = render()
    expect(markup.indexOf('Rebuild message details')).toBeLessThan(
      markup.indexOf('Repair local database'),
    )
  })

  test('says the detail rebuild downloads and deletes nothing', () => {
    expect(render()).toContain('Nothing is downloaded, nothing is deleted')
  })

  test('says the database repair discards and re-downloads cached mail', () => {
    const markup = render()
    expect(markup).toContain('discarded and downloaded again')
    // And points at the cheaper action, so the blank-field case never lands
    // here by accident.
    expect(markup).toContain('which the rebuild above fixes')
  })

  test('hides the factory reset where the platform cannot do it', () => {
    expect(render(false)).not.toContain('Reset all local data')
  })
})
