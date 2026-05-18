---
host: community.ui.com
content_type: discourse_topic
content_probe: "#root"
schema_version: 2
source: seed
---

```json
{
  "traversal_mode": "linear",
  "content_type": "discourse_topic",
  "content_probe": "#root",
  "post_selector": "[data-testid=\"post\"], .post, .comment, [class*=\"post-\"], [class*=\"comment-\"]",
  "op_selector": null,
  "author_selector": "[class*=\"author\"], [class*=\"username\"], [class*=\"user-name\"]",
  "author_url_selector": null,
  "avatar_selector": null,
  "author_rank_selector": null,
  "author_post_count_selector": null,
  "author_join_date_selector": null,
  "author_location_selector": null,
  "timestamp_selector": null,
  "like_count_selector": null,
  "body_selector": "[class*=\"post-body\"], [class*=\"post-content\"], [class*=\"message-body\"]",
  "signature_selector": null,
  "post_number_selector": null,
  "quote_block_selector": null,
  "child_comment_selector": null,
  "child_comment_author_selector": null,
  "child_comment_body_selector": null,
  "pagination_next": null,
  "media_expand_selectors": [],
  "extra_media_selectors": [],
  "load_more_strategy": "click_button",
  "load_more_selector": null,
  "author_profile_url_template": null,
  "author_avatar_format": null,
  "author_avatar_locator": null,
  "author_post_count_locator": null,
  "author_karma_locator": null,
  "author_join_date_locator": null,
  "author_last_active_locator": null,
  "author_location_locator": null,
  "author_rank_locator": null,
  "platform_guess": "discourse",
  "notes": "Heuristic selectors for the Ubiquiti Community SPA. The page is client-rendered (React) so the static HTML exposes only #root and #modal; live scraping requires JS hydration before these selectors resolve. Treat as a best-effort baseline; the in-app debugger can refine once you've opened a real topic."
}
```

## Background

community.ui.com (the Ubiquiti Community Discourse-style forum) is fully
client-rendered. The initial HTML payload is essentially `<div id="root">`,
which means static-DOM selector derivation finds nothing to match.

These selectors are heuristic best-guesses keyed on common Discourse/SPA
class-name patterns (`post-*`, `comment-*`, `username`, etc.). After the
WKWebView hydrates the page, real elements appear and these selectors
have a decent shot at matching. If you hit misses on a specific topic,
open the debugger from the thread toolbar and the agent will refine
against the hydrated DOM.

## Why bundle it

Without the seed, every fresh install would burn an Anthropic call to
derive these (and produce the same heuristic answer, since the static HTML
hasn't changed). Bundling avoids the cost and gives the user a working
baseline on first launch.
