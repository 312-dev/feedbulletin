---
host: www.bimmerpost.com
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
  "author_rank_selector": "td.alt2 div.usertitle, td.alt2 div.rank",
  "author_post_count_selector": null,
  "author_join_date_selector": null,
  "author_location_selector": null,
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
  "notes": "BimmerPost main forum — vBulletin 4. Same Bimmer Network theme as g80.bimmerpost.com. See that file for the rationale behind keeping Posts/Joined/Location null in the seed."
}
```

## Why bundle it

`www.bimmerpost.com` is the umbrella site of the Bimmer Network and a
default-seed forum. Identical to `g80.bimmerpost.com` selectorwise.
