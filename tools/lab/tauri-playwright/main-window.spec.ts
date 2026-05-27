import { expect, test } from "./fixtures";

const READY_MARKERS = [
  "state.app.ready.test",
  "state.settings.ready.test",
] as const;

test.describe("Linux Tauri main-window smoke", () => {
  test("waits for a Lab readiness marker", async ({ tauriPage }) => {
    const deadline = Date.now() + 60_000;
    let sawReadyMarker = false;

    while (Date.now() < deadline && !sawReadyMarker) {
      for (const marker of READY_MARKERS) {
        try {
          if (
            await tauriPage
              .locator(`[data-posthaste-state="${marker}"]`)
              .isVisible()
          ) {
            sawReadyMarker = true;
            break;
          }
        } catch {
          // The app-side bridge can start accepting socket traffic before the
          // first webview listener is registered. Retry until the readiness
          // deadline instead of failing on the first dropped command.
        }
      }
      if (!sawReadyMarker) {
        await new Promise((resolve) => setTimeout(resolve, 250));
      }
    }

    expect(sawReadyMarker).toBe(true);
  });
});
