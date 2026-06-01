import { afterAll, afterEach, beforeAll } from 'bun:test'
import { GlobalRegistrator } from '@happy-dom/global-registrator'
import { cleanup } from '@testing-library/react'

let activeDomSuites = 0
let registeredByTestHarness = false

/**
 * Register a happy-dom DOM (`window`/`document`/`HTMLElement`/…) for the calling
 * test file, and tear it down afterward. Call once at the top level of any test
 * file that renders React hooks/components (e.g. via `@testing-library/react`).
 *
 * Per-file (rather than a global `bunfig` preload) on purpose: bun does not
 * reliably keep a preloaded DOM registered across the whole suite, and scoping
 * the DOM to the files that need it keeps the ~150 pure-logic tests in a clean,
 * DOM-free global. `react-dom` only touches `window` at render time, so
 * registering in `beforeAll` (before any `renderHook`/`render`) is sufficient
 * even though the imports are hoisted above it.
 *
 * Also runs `@testing-library/react`'s `cleanup` after every test. RTL only
 * auto-registers that hook when `afterEach` is a global, which it is not under
 * `bun:test`, so without this the DOM/React teardown between tests is left to
 * runtime incidentals. Calling it explicitly unmounts rendered trees (running
 * effect cleanups, e.g. store unsubscribes) deterministically.
 */
export function setupDomEnvironment(): void {
  afterEach(() => {
    cleanup()
  })
  beforeAll(() => {
    activeDomSuites += 1
    if (typeof (globalThis as { window?: unknown }).window === 'undefined') {
      GlobalRegistrator.register()
      registeredByTestHarness = true
    }
  })
  afterAll(async () => {
    activeDomSuites -= 1
    if (activeDomSuites === 0 && registeredByTestHarness) {
      registeredByTestHarness = false
      await GlobalRegistrator.unregister()
    }
  })
}
