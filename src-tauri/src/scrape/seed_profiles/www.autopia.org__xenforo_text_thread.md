---
host: www.autopia.org
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
  "author_selector": ".message-name a.username, .message-name span.username",
  "author_url_selector": ".message-name a.username",
  "avatar_selector": ".message-avatar-wrapper a.avatar img, .message-avatar-wrapper span.avatar img",
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
  "media_expand_selectors": [],
  "extra_media_selectors": [],
  "load_more_strategy": "next_page",
  "load_more_selector": null,
  "author_profile_url_template": "/members/{author}/",
  "author_avatar_format": "html",
  "author_avatar_locator": ".avatarWrapper img, .avatar img",
  "platform_guess": "xenforo",
  "notes": "Stock XenForo 2 layout. The avatar_selector explicitly ends in `img` for BOTH wrapper types (a.avatar AND span.avatar) — XF2 renders default letter avatars as <span class='avatar avatar--default'> wrapping a child <span data-text='X'> with NO <img>. A bare `.avatar` selector would match those wrapper spans, then select_one_attr(_, 'src') would return None and the silhouette fallback would render even for users with real uploaded avatars (if their first match in DOM order was a letter-default elsewhere on the page). Forcing `img` at the end means letter-default users correctly fall through to None → silhouette + geopattern, while users with real avatars get their <img src>."
}
```

## Background

`www.autopia.org` is a long-running car-detailing community running stock
XenForo 2. Same selector shape as `www.garagejournal.com` and `xdaforums.com`
— the avatar selector specifically uses the dual-wrapper pattern from XDA
(handles both `a.avatar` for uploaded-image users and `span.avatar` for
letter-default users) so old threads from the early 2010s, where most users
never uploaded an avatar, gracefully render through our silhouette +
geopattern fallback rather than producing broken-image glyphs.

## Why bundle it

Autopia (Detailing) is in the default-seed `BMW & Wrenching` category. The
Anthropic learner's one-shot output was missing the `img` suffix on the
avatar selector, which produced a silhouette for every user — including
those with real avatars — because the selector greedily matched the wrapper
element. Bundling the verified selector fixes this on first visit.
