<script lang="ts">
  import { page } from "$app/state";
  let isOnFavorites = $derived(page.url.pathname === "/favorites");
</script>

<a
  class="vb-fav-link"
  class:active={isOnFavorites}
  href="/favorites"
  title="Posts you've favorited — kept cached indefinitely"
>
  My Favorites
</a>

<style>
  .vb-fav-link {
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
  .vb-fav-link:hover {
    text-decoration: underline;
    opacity: 1;
  }
  .vb-fav-link.active {
    text-decoration: underline;
    opacity: 1;
    font-weight: bold;
  }
</style>
