import { afterAll, beforeAll } from 'bun:test'
import { GlobalRegistrator } from '@happy-dom/global-registrator'

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
 */
export function setupDomEnvironment(): void {
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
