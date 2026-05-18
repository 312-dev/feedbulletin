import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  testIgnore: ["**/screenshot.spec.ts"],
  timeout: 30_000,
  fullyParallel: false,
  reporter: "list",
  use: {
    trace: "on-first-retry",
  },
});
