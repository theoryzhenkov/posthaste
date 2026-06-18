import { expect, test } from "./fixtures";

const READY_MARKERS = [
  "state.app.ready.test",
  "state.settings.ready.test",
] as const;

interface LabTauriPage {
  evaluate: (script: string) => Promise<unknown>;
  locator: (selector: string) => {
    isVisible: () => Promise<boolean>;
  };
}

async function waitForMainWindowReadiness(
  tauriPage: LabTauriPage,
): Promise<void> {
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
}

test.describe("Linux Tauri main-window smoke", () => {
  test("waits for bundled runtime readiness and renders surface states", async ({
    tauriPage,
  }) => {
    await waitForMainWindowReadiness(tauriPage);

    await expect(
      tauriPage.evaluate(`
        ({
          mode: window.__POSTHASTE_RUNTIME_MODE__,
          hrefCarriesToken: window.location.href.includes("access_token="),
        })
      `),
    ).resolves.toEqual({ mode: "loopback", hrefCarriesToken: false });

    await tauriPage.evaluate(`
      window.history.pushState(
        null,
        "",
        "#/surface/compose?composeKind=new&sourceId=lab-smoke"
      );
      window.dispatchEvent(new HashChangeEvent("hashchange"));
    `);

    await expect(
      tauriPage.locator(
        '[data-posthaste-state="state.surface.compose.ready.test"]',
      ),
    ).toBeVisible({ timeout: 10_000 });

    await tauriPage.evaluate(`
      window.history.pushState(null, "", "#/surface/compose?composeKind=new");
      window.dispatchEvent(new HashChangeEvent("hashchange"));
    `);

    await expect(
      tauriPage.locator(
        '[data-posthaste-state="state.surface.invalid.ready.test"]',
      ),
    ).toBeVisible({ timeout: 10_000 });
  });
});
