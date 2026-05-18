<script lang="ts">
  import { page } from "$app/state";
  let isOnHistory = $derived(String(page.url.pathname) === "/history");
</script>

<a
  class="vb-history-link"
  class:active={isOnHistory}
  href="/history"
  title="Recently viewed threads"
>
  History
</a>

<style>
  .vb-history-link {
    display: inline-flex;
    align-items: center;
    height: 20px;
    padding: 0 6px;
    font-family: Verdana, Tahoma, "Lucida Sans", sans-serif;
    font-size: 11px;
    color: var(--vb-banner-text);
    text-decoration: none;
    letter-spacing: 0.2px;
    opacity: 0.85;
    white-space: nowrap;
  }
  .vb-history-link:hover {
    text-decoration: underline;
    opacity: 1;
  }
  .vb-history-link.active {
    text-decoration: underline;
    opacity: 1;
    font-weight: bold;
  }
</style>
