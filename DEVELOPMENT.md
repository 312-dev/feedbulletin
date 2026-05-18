# Forum Reader — development notes

Local vBulletin-style aggregator for Reddit and forum RSS feeds. v0.

## Prerequisites

- Rust toolchain (rustup): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y`
- Node 22+ and pnpm 9+ (already installed on this machine)
- macOS native toolchain (`xcode-select --install` if `cc` is missing)

The user's `~/.local/bin/cc` shadows the system C compiler. When invoking cargo
manually, sanitize PATH so the real `cc` is picked up:

```sh
env PATH="/usr/bin:/bin:/usr/sbin:/sbin:$HOME/.cargo/bin" cargo build
```

`pnpm tauri dev` is fine because cargo finds `cc` through `/usr/bin` when the
shell is invoked normally — the shadowing only bites direct `cargo` calls from
this Claude environment.

## Run the app

```sh
cd ~/Repos/forum-reader
pnpm install              # one-time
pnpm tauri dev            # launches the desktop window with hot reload
```

The first run takes ~50s to compile Tauri + sqlx + reqwest. Subsequent runs
incremental-compile in ~2-3s.

Feeds.yaml is read from the project root in dev. Database lives at
`~/Library/Application Support/com.grayada.forumreader/forum-reader.sqlite`.

## Run the tests

```sh
# Rust backend (parsers, db, config)
cd src-tauri
cargo test

# Frontend (Vitest)
cd ..
pnpm test

# Playwright responsive smoke test (requires `pnpm tauri dev` running)
pnpm test:e2e
```

## Visual regression / screenshots

`e2e/screenshot.spec.ts` is excluded from `pnpm test:e2e` but is the canonical
way to refresh the design baselines. Run it manually with the dev server up:

```sh
pnpm exec playwright test e2e/screenshot.spec.ts
# writes index-{1200,480}.png and feed-{1200,480}.png into e2e/screenshots/
```

## Production build

```sh
pnpm tauri build         # produces a .app and a .dmg under src-tauri/target/release/bundle
```

## Architecture notes

- **TLS**: reqwest uses `default-tls` (Secure Transport on macOS). rustls'
  fingerprint trips Reddit's bot detection and yields 403 against
  `old.reddit.com/r/<sub>.json`; Secure Transport passes.
- **Reddit Accept header**: must be exactly `application/json`. A multi-type
  Accept header with `application/rss+xml` also yields 403.
- **Cloudflare-protected feeds** (LTHForum in the seed set): respond 403 to
  reqwest regardless of UA. The plan calls for logging a warning and skipping.
- **SvelteKit** is used (not bare Svelte) with file-based routing under
  `src/routes/`. `+page.svelte` is the forum index, `feed/[id]/+page.svelte`
  is the feed view.
- **No git init** intentionally — review the diff and commit yourself.

## v1 work queue

1. Thread view with embedded WebView + prev/next sibling nav, keyboard shortcuts
2. Remaining parsers: discourse, vbulletin, phpbb, smf, ipb, ubiquiti
3. Per-feed read state + mark-read actions persisted to SQLite
4. Group aggregate view (`/group/[id]`)
5. Hot sort where the source supports it (Reddit `/hot.json`, Discourse `/top.rss`)
