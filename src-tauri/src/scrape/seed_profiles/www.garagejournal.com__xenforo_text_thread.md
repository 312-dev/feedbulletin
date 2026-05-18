---
host: www.garagejournal.com
content_type: xenforo_text_thread
content_probe: "article.message.message--post.js-post"
schema_version: 2
source: seed
---

```json
{
  "traversal_mode": "linear",
  "content_type": "xenforo_text_thread",
  "content_probe": "article.message.message--post.js-post",
  "post_selector": "article.message.message--post.js-post",
  "op_selector": null,
  "author_selector": ".message-name a.username",
  "author_url_selector": ".message-name a.username",
  "avatar_selector": ".message-avatar-wrapper a.avatar",
  "author_rank_selector": ".message-userTitle",
  "author_post_count_selector": ".message-userExtras dl.pairs--justified:nth-child(2) dd",
  "author_join_date_selector": ".message-userExtras dl.pairs--justified:nth-child(1) dd",
  "author_location_selector": ".message-userExtras dl.pairs--justified:nth-child(3) dd",
  "timestamp_selector": ".message-attribution-main time.u-dt",
  "body_selector": ".message-body .bbWrapper",
  "signature_selector": ".message-signature",
  "post_number_selector": ".message-attribution-opposite a[href*='/post-']",
  "quote_block_selector": ".bbCodeBlock--quote",
  "child_comment_selector": null,
  "child_comment_author_selector": null,
  "child_comment_body_selector": null,
  "pagination_next": "a.pageNav-jump--next",
  "load_more_strategy": "next_page",
  "load_more_selector": null,
  "platform_guess": "xenforo",
  "notes": "Stock XenForo 2 layout. Author sidebar metadata pulled from .message-userExtras dl pairs in DOM order; location may be absent for some users."
}
```

## Background

The Garage Journal runs a stock-themed XenForo 2 installation. Posts are
`article.message--post` elements with `.message-userExtras` as the sidebar
containing join date, message count, and (optionally) location as a
horizontal `<dl>` list in that order. All selectors are vanilla XF2 — no
theme customization to work around.

## Why bundle it

The Garage Journal is a popular automotive forum and many of its threads
are linked from sibling vBulletin/XenForo communities. Seeding the profile
lets the first thread open render natively right away.
