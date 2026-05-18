<script lang="ts">
  type Crumb = { label: string; href?: string };
  type Props = { crumbs: Crumb[] };
  let { crumbs }: Props = $props();
</script>

<div class="vb-breadcrumb" data-testid="breadcrumb">
  {#each crumbs as c, i}
    {#if i > 0}<span class="sep"> &gt; </span>{/if}
    {#if c.href}
      <a href={c.href}>{c.label}</a>
    {:else}
      <span>{c.label}</span>
    {/if}
  {/each}
</div>
