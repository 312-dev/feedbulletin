import { render, fireEvent } from "@testing-library/svelte";
import { describe, expect, it, vi } from "vitest";
import CategoryHeader from "./CategoryHeader.svelte";

describe("CategoryHeader", () => {
  it("renders name and forum count", () => {
    const { getByText, getByTestId } = render(CategoryHeader, {
      props: { id: 1, name: "Cars", forumCount: 3, open: true },
    });
    expect(getByText("Cars")).toBeInTheDocument();
    expect(getByText("(3 forums)")).toBeInTheDocument();
    expect(getByTestId("category-header")).toHaveAttribute("aria-expanded", "true");
  });

  it("singularizes label when forumCount is 1", () => {
    const { getByText } = render(CategoryHeader, {
      props: { id: 1, name: "Solo", forumCount: 1, open: true },
    });
    expect(getByText("(1 forum)")).toBeInTheDocument();
  });

  it("shows ▶ when closed and ▼ when open", () => {
    const closed = render(CategoryHeader, {
      props: { id: 1, name: "x", forumCount: 1, open: false },
    });
    expect(closed.container.textContent).toContain("▶");
    const open = render(CategoryHeader, {
      props: { id: 1, name: "x", forumCount: 1, open: true },
    });
    expect(open.container.textContent).toContain("▼");
  });

  it("calls onToggle on click", async () => {
    const onToggle = vi.fn();
    const { getByTestId } = render(CategoryHeader, {
      props: { id: 1, name: "Cars", forumCount: 1, open: true, onToggle },
    });
    await fireEvent.click(getByTestId("category-header"));
    expect(onToggle).toHaveBeenCalled();
  });
});
