# Security Policy

## Supported versions

feedBulletin is pre-1.0. Only the **latest released version** receives
security fixes. Older versions are not patched.

| Version  | Supported |
| -------- | --------- |
| 0.x      | :white_check_mark: latest minor only |
| < 0.1    | :x: |

## Reporting a vulnerability

**Do not file a public GitHub issue for a security report.**

Email `ope@312.dev` with:

- A description of the vulnerability and its impact.
- Steps to reproduce, ideally with a minimal feeds.yaml or a sample of the malicious input.
- Your assessment of severity (low / medium / high / critical) and any CVSS estimate you'd like to suggest.
- Whether you'd like to be credited publicly in the fix changelog.

We acknowledge reports within 72 hours and aim to ship a fix within 14 days
for medium/high severity issues. Critical issues get expedited handling.

## What's in scope

The interesting attack surface in a forum aggregator:

- **HTML injection through scraped post bodies** — we sanitize via `ammonia`, but bugs are possible. Any HTML escape from a learned site profile that lands rendered in the WebView is in scope.
- **YAML deserialization** — `feeds.yaml` is user-controlled but read locally; a malicious YAML file that causes the app to write outside its app-data directory, exfiltrate the API key, or escalate to RCE is in scope.
- **Anthropic API key exposure** — the key is read from `ANTHROPIC_API_KEY` at runtime. Any code path that logs it, ships it to a third party, or persists it to disk in plaintext is in scope.
- **Cookie cache leakage** — the cookie cache stores session tokens for forums that gate behind login. Cross-site reads, dumping the cache to logs, etc. are in scope.
- **Selector-learner prompt injection** — a forum that serves malicious HTML designed to manipulate the Anthropic selector-learner into producing harmful selectors (e.g. ones that exfiltrate page content) is in scope.

## What's out of scope

- The Anthropic API itself. Report API issues to Anthropic.
- The forum sites we scrape. We treat them as untrusted but we don't own their security.
- Denial-of-service from misconfigured polling (set a reasonable `poll_interval_s` and don't bury your machine).
- Issues that require physical access to an already-unlocked machine.

## Disclosure

We follow coordinated disclosure: we fix, ship, then publish details in the
release notes. Embargo extensions are negotiable for severe issues.

Reporters who prefer not to be named will not be named. Reporters who want
credit get a line in the release notes and the optional CVE assignment.

## Thanks

To everyone who takes the time to look at the code and report responsibly.
