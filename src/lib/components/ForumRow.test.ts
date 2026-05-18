import { render } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import ForumRow from "./ForumRow.svelte";
import type { Forum } from "$lib/types";

function forum(overrides: Partial<Forum> = {}): Forum {
  return {
    id: 1,
    category_id: 1,
    kind: "reddit",
    title: "Old House Restoration",
    source_url: "https://old.reddit.com/r/centuryhomes.json",
    description: "Active old-house community",
    poll_interval_s: 1800,
    thread_count: 27,
    post_count: 312,
    last_polled_at: Math.floor(Date.now() / 1000) - 60,
    last_error: null,
    last_visited_at: null,
    latest_thread_title: "1923 Foursquare full restoration log",
    latest_thread_author: "oldhouselove",
    latest_thread_at: Math.floor(Date.now() / 1000) - 3600,
    latest_thread_url: "https://www.reddit.com/r/centuryhomes/comments/abc/",
    theme: null,
    unread: true,
    ...overrides,
  };
}

describe("ForumRow", () => {
  it("renders forum title, description, and counts", () => {
    const { getByTestId, getByText } = render(ForumRow, { props: { forum: forum() } });
    const row = getByTestId("forum-row");
    expect(row).toBeInTheDocument();
    expect(row.dataset.forumId).toBe("1");
    expect(getByText("Old House Restoration")).toBeInTheDocument();
    expect(getByText("Active old-house community")).toBeInTheDocument();
    expect(getByText("27")).toBeInTheDocument();
    expect(getByText("Threads")).toBeInTheDocument();
  });

  it("does not display r/ prefix in visible text", () => {
    const { container } = render(ForumRow, { props: { forum: forum() } });
    expect(container.textContent ?? "").not.toMatch(/r\/\w+/);
  });

  it("does not show a platform-name subtext under the forum name", () => {
    const { queryByText } = render(ForumRow, { props: { forum: forum({ kind: "xenforo" }) } });
    expect(queryByText("XenForo")).toBeNull();
    expect(queryByText("Reddit")).toBeNull();
    expect(queryByText("RSS")).toBeNull();
  });

  it("renders gray 'feed temporarily unavailable' caption when last_error is set", () => {
    const { getByText, queryByText } = render(ForumRow, {
      props: {
        forum: forum({
          last_error: "http 403 fetching https://x",
          latest_thread_title: null,
          latest_thread_author: null,
          latest_thread_at: null,
          latest_thread_url: null,
        }),
      },
    });
    expect(getByText("feed temporarily unavailable")).toBeInTheDocument();
    // Old red "error" pill text should be gone.
    expect(queryByText("error")).toBeNull();
  });

  it("renders unread state with open folder + bold name", () => {
    const { getByTestId } = render(ForumRow, {
      props: { forum: forum({ unread: true }) },
    });
    const row = getByTestId("forum-row");
    expect(row.className).toContain("unread");
    expect(row.textContent).toContain("📂");
  });

  it("renders read state with closed folder", () => {
    const { getByTestId } = render(ForumRow, {
      props: { forum: forum({ unread: false }) },
    });
    const row = getByTestId("forum-row");
    expect(row.className).not.toContain("unread");
    expect(row.textContent).toContain("📁");
  });

  it("renders last-post block with title, author, absolute date", () => {
    // Use a timestamp from ~30 days ago so the formatter renders an absolute
    // date ("12 Mar 2026, 14:30") rather than the relative "Today, HH:MM"
    // that fires for same-day posts.
    const thirtyDaysAgo = Math.floor(Date.now() / 1000) - 30 * 86400;
    const { getByText, container } = render(ForumRow, {
      props: { forum: forum({ latest_thread_at: thirtyDaysAgo }) },
    });
    expect(getByText(/Re: 1923 Foursquare/)).toBeInTheDocument();
    expect(getByText("oldhouselove")).toBeInTheDocument();
    // vB4 classic format is MM-DD-YYYY, HH:MM AM/PM for older posts.
    expect(container.innerHTML).toMatch(/\d{2}-\d{2}-\d{4}, \d{2}:\d{2} [AP]M/);
  });

  it("renders no-posts-yet placeholder when latest_thread_title is null", () => {
    const { getByText } = render(ForumRow, {
      props: {
        forum: forum({
          latest_thread_title: null,
          latest_thread_author: null,
          latest_thread_at: null,
          latest_thread_url: null,
        }),
      },
    });
    expect(getByText("No posts yet")).toBeInTheDocument();
  });
});
