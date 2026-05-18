use forum_reader_lib::AppConfig;
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures");
    p.push(name);
    std::fs::read_to_string(p).unwrap()
}

#[test]
fn loads_sample_feeds_yaml() {
    let text = fixture("feeds.sample.yaml");
    let cfg = AppConfig::from_str(&text).unwrap();
    assert_eq!(cfg.categories.len(), 3, "expected 3 categories");
    assert_eq!(cfg.total_forum_count(), 6, "expected 6 forums total");
    let category_names: Vec<&str> = cfg.categories.iter().map(|c| c.name.as_str()).collect();
    assert!(category_names.contains(&"Cars"));
    assert!(category_names.contains(&"Chicago"));
    assert!(category_names.contains(&"Tech"));

    // Reddit forum with explicit forum-admin-style title overrides the default `r/<sub>` form.
    let cars = cfg
        .categories
        .iter()
        .find(|c| c.name == "Cars")
        .expect("Cars category");
    let restoration = cars
        .forums
        .iter()
        .find(|f| f.sub.as_deref() == Some("centuryhomes"))
        .expect("centuryhomes forum");
    assert_eq!(restoration.resolved_title(), "Old House Restoration");
    assert!(!restoration.resolved_title().starts_with("r/"));

    // Reddit user-feed support.
    let tech = cfg
        .categories
        .iter()
        .find(|c| c.name == "Tech")
        .expect("Tech category");
    let user_feed = tech
        .forums
        .iter()
        .find(|f| f.user.as_deref() == Some("doppio"))
        .expect("doppio user feed");
    assert_eq!(user_feed.resolved_title(), "doppio");
    assert!(user_feed
        .resolved_source_url()
        .starts_with("https://www.reddit.com/user/doppio/.json"));
}
