import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  testMatch: ["**/screenshot.spec.ts"],
  timeout: 30_000,
  fullyParallel: false,
  reporter: "list",
});
