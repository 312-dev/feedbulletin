<script lang="ts">
  import { onDestroy } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { api, type ChatMessage } from "$lib/api";

  type Props = {
    open: boolean;
    onClose: () => void;
  };
  let { open, onClose }: Props = $props();

  type Bubble = {
    role: "user" | "assistant";
    text: string;
  };

  // The on-screen rendering is a flattened sequence of bubbles. The full
  // Anthropic message history (including tool calls / tool results) lives
  // in `history` and is what we round-trip back to the backend each turn.
  let history = $state<ChatMessage[]>([]);
  let bubbles = $state<Bubble[]>([]);
  let input = $state("");
  let sending = $state(false);
  let progress = $state<string>("");
  let modified = $state(false);
  let error = $state<string | null>(null);
  let reverting = $state(false);

  let progressUnlisten: UnlistenFn | null = null;
  let scroller: HTMLDivElement | null = $state(null);

  // Attach progress listener once.
  $effect(() => {
    if (progressUnlisten) return;
    listen<{ kind: string; data: Record<string, unknown> }>("feeds_agent_progress", (e) => {
      const { kind, data } = e.payload;
      if (kind === "thinking") progress = "Thinking…";
      else if (kind === "tool") {
        const name = data?.name as string | undefined;
        progress = describeTool(name);
      }
      else if (kind === "applied") {
        progress = "Applying change…";
        modified = true;
      }
    }).then((un) => (progressUnlisten = un));
  });

  function describeTool(name: string | undefined): string {
    switch (name) {
      case "web_search": return "Searching the web…";
      case "list_categories": return "Listing categories…";
      case "list_forums_in_category": return "Listing forums…";
      case "get_forum": return "Looking up forum details…";
      case "add_or_update_forum": return "Adding/updating forum…";
      case "remove_forum": return "Removing forum…";
      case "add_category": return "Adding category…";
      case "rename_category": return "Renaming category…";
      case "remove_category": return "Removing category…";
      default: return name ? `Calling ${name}…` : "Working…";
    }
  }

  function autoScroll() {
    requestAnimationFrame(() => {
      if (scroller) scroller.scrollTop = scroller.scrollHeight;
    });
  }

  async function send() {
    const msg = input.trim();
    if (!msg || sending) return;
    bubbles = [...bubbles, { role: "user", text: msg }];
    input = "";
    sending = true;
    progress = "Thinking…";
    error = null;
    autoScroll();
    try {
      const result = await api.feedsAgentSendMessage(history, msg);
      history = result.messages;
      bubbles = [...bubbles, { role: "assistant", text: result.final_text }];
      if (result.did_modify_feeds) modified = true;
    } catch (e) {
      error = String(e);
      bubbles = [...bubbles, { role: "assistant", text: `(error) ${String(e)}` }];
    } finally {
      sending = false;
      progress = "";
      autoScroll();
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void send();
    } else if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    }
  }

  async function revert() {
    if (reverting) return;
    reverting = true;
    try {
      const restored = await api.feedsAgentRevert();
      if (restored == null) {
        bubbles = [...bubbles, { role: "assistant", text: "No snapshot available to revert to." }];
      } else {
        bubbles = [...bubbles, { role: "assistant", text: "Reverted feeds.yaml to the previous snapshot." }];
      }
    } catch (e) {
      bubbles = [...bubbles, { role: "assistant", text: `(revert failed) ${String(e)}` }];
    } finally {
      reverting = false;
      autoScroll();
    }
  }

  function newConversation() {
    history = [];
    bubbles = [];
    progress = "";
    error = null;
  }

  onDestroy(() => {
    if (progressUnlisten) progressUnlisten();
  });
</script>

