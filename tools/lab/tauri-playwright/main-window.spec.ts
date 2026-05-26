import { expect, test } from "./fixtures";

test.describe("Linux Tauri main-window smoke", () => {
  test("waits for the app lab readiness marker", async ({ tauriPage }) => {
    await expect(
      tauriPage.locator('[data-posthaste-state="state.app.ready.test"]'),
    ).toBeVisible({ timeout: 60_000 });
  });
});
