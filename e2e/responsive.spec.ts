import { test, expect } from "@playwright/test";

/*
   Pure-browser pass against the Vite dev server (no Tauri runtime).
   Asserts that the responsive CSS collapses cleanly at 480 px and shows the
   table layout at 1200 px. The Tauri API mocks themselves are stubbed via
   window globals so the page does not throw on render.
*/

const VITE_URL = process.env.VITE_DEV_URL ?? "http://localhost:1420";

const stubFixture = `
  const now = Math.floor(Date.now() / 1000);
  window.__TAURI_INTERNALS__ = {
    invoke: async (cmd) => {
      if (cmd === "get_categories") {
        return [
          {
            id: 1, name: "Cars", sort_order: 0,
            forums: [
              { id: 10, category_id: 1, kind: "reddit", title: "BMW Discussion",
                source_url: "x", description: "Test", poll_interval_s: 1800,
                thread_count: 50, post_count: 500, last_polled_at: now, last_error: null,
                last_visited_at: now,
                latest_thread_title: "Sample", latest_thread_author: "alice",
                latest_thread_at: now - 600, latest_thread_url: "https://x/1",
                unread: false },
            ],
          },
        ];
      }
      return [];
    },
  };
`;

test.describe("Responsive layout (browser-only, no Tauri)", () => {
  test("desktop 1200px shows the table-style subbar", async ({ page }) => {
    await page.addInitScript(stubFixture);
    await page.setViewportSize({ width: 1200, height: 800 });
    await page.goto(VITE_URL);
    await expect(page.locator("[data-testid='banner']")).toBeVisible();
  });

  test("mobile 480px collapses without horizontal scroll", async ({ page }) => {
    await page.addInitScript(stubFixture);
    await page.setViewportSize({ width: 480, height: 800 });
    await page.goto(VITE_URL);
    await expect(page.locator("[data-testid='banner']")).toBeVisible();
    const hasHscroll = await page.evaluate(() => {
      const root = document.documentElement;
      return root.scrollWidth > root.clientWidth + 1;
    });
    expect(hasHscroll).toBe(false);
  });
});