{#if open}
  <div
    class="ai-backdrop"
    role="button"
    tabindex="-1"
    aria-label="Close wizard"
    onclick={onClose}
    onkeydown={(e) => { if (e.key === "Enter" || e.key === " ") onClose(); }}
  ></div>
  <div
    class="ai-drawer"
    role="dialog"
    aria-modal="true"
    aria-label="Feed wizard"
    tabindex="-1"
    onkeydown={onKey}
  >
    <header class="ai-head">
      <div class="ai-head-title">
        <span class="ai-spark">✦</span>
        <span>Wizard</span>
      </div>
      <div class="ai-head-actions">
        <button class="ai-head-btn" onclick={newConversation} title="Start a new conversation">New</button>
        <button class="ai-head-close" onclick={onClose} title="Close (Esc)" aria-label="Close">×</button>
      </div>
    </header>

    <div class="ai-scroller" bind:this={scroller}>
      {#if bubbles.length === 0}
        <div class="ai-welcome">
          <p><b>Tell me what forum sources you want to add.</b></p>
          <p class="ai-welcome-sub">
            I can search the web for RSS endpoints and configure them for you. Try:
          </p>
          <ul class="ai-welcome-examples">
            <li><em>"Add Hacker News best-of"</em></li>
            <li><em>"Add r/homeassistant"</em></li>
            <li><em>"Add the StackOverflow front page"</em></li>
            <li><em>"Remove BimmerPost — M3 G80/G82"</em></li>
            <li><em>"Add a category 'Investing' with Bogleheads"</em></li>
          </ul>
        </div>
      {/if}

      {#each bubbles as b, i (i)}
        <div class="ai-bubble" class:ai-user={b.role === "user"} class:ai-assistant={b.role === "assistant"}>
          <div class="ai-role">{b.role === "user" ? "You" : "Assistant"}</div>
          <div class="ai-text">{b.text}</div>
        </div>
      {/each}

      {#if sending}
        <div class="ai-bubble ai-assistant ai-pending">
          <div class="ai-role">Assistant</div>
          <div class="ai-text"><em>{progress || "Working…"}</em></div>
        </div>
      {/if}
    </div>

    {#if modified}
      <div class="ai-banner-modified">
        <span>feeds.yaml changed — <b>restart feedBulletin</b> for changes to take effect.</span>
        <button class="ai-banner-btn" onclick={revert} disabled={reverting}>
          {reverting ? "Reverting…" : "Undo last"}
        </button>
      </div>
    {/if}

    {#if error}
      <div class="ai-banner-error">{error}</div>
    {/if}

    <footer class="ai-foot">
      <textarea
        class="ai-input"
        placeholder="Describe what you want to add or change…"
        bind:value={input}
        disabled={sending}
        rows="3"
      ></textarea>
      <button class="ai-send" onclick={send} disabled={sending || !input.trim()}>
        {sending ? "…" : "Send"}
      </button>
    </footer>
  </div>
{/if}

<style>
  .ai-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.35);
    z-index: 5000;
    backdrop-filter: blur(2px);
  }
  .ai-drawer {
    position: fixed;
    top: 0;
    right: 0;
    bottom: 0;
    width: min(440px, 90vw);
    z-index: 5001;
    background: var(--vb-content-bg);
    color: var(--vb-page-text);
    border-left: 1px solid var(--vb-border);
    box-shadow: -8px 0 24px rgba(0, 0, 0, 0.3);
    display: flex;
    flex-direction: column;
    font-family: Verdana, Tahoma, "Lucida Sans", sans-serif;
    font-size: 12px;
    animation: ai-slide-in 200ms ease;
  }
  @keyframes ai-slide-in {
    from { transform: translateX(100%); }
    to { transform: translateX(0); }
  }
  .ai-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 14px;
    background: linear-gradient(to bottom, var(--vb-banner-grad-from), var(--vb-banner-grad-to));
    color: var(--vb-banner-text);
    border-bottom: 1px solid var(--vb-border);
  }
  .ai-head-title {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-weight: bold;
    font-size: 13px;
    letter-spacing: 0.3px;
  }
  .ai-spark {
    color: #f8d96e;
    text-shadow: 0 0 6px rgba(248, 217, 110, 0.6);
  }
  .ai-head-actions {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .ai-head-btn {
    background: transparent;
    border: 1px solid color-mix(in srgb, var(--vb-banner-text) 50%, transparent);
    color: var(--vb-banner-text);
    padding: 3px 10px;
    border-radius: 3px;
    cursor: pointer;
    font-family: inherit;
    font-size: 11px;
  }
  .ai-head-btn:hover {
    background: color-mix(in srgb, var(--vb-banner-text) 12%, transparent);
  }
  .ai-head-close {
    background: transparent;
    border: 0;
    color: var(--vb-banner-text);
    font-size: 22px;
    line-height: 1;
    cursor: pointer;
    padding: 0 4px;
  }
  .ai-head-close:hover {
    color: color-mix(in srgb, var(--vb-banner-text) 70%, transparent);
  }
  .ai-scroller {
    flex: 1;
    overflow-y: auto;
    padding: 12px 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .ai-welcome {
    color: var(--vb-meta);
    padding: 8px 12px;
    background: var(--vb-row-bg-b);
    border: 1px dashed var(--vb-border);
    border-radius: 3px;
  }
  .ai-welcome p { margin: 0 0 6px; }
  .ai-welcome-sub { font-size: 11px; }
  .ai-welcome-examples {
    margin: 4px 0 0;
    padding-left: 18px;
    font-size: 11px;
  }
  .ai-welcome-examples li { margin: 2px 0; }
  .ai-bubble {
    padding: 8px 10px;
    border-radius: 4px;
    border: 1px solid var(--vb-border);
    background: var(--vb-content-bg);
    max-width: 95%;
  }
  .ai-user {
    align-self: flex-end;
    background: color-mix(in srgb, var(--vb-link) 14%, var(--vb-content-bg));
    border-color: color-mix(in srgb, var(--vb-link) 40%, var(--vb-border));
  }
  .ai-assistant {
    align-self: flex-start;
    background: var(--vb-row-bg-b);
  }
  .ai-pending { opacity: 0.85; }
  .ai-role {
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--vb-very-muted);
    margin-bottom: 4px;
  }
  .ai-text {
    white-space: pre-wrap;
    word-wrap: break-word;
    line-height: 1.5;
  }
  .ai-banner-modified {
    padding: 8px 12px;
    background: color-mix(in srgb, #f8d96e 18%, var(--vb-content-bg));
    border-top: 1px solid var(--vb-border);
    color: var(--vb-page-text);
    font-size: 11px;
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .ai-banner-btn {
    margin-left: auto;
    padding: 3px 10px;
    border: 1px solid var(--vb-border);
    background: var(--vb-content-bg);
    color: var(--vb-page-text);
    border-radius: 3px;
    cursor: pointer;
    font-family: inherit;
    font-size: 11px;
  }
  .ai-banner-btn:hover { background: var(--vb-row-hover); }
  .ai-banner-error {
    padding: 8px 12px;
    background: color-mix(in srgb, var(--vb-pill-text) 12%, var(--vb-content-bg));
    color: var(--vb-pill-text);
    border-top: 1px solid var(--vb-border);
    font-size: 11px;
    word-break: break-word;
  }
  .ai-foot {
    border-top: 1px solid var(--vb-border);
    padding: 10px 14px;
    display: flex;
    gap: 8px;
    background: var(--vb-content-bg);
  }
  .ai-input {
    flex: 1;
    padding: 8px 10px;
    border: 1px solid var(--vb-border);
    border-radius: 3px;
    background: var(--vb-content-bg);
    color: var(--vb-page-text);
    font-family: inherit;
    font-size: 12px;
    resize: vertical;
    min-height: 50px;
    max-height: 200px;
  }
  .ai-input:focus { outline: none; border-color: var(--vb-link); }
  .ai-send {
    align-self: flex-end;
    padding: 6px 14px;
    border: 1px solid var(--vb-link);
    background: var(--vb-link);
    color: white;
    border-radius: 3px;
    cursor: pointer;
    font-family: inherit;
    font-size: 12px;
    font-weight: bold;
  }
  .ai-send:hover:not(:disabled) {
    background: var(--vb-link-hover);
  }
  .ai-send:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
