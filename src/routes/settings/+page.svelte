<script lang="ts">
  import { onMount } from "svelte";
  import { beforeNavigate } from "$app/navigation";
  import { api } from "$lib/api";
  import { fmtVbFooterTime } from "$lib/types";
  import SearchBox from "$lib/components/SearchBox.svelte";
  import FavoritesLink from "$lib/components/FavoritesLink.svelte";
  import HistoryLink from "$lib/components/HistoryLink.svelte";
  import YamlEditor from "$lib/components/YamlEditor.svelte";
  import AIAssistantDrawer from "$lib/components/AIAssistantDrawer.svelte";
  import ThemePicker from "$lib/components/ThemePicker.svelte";
  import { hasErrors } from "$lib/feeds-schema";
  import { theme } from "$lib/theme/store";
  import type { Diagnostic } from "@codemirror/lint";

  let yaml = $state("");
  let originalYaml = $state("");
  let path = $state("");
  let loading = $state(true);
  let saving = $state(false);
  let error = $state<string | null>(null);
  let savedAt = $state<number | null>(null);
  let now = $state(new Date());
  let diagnostics = $state<Diagnostic[]>([]);
  let wizardOpen = $state(false);
  // When SvelteKit's beforeNavigate fires on a dirty editor, we stash the
  // intended destination and pop the in-app "discard?" modal. The user's
  // answer either re-navigates (allowing the unsaved change to drop) or
  // dismisses the modal (staying put). We bypass the guard during programmatic
  // post-save / post-revert nav with `bypassGuard`.
  let pendingNavTo = $state<URL | null>(null);
  let bypassGuard = $state(false);

  // Theme picker has its own pending-pick dirty state. Wire its dirty signal
  // up so the page's Save / Revert / nav-guard treat YAML + theme uniformly.
  let themePicker: ThemePicker | undefined = $state();
  let themeDirty = $state(false);

  let yamlDirty = $derived(yaml !== originalYaml);
  let dirty = $derived(yamlDirty || themeDirty);
  let errorCount = $derived(diagnostics.filter((d) => d.severity === "error").length);
  let warningCount = $derived(diagnostics.filter((d) => d.severity === "warning").length);
  // YAML errors block YAML save, but a theme-only pending change is still
  // saveable (it doesn't go through the YAML validator).
  let canSave = $derived(dirty && !saving && (yamlDirty ? errorCount === 0 : true));

  async function load() {
    loading = true;
    try {
      const [y, p] = await Promise.all([api.getFeedsYaml(), api.getFeedsPath()]);
      yaml = y;
      originalYaml = y;
      path = p;
      error = null;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function save() {
    if (!canSave) return;
    saving = true;
    error = null;
    try {
      if (yamlDirty) {
        // Backend auto-formats and returns the canonical text; sync the editor
        // to it so what the user sees matches what's on disk.
        const formatted = await api.saveFeedsYaml(yaml);
        yaml = formatted;
        originalYaml = formatted;
      }
      // Theme picker commit is local-only (localStorage), so it's safe to
      // run after a successful YAML save.
      themePicker?.commit();
      savedAt = Date.now();
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  function reset() {
    yaml = originalYaml;
    themePicker?.revert();
    error = null;
  }

  function keyHandler(e: KeyboardEvent) {
    // Cmd/Ctrl+S to save when valid.
    if ((e.metaKey || e.ctrlKey) && (e.key === "s" || e.key === "S")) {
      e.preventDefault();
      if (canSave) void save();
    }
  }

  beforeNavigate((nav) => {
    if (!dirty || bypassGuard) return;
    if (!nav.to) return;
    // Cancel the pending navigation and stash its target. The modal's
    // "Discard & leave" button re-fires the nav with the guard bypassed.
    nav.cancel();
    pendingNavTo = nav.to.url;
  });

  function discardAndLeave() {
    if (!pendingNavTo) return;
    const url = pendingNavTo;
    pendingNavTo = null;
    bypassGuard = true;
    // location.assign respects SvelteKit's client router via the runtime's
    // history hook on same-origin URLs.
    window.location.assign(url.toString());
  }

  function cancelLeave() {
    pendingNavTo = null;
  }

  function pendingKey(e: KeyboardEvent) {
    if (!pendingNavTo) return;
    if (e.key === "Escape") {
      e.preventDefault();
      cancelLeave();
    } else if (e.key === "Enter") {
      e.preventDefault();
      discardAndLeave();
    }
  }

  onMount(() => {
    void load();
    window.addEventListener("keydown", keyHandler);
    window.addEventListener("keydown", pendingKey);
    // Browser-level guard for Cmd+R / closing the webview without a SvelteKit
    // nav (e.g. Tauri "close" → the OS asks the user via standard
    // beforeunload UI). Tauri's macOS Cmd+Q close doesn't go through this,
    // but reload does.
    function onBeforeUnload(e: BeforeUnloadEvent) {
      if (!dirty || bypassGuard) return;
      e.preventDefault();
      e.returnValue = "";
    }
    window.addEventListener("beforeunload", onBeforeUnload);
    return () => {
      window.removeEventListener("keydown", keyHandler);
      window.removeEventListener("keydown", pendingKey);
      window.removeEventListener("beforeunload", onBeforeUnload);
    };
  });
</script>

<div class="vb-banner">
  <h1><a href="/" title="Back to home">Settings — Feeds</a></h1>
  <div class="vb-actions">
    <HistoryLink />
    <FavoritesLink />
    <SearchBox />
    <a class="vb-btn" href="/">&larr; Home</a>
  </div>
</div>

<div class="vb-content">
  <div class="vb-toolbar">
    <span>
      <b>feeds.yaml</b>
      <span class="vb-toolbar-sep">·</span>
      <span style="color:var(--vb-muted)">{path || "loading…"}</span>
    </span>
    <span class="vb-spacer"></span>

    {#if !loading}
      {#if errorCount > 0}
        <span class="settings-err">{errorCount} {errorCount === 1 ? "error" : "errors"}</span>
      {/if}
      {#if warningCount > 0}
        <span class="settings-warn">{warningCount} {warningCount === 1 ? "warning" : "warnings"}</span>
      {/if}
      {#if errorCount === 0 && warningCount === 0}
        <span class="settings-ok">✓ valid</span>
      {/if}
    {/if}

    {#if dirty}
      <span class="settings-dirty">unsaved</span>
    {:else if savedAt}
      <span class="settings-saved">saved · restart for changes to apply</span>
    {/if}
    <span class="settings-theme-label" title="Site-wide theme. Hover any option to preview live; click to set as pending; Save to commit.">Theme:</span>
    <ThemePicker bind:this={themePicker} onDirtyChange={(d) => (themeDirty = d)} />

    <button
      class="vb-btn vb-btn-wizard"
      onclick={() => (wizardOpen = true)}
      title="Open the AI wizard to add/remove feeds conversationally"
    >
      <span class="vb-btn-wizard-spark">✦</span> Wizard
    </button>
    <button class="vb-btn" onclick={reset} disabled={!dirty || saving}>Revert</button>
    <button
      class="vb-btn"
      onclick={save}
      disabled={!canSave}
      title={errorCount > 0 ? `Fix ${errorCount} error(s) before saving` : "Save (⌘S)"}
    >
      {saving ? "Saving…" : "Save"}
    </button>
  </div>

  {#if loading}
    <div class="vb-group"><div class="vb-empty">Loading…</div></div>
  {:else if error}
    <div class="vb-group">
      <div class="vb-empty" style="color:var(--vb-pill-text);white-space:pre-wrap;">{error}</div>
    </div>
  {:else}
    <YamlEditor
      value={yaml}
      onChange={(next) => (yaml = next)}
      onDiagnostics={(d) => (diagnostics = d)}
      dark={$theme === "dark"}
    />

    {#if diagnostics.length > 0}
      <div class="settings-diags">
        {#each diagnostics as d, i (i)}
          <div class="settings-diag settings-diag-{d.severity}">
            <span class="settings-diag-sev">{d.severity}</span>
            <span class="settings-diag-msg">{d.message}</span>
          </div>
        {/each}
      </div>
    {/if}

    <div class="settings-help">
      <b>Forum entry shapes:</b>
      <pre>- kind: generic_rss
  title: Some Forum
  url: https://example.com/forum/feed.rss
  poll_interval_s: 1800   # optional — defaults to 30 min

- kind: reddit
  sub: chicago
  title: r/chicago       # optional — defaults to capitalized sub

- kind: reddit
  user: spez
  title: u/spez          # optional

- kind: reddit
  sub: chicago
  theme: myspace-2006    # optional — override site-wide skin for this forum</pre>
      <p>
        Allowed kinds: <code>reddit, xenforo, vbulletin, discourse, phpbb, smf, ipb, ubiquiti, generic_rss</code>.
        Errors block save; warnings don't. <b>Restart feedBulletin</b> after saving — the poller captured the
        previous config at startup. Forums that disappear from this file will be pruned on next run.
      </p>
    </div>
  {/if}

  <div class="vb-footer">
    <div class="vb-footer-left">{fmtVbFooterTime(now)}</div>
    <div class="vb-footer-right">Powered by feedBulletin v0.1</div>
  </div>
</div>

<AIAssistantDrawer
  open={wizardOpen}
  onClose={() => {
    wizardOpen = false;
    // Reload the YAML so anything the wizard wrote shows up in the editor.
    void load();
  }}
/>

{#if pendingNavTo}
  <div
    class="unsaved-backdrop"
    role="button"
    tabindex="-1"
    aria-label="Cancel"
    onclick={cancelLeave}
    onkeydown={(e) => { if (e.key === "Enter" || e.key === " ") cancelLeave(); }}
  >
    <div
      class="unsaved-modal"
      role="dialog"
      aria-modal="true"
      aria-label="Unsaved changes"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.stopPropagation()}
      tabindex="-1"
    >
      <div class="unsaved-head"><h2>Unsaved Changes</h2></div>
      <div class="unsaved-body">
        <p>You have unsaved edits in <b>feeds.yaml</b>. Leaving now will discard them.</p>
        <p class="unsaved-sub">Save first, or discard and leave.</p>
      </div>
      <div class="unsaved-foot">
        <button class="vb-btn" onclick={cancelLeave}>Stay (Esc)</button>
        <button
          class="vb-btn"
          onclick={async () => {
            if (!canSave) return;
            await save();
            // Save succeeded → dirty becomes false → re-fire the nav.
            if (!dirty) discardAndLeave();
          }}
          disabled={!canSave}
          title={canSave ? "Save then leave" : (errorCount > 0 ? `Fix ${errorCount} error(s) first` : "Nothing to save")}
        >
          {saving ? "Saving…" : "Save & leave"}
        </button>
        <button class="vb-btn unsaved-discard" onclick={discardAndLeave}>Discard & leave (Enter)</button>
      </div>
    </div>
  </div>
{/if}

<style>
  /* Wizard button: gradient + sparkle so it's visually distinct from
     the plain Save / Revert toolbar buttons. */
  .vb-btn-wizard {
    background: linear-gradient(
      135deg,
      color-mix(in srgb, var(--vb-link) 80%, #b76aff),
      var(--vb-link)
    );
    color: white;
    border-color: var(--vb-link);
    font-weight: bold;
  }
  .vb-btn-wizard:hover {
    background: linear-gradient(
      135deg,
      color-mix(in srgb, var(--vb-link-hover) 80%, #b76aff),
      var(--vb-link-hover)
    );
  }
  .vb-btn-wizard-spark {
    color: #f8d96e;
    text-shadow: 0 0 4px rgba(248, 217, 110, 0.6);
    margin-right: 2px;
  }
  .settings-diags {
    margin-top: 12px;
    border: 1px solid var(--vb-border);
    border-radius: 3px;
    overflow: hidden;
    font-family: "SF Mono", Menlo, Consolas, monospace;
    font-size: 11px;
  }
  .settings-diag {
    padding: 4px 10px;
    border-bottom: 1px solid var(--vb-border);
    display: flex;
    gap: 10px;
    align-items: baseline;
    background: var(--vb-content-bg);
  }
  .settings-diag:last-child { border-bottom: none; }
  .settings-diag-error { background: color-mix(in srgb, #d33 8%, var(--vb-content-bg)); }
  .settings-diag-warning { background: color-mix(in srgb, #c8a04a 8%, var(--vb-content-bg)); }
  .settings-diag-sev {
    width: 60px;
    flex-shrink: 0;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    font-size: 10px;
  }
  .settings-diag-error .settings-diag-sev { color: #d33; }
  .settings-diag-warning .settings-diag-sev { color: #c8a04a; }
  .settings-diag-msg { color: var(--vb-page-text); flex: 1; }
  .settings-help {
    margin-top: 12px;
    padding: 12px 14px;
    background: var(--vb-row-bg-b);
    border: 1px solid var(--vb-border);
    border-radius: 3px;
    font-size: 11px;
    color: var(--vb-meta);
  }
  .settings-help pre {
    margin: 6px 0;
    padding: 8px;
    background: var(--vb-content-bg);
    border: 1px solid var(--vb-border);
    border-radius: 3px;
    font-family: "SF Mono", Menlo, Consolas, monospace;
    overflow-x: auto;
    color: var(--vb-page-text);
  }
  .settings-help code {
    font-family: "SF Mono", Menlo, Consolas, monospace;
    padding: 1px 4px;
    background: var(--vb-content-bg);
    border-radius: 2px;
  }
  .settings-err {
    color: #d33;
    font-weight: 700;
    margin-right: 8px;
  }
  .settings-warn {
    color: #c8a04a;
    font-weight: 700;
    margin-right: 8px;
  }
  .settings-ok {
    color: var(--vb-link);
    margin-right: 8px;
  }
  .settings-dirty {
    color: var(--vb-pill-text);
    font-style: italic;
    margin-right: 8px;
  }
  .settings-saved {
    color: var(--vb-link);
    margin-right: 8px;
  }

  /* "Unsaved changes" confirmation modal */
  .unsaved-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 6000;
    backdrop-filter: blur(2px);
  }
  .unsaved-modal {
    width: min(480px, 90vw);
    background: var(--vb-content-bg);
    color: var(--vb-page-text);
    border: 1px solid var(--vb-border);
    border-radius: 6px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
    font-family: Verdana, Tahoma, "Lucida Sans", sans-serif;
    font-size: 12px;
    display: flex;
    flex-direction: column;
  }
  .unsaved-head {
    padding: 10px 14px;
    background: linear-gradient(to bottom, var(--vb-banner-grad-from), var(--vb-banner-grad-to));
    color: var(--vb-banner-text);
    border-bottom: 1px solid var(--vb-border);
    border-top-left-radius: 6px;
    border-top-right-radius: 6px;
  }
  .unsaved-head h2 {
    margin: 0;
    font-size: 13px;
    font-weight: bold;
    letter-spacing: 0.3px;
  }
  .unsaved-body {
    padding: 14px 16px;
  }
  .unsaved-body p { margin: 0 0 8px; }
  .unsaved-body p:last-child { margin-bottom: 0; }
  .unsaved-sub { color: var(--vb-muted); font-size: 11px; }
  .unsaved-foot {
    padding: 10px 14px;
    border-top: 1px solid var(--vb-border);
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .unsaved-discard {
    background: color-mix(in srgb, var(--vb-pill-text) 90%, transparent);
    color: white;
    border-color: var(--vb-pill-text);
  }
  .settings-theme-label {
    color: var(--vb-muted);
    font-size: 11px;
    margin: 0 2px 0 8px;
  }
</style>
