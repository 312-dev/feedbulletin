---
host: www.e90post.com
content_type: vbulletin_thread
content_probe: "div#posts, table[id^='post']"
schema_version: 2
source: seed
---

```json
{
  "traversal_mode": "linear",
  "content_type": "vbulletin_thread",
  "content_probe": "div#posts, table[id^='post']",
  "post_selector": "table[id^='post']",
  "op_selector": null,
  "author_selector": "td.alt2 a.bigusername, td.alt2 div.username_container a, td.alt2 strong.username, td.alt2 span.username",
  "author_url_selector": "td.alt2 a.bigusername, td.alt2 div.username_container a[href*='member.php']",
  "avatar_selector": "td.alt2 img.avatar",
  "author_rank_selector": "td.alt2 div.usertitle",
  "author_post_count_selector": "td.alt2 div.postdetails",
  "author_join_date_selector": "td.alt2 div.joindate, td.alt2 div.postdetails ~ div",
  "author_location_selector": "td.alt2 div.location",
  "timestamp_selector": "td.thead div.normal",
  "body_selector": "div[id^='post_message_']",
  "signature_selector": "div[id^='post_message_'] ~ div.signature",
  "post_number_selector": "td.thead div.normal a[href*='#post']",
  "quote_block_selector": "div[id^='post_message_'] div.quote",
  "child_comment_selector": null,
  "child_comment_author_selector": null,
  "child_comment_body_selector": null,
  "pagination_next": "a[rel='next'], a[href*='showthread.php'][href*='page=']",
  "load_more_strategy": "next_page",
  "load_more_selector": null,
  "platform_guess": "vbulletin",
  "notes": "Classic vBulletin 3.x thread layout on e90post. Posts are top-level tables[id^=post]; author column is the leftmost td.alt2 cell."
}
```

## Background

e90post.com is a long-running BMW E90/E91/E92/E93 enthusiast forum running
vBulletin 3.x. Each post is rendered as a `<table id="postNNNNN">` with a
two-cell layout: author column on the left (`td.alt2`) and post body on the
right (`td.alt1`). Avatars, usertitle/rank, post count, join date, and
location all live within the author cell.

The selectors were derived once via the in-app Anthropic learner and then
refined via the critique pass (post-render validation that ran against a
representative thread). They have been stable across the dozens of E90/E92
threads tested.

## Why bundle it

E90Post is a popular destination for the BMW & Wrenching category in the
default seed feeds; bundling means new users open their first M3 thread
and get the full native render — avatars, sidebar metadata, the lot —
without spending an Anthropic call.
