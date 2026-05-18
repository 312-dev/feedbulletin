# Contributing to feedBulletin

Thanks for considering a contribution. This document covers the workflow,
branch strategy, PR expectations, and how the project is structured so you
can get oriented quickly.

## Quick links

- **Bugs**: [open an issue](https://github.com/312-dev/feedbulletin/issues/new?template=bug_report.md) with the steps to reproduce.
- **Feature ideas**: [open an issue](https://github.com/312-dev/feedbulletin/issues/new?template=feature_request.md) — let's discuss before you write code so we can align on scope.
- **Security issues**: see [SECURITY.md](SECURITY.md). Do not file a public issue.
- **New site profile**: [drop a markdown file](src-tauri/src/scrape/seed_profiles/) and open a PR.

## Branch strategy

Trunk-based, with short-lived feature branches.

- **`main`** is the only long-lived branch. It's always releasable.
- **Feature branches** are named `feat/<short-slug>`, `fix/<short-slug>`, `chore/<short-slug>`, etc. They branch from `main` and target `main`. Rebase before merging.
- **Releases** are cut by pushing a `v*` tag (e.g. `v0.2.0`). The `release.yml` workflow picks it up and produces cross-platform artifacts attached to a GitHub Release.
- **Hotfixes** branch from `main`, merge to `main`, then a fresh tag is pushed.

No `develop`, no `staging`. Pre-release work happens behind feature flags on `main` when it's risky.

## Commit messages

Use **[Conventional Commits](https://www.conventionalcommits.org/)**:

```
<type>(<optional scope>): <description>
```

Types: `feat`, `fix`, `refactor`, `chore`, `ci`, `docs`, `test`, `style`, `perf`.

- Description in imperative mood, lowercase, no trailing period.
- Keep subject line under 72 characters.
- Use the commit body for context when the subject isn't enough.
- Breaking changes get a `!` after the type/scope (e.g. `feat!: drop ipb6 parser`).

Examples:

```
feat(themes): add neon-arcade skin with retro CRT palette
fix(scrape): handle xenforo posts without .message-userExtras
chore(deps): bump @tauri-apps/cli to 2.1
docs(readme): clarify ANTHROPIC_API_KEY scope
```

## PR policy

- **One topic per PR.** Don't bundle a theme addition with a parser fix.
- **Link an issue** in the description (`Fixes #42` or `Refs #42`).
- **Test plan** is required. The PR template has the checklist.
- **CI must be green** before review. The CI workflow runs cargo tests, svelte-check, and vitest on every push to a PR.
- **Squash-merge** is the default. PR title becomes the commit message — make it a clean conventional-commit subject.
- **Conversation resolved** before merge. Don't merge with unresolved review comments.
- **No `--no-verify`** to skip hooks unless explicitly approved by a maintainer in the PR thread.

A maintainer will:
1. Confirm the PR addresses the linked issue.
2. Read the code change for correctness and style.
3. Verify the test plan covered the change.
4. Approve and merge, or request changes.

## Local development

See [DEVELOPMENT.md](DEVELOPMENT.md) for the full setup. The short version:

```sh
git clone https://github.com/312-dev/feedbulletin.git
cd feedbulletin
pnpm install
pnpm tauri dev          # hot-reload dev window
```

Before opening a PR:

```sh
# Type/lint check the Svelte frontend
pnpm check

# Run the Rust + frontend test suites
cd src-tauri && cargo test && cd ..
pnpm test
```

## Adding a new theme

1. Decide on a slug. Lowercase, kebab-case, descriptive. Look at `src/lib/theme/skins.ts` for the established style.
2. Add the slug to `SkinSlug`, the `SKINS` array, and pick a 2-color swatch in `src/lib/theme/skins.ts`.
3. Add a palette block to `src/lib/theme/skins.css` — both light and dark variants, every `--vb-*` variable. Use existing skins as templates.
4. Run `pnpm test:e2e` (with a dev server up) to generate a screenshot, save it under `e2e/screenshots/gallery-<slug>.png`, and link it in the README's gallery table.

## Adding a new site profile (bundled selectors)

If you have a host with stable selectors that the project should ship pre-learned:

1. Create `src-tauri/src/scrape/seed_profiles/<host>__<content_type>.md`. Match the format of the existing files: YAML frontmatter (`host`, `content_type`, `content_probe`, `schema_version`, `source: seed`) followed by a single ```json fenced block with the canonical `SelectorSet`.
2. Add the filename to `SEEDS` in `src-tauri/src/scrape/seed.rs`.
3. Run `cargo test --lib scrape::seed` — the `every_seed_file_parses` test will catch malformed JSON or missing frontmatter at the binary level.
4. Open a PR with screenshots showing native render working against a real thread on that host.

## Adding a new forum parser (`kind`)

Adding a new `kind` (e.g. an `nodebb` parser) is more involved. Open an issue first so we can confirm the scope. Generally:

1. Add the variant to `ForumKind` in `src-tauri/src/config.rs` (and `as_str`).
2. Add a parser in `src-tauri/src/forums/<kind>.rs`.
3. Wire it up in the poller dispatch.
4. Mirror the kind in `ALLOWED_KINDS` in `src/lib/feeds-schema.ts`.
5. Add a fixture under `src-tauri/tests/fixtures/` and a test under `src-tauri/tests/parsers.rs`.

## Code style

- **Rust**: `cargo fmt` before committing. No `clippy::pedantic`; just keep warnings to zero.
- **TypeScript / Svelte**: there's no autoformatter wired up yet — match the surrounding style. 2-space indent. Single quotes in TS, double in Svelte template strings.
- **Comments explain why**, not what. Don't narrate; give context a future reader can't get from `git blame`.

## Code of Conduct

Be reasonable. Differences in opinion are expected; personal attacks aren't. A
maintainer reserves the right to close issues / decline PRs / restrict
participation when behavior crosses the line. If you witness something
unacceptable, contact the maintainers at `ope@312.dev`.

## Maintainers

- [@GraysonCAdams](https://github.com/GraysonCAdams)

Thanks for reading this far. Looking forward to your PR.
