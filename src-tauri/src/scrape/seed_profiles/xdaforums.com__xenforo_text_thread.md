---
host: xdaforums.com
content_type: xenforo_text_thread
content_probe: "article.message--post.js-post"
schema_version: 2
source: seed
---

```json
{
  "traversal_mode": "linear",
  "content_type": "xenforo_text_thread",
  "content_probe": "article.message--post.js-post",
  "post_selector": "article.message--post.js-post",
  "op_selector": null,
  "author_selector": "h4.message-name a.username",
  "author_url_selector": "h4.message-name a.username",
  "avatar_selector": ".message-avatar-wrapper a.avatar img",
  "author_rank_selector": "h5.userTitle.message-userTitle",
  "author_post_count_selector": ".message-userExtras dl:nth-child(2) dd",
  "author_join_date_selector": ".message-userExtras dl:nth-child(1) dd",
  "author_location_selector": null,
  "timestamp_selector": "header.message-attribution time.u-dt",
  "op_body_selector": null,
  "like_count_selector": ".reactionsBar.js-reactionsList",
  "body_selector": "article.message-body .bbWrapper",
  "signature_selector": ".message-signature",
  "post_number_selector": ".message-attribution-opposite a[href*=\"/post-\"]:last-child",
  "quote_block_selector": ".bbCodeBlock--quote",
  "child_comment_selector": null,
  "child_comment_author_selector": null,
  "child_comment_body_selector": null,
  "pagination_next": "a[rel=\"next\"]",
  "media_expand_selectors": [],
  "extra_media_selectors": [],
  "load_more_strategy": "next_page",
  "load_more_selector": null,
  "author_profile_url_template": "/m/{author}/",
  "author_avatar_format": "html",
  "author_avatar_locator": ".avatarWrapper img, .avatar img",
  "author_post_count_locator": null,
  "author_karma_locator": null,
  "author_join_date_locator": null,
  "author_last_active_locator": null,
  "author_location_locator": null,
  "author_rank_locator": null,
  "platform_guess": "xenforo",
  "notes": "XDA Forums uses XenForo with the Audentio/UIX theme. Avatars may be span.avatar--default when the user has never uploaded an image. Post-count dl is 2nd (fa-comments icon); join-date dl is 1st (fa-user icon)."
}
```

## Background

XDA Forums runs XenForo 2 with the Audentio/UIX theme, which subtly tweaks
the standard markup (notably the user title moves from a class on the
sidebar header to its own `h5.userTitle`, and avatars wrap a default
`<span>` for users without uploaded images). Selectors here account for
both real `<img>` avatars and the letter-default fallback.

## Why bundle it

XDA is the canonical destination for Android ROM and modding discussion;
the default seed feeds include it directly. Bundled selectors mean the
first XDA thread opens with a clean render — avatars, rank, post count,
the full XF2 sidebar.
