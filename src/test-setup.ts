import "@testing-library/jest-dom/vitest";
import { vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => []),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(async () => {}),
}));

vi.mock("$app/navigation", () => ({
  goto: vi.fn(),
}));

vi.mock("$app/state", () => ({
  page: { params: { id: "1" }, url: new URL("http://localhost/"), route: { id: null } },
}));
