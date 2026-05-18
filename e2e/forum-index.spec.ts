import { test, expect } from "@playwright/test";

/*
   Layer 2 E2E placeholder. Runs the built app via tauri-driver + WebDriver.
   tauri-driver is not yet installed; this spec is the scaffold that proves the
   infrastructure is wired so v1 specs can land cleanly.

   To enable locally:
     1. `cargo install tauri-driver --locked` (requires WebDriverIO server bits)
     2. `cargo tauri build` to produce the bundle
     3. Set TAURI_BIN to the binary path and uncomment the WebDriver setup below.

   Without the driver running, this test is skipped at runtime so the suite
   still reports green for unrelated frontend changes.
*/

test.describe("Forum Index (Layer 2 scaffold)", () => {
  test.skip(
    !process.env.TAURI_DRIVER_URL,
    "Set TAURI_DRIVER_URL=http://localhost:4444 to run against tauri-driver"
  );

  test("window opens and shows the Forum Reader title", async ({ page }) => {
    await page.goto(process.env.TAURI_FRONTEND_URL ?? "http://localhost:1420");
    await expect(page.locator("[data-testid='banner'] h1")).toHaveText("Forum Reader");
    await expect(page.locator("[data-testid='group']").first()).toBeVisible();
  });
});
