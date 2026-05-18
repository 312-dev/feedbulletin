use forum_reader_lib::test_exports::{generic_rss_parse, reddit_parse, xenforo_parse};
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures");
    p.push(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read fixture {}: {}", p.display(), e))
}

#[test]
fn reddit_centuryhomes_fixture_parses() {
    let body = fixture("reddit-centuryhomes.json");
    let threads = reddit_parse(&body).unwrap();
    assert!(threads.len() >= 3, "got {} threads", threads.len());
    let first = &threads[0];
    assert!(first.title.contains("1923 Foursquare"));
    assert_eq!(first.op_author.as_deref(), Some("oldhouselove"));
    assert_eq!(first.reply_count, Some(47));
    let deleted = threads
        .iter()
        .find(|t| t.title.starts_with("Plaster"))
        .unwrap();
    assert!(deleted.op_author.is_none(), "[deleted] should be filtered out");
}

#[test]
fn styleforum_xenforo_fixture_extracts_slash_comments() {
    let body = fixture("styleforum.rss");
    let threads = xenforo_parse(&body).unwrap();
    assert_eq!(threads.len(), 3);
    let waywt = threads
        .iter()
        .find(|t| t.title.starts_with("WAYWT"))
        .unwrap();
    assert_eq!(waywt.reply_count, Some(412));
    assert_eq!(waywt.op_author.as_deref(), Some("menswearmaven"));
}

#[test]
fn lthforum_generic_rss_fixture_parses() {
    let body = fixture("lthforum.rss");
    let threads = generic_rss_parse(&body).unwrap();
    assert_eq!(threads.len(), 2);
    assert!(threads[0].title.contains("ramen"));
    assert!(
        threads[0].reply_count.is_none(),
        "generic_rss should not invent reply counts"
    );
}
