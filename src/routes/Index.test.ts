import { render, waitFor } from "@testing-library/svelte";
import { describe, expect, it, vi, beforeEach } from "vitest";
import type { CategoryWithForums, Forum } from "$lib/types";

vi.mock("$lib/api", () => ({
  api: {
    getCategories: vi.fn(),
    getForum: vi.fn(),
    getForumThreads: vi.fn(),
    markForumVisited: vi.fn(async () => {}),
    markAllForumsVisited: vi.fn(async () => {}),
    refreshAll: vi.fn(async () => {}),
    openExternal: vi.fn(async () => {}),
    openThread: vi.fn(async () => {}),
    openThreadByUrl: vi.fn(async () => {}),
  },
}));

import Page from "./+page.svelte";
import { api } from "$lib/api";

function mkForum(overrides: Partial<Forum>): Forum {
  return {
    id: 0,
    category_id: 0,
    kind: "reddit",
    title: "",
    source_url: "x",
    description: null,
    poll_interval_s: 1800,
    thread_count: 0,
    post_count: 0,
    last_polled_at: Math.floor(Date.now() / 1000),
    last_error: null,
    last_visited_at: null,
    latest_thread_title: null,
    latest_thread_author: null,
    latest_thread_at: null,
    latest_thread_url: null,
    theme: null,
    unread: false,
    ...overrides,
  };
}

const fixture: CategoryWithForums[] = [
  {
    id: 1,
    name: "Cars",
    sort_order: 0,
    forums: [
      mkForum({
        id: 10,
        category_id: 1,
        kind: "reddit",
        title: "Old House Restoration",
        description: "Old houses",
        thread_count: 5,
        post_count: 50,
      }),
      mkForum({
        id: 11,
        category_id: 1,
        kind: "xenforo",
        title: "Style Forum",
        description: "XenForo",
        thread_count: 3,
        post_count: 30,
      }),
    ],
  },
  {
    id: 2,
    name: "Chicago",
    sort_order: 1,
    forums: [
      mkForum({
        id: 20,
        category_id: 2,
        kind: "reddit",
        title: "The Chicago Lounge",
        thread_count: 7,
        post_count: 70,
        unread: true,
        latest_thread_title: "Best Italian beef sandwich 2026",
        latest_thread_author: "deepdishfan",
        latest_thread_at: Math.floor(Date.now() / 1000) - 1800,
        latest_thread_url: "https://www.reddit.com/r/chicago/abc/",
      }),
    ],
  },
  {
    id: 3,
    name: "Smart Home",
    sort_order: 2,
    forums: [],
  },
];

describe("Forum Index page", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    (api.getCategories as ReturnType<typeof vi.fn>).mockResolvedValue(fixture);
  });

  it("renders banner and categories from the api", async () => {
    const { getByTestId, getAllByTestId, getByText } = render(Page);
    expect(getByTestId("banner")).toBeInTheDocument();
    await waitFor(() => {
      expect(getAllByTestId("category")).toHaveLength(3);
    });
    expect(getByText("Cars")).toBeInTheDocument();
    expect(getByText("Chicago")).toBeInTheDocument();
    expect(getByText("Smart Home")).toBeInTheDocument();
  });

  it("renders forum rows under each category", async () => {
    const { getAllByTestId } = render(Page);
    await waitFor(() => {
      const rows = getAllByTestId("forum-row");
      expect(rows.length).toBeGreaterThanOrEqual(3);
    });
  });

  it("never shows r/* style names in visible text", async () => {
    const { container } = render(Page);
    await waitFor(() => {
      expect(container.querySelectorAll("[data-testid='forum-row']").length).toBeGreaterThan(0);
    });
    expect(container.textContent ?? "").not.toMatch(/r\/\w+/);
  });
});
