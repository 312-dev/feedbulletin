use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ForumKind {
    Reddit,
    Xenforo,
    Vbulletin,
    Discourse,
    Phpbb,
    Smf,
    Ipb,
    Ubiquiti,
    GenericRss,
}

impl ForumKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ForumKind::Reddit => "reddit",
            ForumKind::Xenforo => "xenforo",
            ForumKind::Vbulletin => "vbulletin",
            ForumKind::Discourse => "discourse",
            ForumKind::Phpbb => "phpbb",
            ForumKind::Smf => "smf",
            ForumKind::Ipb => "ipb",
            ForumKind::Ubiquiti => "ubiquiti",
            ForumKind::GenericRss => "generic_rss",
        }
    }
}

// WHY: field order here doubles as the YAML serialization order — serde_yaml
// writes struct fields in declaration order. We lead with `title` so wizard-
// authored entries are immediately readable in the editor; `kind` and the
// connection fields follow.
//
// `skip_serializing_if` keeps the YAML clean — without it, every None gets
// emitted as `null` and every default-false `disabled` gets emitted as
// `disabled: false`, drowning real fields in noise.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ForumConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    pub kind: ForumKind,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll_interval_s: Option<u64>,

    /// Optional per-forum skin override (slug from the frontend theme registry,
    /// e.g. "myspace-2006", "matrix-rain"). When set, viewing this forum or its
    /// threads applies the named skin in place of the site-wide pick. When None,
    /// the site-wide skin (stored in the frontend's localStorage) wins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,

    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl ForumConfig {
    pub fn resolved_title(&self) -> String {
        if let Some(t) = &self.title {
            return t.clone();
        }
        match self.kind {
            ForumKind::Reddit => {
                if let Some(s) = &self.sub {
                    // Capitalize first letter of the sub name — no "r/" prefix.
                    capitalize_first(s)
                } else if let Some(u) = &self.user {
                    capitalize_first(u)
                } else {
                    "Reddit".into()
                }
            }
            _ => self.url.clone().unwrap_or_else(|| "untitled".into()),
        }
    }

    pub fn resolved_source_url(&self) -> String {
        if let Some(u) = &self.url {
            return u.clone();
        }
        match self.kind {
            ForumKind::Reddit => {
                if let Some(s) = &self.sub {
                    format!("https://www.reddit.com/r/{}/.json?limit=50", s)
                } else if let Some(u) = &self.user {
                    format!("https://www.reddit.com/user/{}/.json?limit=50", u)
                } else {
                    String::new()
                }
            }
            _ => String::new(),
        }
    }
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CategoryConfig {
    pub name: String,
    pub forums: Vec<ForumConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    pub categories: Vec<CategoryConfig>,
}

impl AppConfig {
    pub fn from_str(text: &str) -> Result<Self> {
        let cfg: AppConfig = serde_yaml::from_str(text).context("failed to parse feeds.yaml")?;
        Ok(cfg)
    }

    pub fn from_path(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        Self::from_str(&text)
    }

    pub fn total_forum_count(&self) -> usize {
        self.categories.iter().map(|c| c.forums.len()).sum()
    }
}

pub fn locate_config(project_root: &Path) -> PathBuf {
    project_root.join("feeds.yaml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_seeded_shape() {
        let yaml = r#"
categories:
  - name: Cars
    forums:
      - kind: reddit
        sub: BMW
        title: BMW Discussion
      - kind: xenforo
        title: Styleforum
        url: https://www.styleforum.net/forums/-/index.rss
  - name: Tech
    forums:
      - kind: reddit
        sub: homeassistant
        title: Home Assistant Hub
"#;
        let cfg = AppConfig::from_str(yaml).unwrap();
        assert_eq!(cfg.categories.len(), 2);
        assert_eq!(cfg.total_forum_count(), 3);
        assert_eq!(cfg.categories[0].forums[0].resolved_title(), "BMW Discussion");
        // No explicit title: capitalized sub, no `r/`
        let no_title = ForumConfig {
            kind: ForumKind::Reddit,
            title: None,
            url: None,
            sub: Some("homeassistant".into()),
            user: None,
            description: None,
            poll_interval_s: None,
            theme: None,
            disabled: false,
        };
        assert_eq!(no_title.resolved_title(), "Homeassistant");
        assert!(
            cfg.categories[0].forums[0]
                .resolved_source_url()
                .starts_with("https://www.reddit.com/r/BMW/.json"),
            "got {}",
            cfg.categories[0].forums[0].resolved_source_url()
        );
    }

    #[test]
    fn reddit_user_feed_url_and_title() {
        let yaml = r#"
categories:
  - name: People
    forums:
      - kind: reddit
        user: doppio
        title: doppio
"#;
        let cfg = AppConfig::from_str(yaml).unwrap();
        let f = &cfg.categories[0].forums[0];
        assert_eq!(f.resolved_title(), "doppio");
        let url = f.resolved_source_url();
        assert!(
            url.starts_with("https://www.reddit.com/user/doppio/.json"),
            "got {}",
            url
        );
        // No title set: capitalized first letter
        let no_title = ForumConfig {
            kind: ForumKind::Reddit,
            title: None,
            url: None,
            sub: None,
            user: Some("agiantkenyan".into()),
            description: None,
            poll_interval_s: None,
            theme: None,
            disabled: false,
        };
        assert_eq!(no_title.resolved_title(), "Agiantkenyan");
        assert!(no_title
            .resolved_source_url()
            .starts_with("https://www.reddit.com/user/agiantkenyan/.json"));
    }
}
