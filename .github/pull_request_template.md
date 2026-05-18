<!--
Thanks for opening a PR. Please fill out each section.

PR title format: <type>(<optional scope>): <description>
  e.g. feat(themes): add neon-arcade skin with retro CRT palette
  Types: feat, fix, refactor, chore, ci, docs, test, style, perf
-->

## Summary

<!-- 1-3 bullets: what changed and why. Link the issue this resolves. -->

- 
- 

Resolves #

## Test plan

<!-- Concrete steps a reviewer can run to verify the change. Check what you did. -->

- [ ] `cargo test` passes (`cd src-tauri && cargo test`)
- [ ] `pnpm check` is clean
- [ ] `pnpm test` passes
- [ ] Manually exercised the change in `pnpm tauri dev`:
  - [ ] <step 1>
  - [ ] <step 2>
- [ ] Screenshots updated under `e2e/screenshots/` (if UI changed)
- [ ] Docs updated (README / CONTRIBUTING / DEVELOPMENT, if relevant)

## Risk / blast radius

<!-- What could go wrong? Anything reviewers should pay extra attention to?
     "Touches the poller", "Changes the DB schema", "Affects existing user
     feeds.yaml files", etc. -->

## Screenshots / video

<!-- Drop them inline if there's a visual change. Omit if not relevant. -->
