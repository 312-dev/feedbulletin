<script lang="ts">
  type Props = {
    id: number;
    name: string;
    open: boolean;
    forumCount: number;
    onToggle?: () => void;
  };
  let { id, name, open, forumCount, onToggle = () => {} }: Props = $props();
</script>

<div
  class="vb-cat"
  aria-expanded={open}
  data-testid="category-header"
>
  <button
    type="button"
    class="cat-toggle"
    aria-label={open ? "Collapse category" : "Expand category"}
    onclick={onToggle}
  >
    <span class="caret">{open ? "▼" : "▶"}</span>
  </button>
  <a class="cat-name" href={`/category/${id}`}>{name}</a>
  <span style="opacity: 0.75; font-weight: normal; margin-left: 6px;">
    ({forumCount} {forumCount === 1 ? "forum" : "forums"})
  </span>
</div>

<style>
  .cat-toggle {
    background: transparent;
    border: none;
    color: inherit;
    font: inherit;
    cursor: pointer;
    padding: 0 4px 0 0;
  }
  .cat-name {
    color: inherit;
    text-decoration: none;
  }
  .cat-name:hover {
    text-decoration: underline;
  }
</style>
