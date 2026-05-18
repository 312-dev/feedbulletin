use crate::db::ThreadInput;
use crate::forums::generic_rss;
use anyhow::Result;

pub fn parse(body: &str) -> Result<Vec<ThreadInput>> {
    generic_rss::parse_rss_like(body, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_slash_comments_and_dc_creator() {
        let body = r#"<?xml version="1.0"?>
<rss xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:slash="http://purl.org/rss/1.0/modules/slash/">
<channel>
<item>
  <title>Suit alterations</title>
  <link>https://www.styleforum.net/threads/suit.123/</link>
  <dc:creator>alice</dc:creator>
  <pubDate>Mon, 01 Jan 2024 12:00:00 GMT</pubDate>
  <slash:comments>42</slash:comments>
</item>
</channel></rss>"#;
        let r = parse(body).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title, "Suit alterations");
        assert_eq!(r[0].op_author.as_deref(), Some("alice"));
        assert_eq!(r[0].reply_count, Some(42));
    }
}
