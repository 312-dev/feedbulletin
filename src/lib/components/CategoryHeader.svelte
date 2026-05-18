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

<!-- Whole-strip click is the canonical toggle (matches vB4's UX where any
     bit of the category bar collapses it). The inner button is preserved
     for keyboard / screen-reader users; its onclick stops propagation so we
     don't double-fire. The anchor inside is also stop-propagation, so
     clicking the category name still navigates without toggling. -->
<div
  class="vb-cat"
  role="button"
  tabindex="0"
  aria-expanded={open}
  data-testid="category-header"
  onclick={onToggle}
  onkeydown={(e) => {
    if (e.key === "Enter" || e.key === " ") { e.preventDefault(); onToggle(); }
  }}
>
  <button
    type="button"
    class="cat-toggle"
    aria-label={open ? "Collapse category" : "Expand category"}
    onclick={(e) => { e.stopPropagation(); onToggle(); }}
  >
    <span class="caret">{open ? "▼" : "▶"}</span>
  </button>
  <a class="cat-name" href={`/category/${id}`} onclick={(e) => e.stopPropagation()}>{name}</a>
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
