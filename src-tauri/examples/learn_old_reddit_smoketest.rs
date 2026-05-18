// In-app WebView smoke test for the site-profile learner.
//
// Spawns a real WKWebView (the same backend Tauri uses), navigates to a known
// stable old.reddit thread, captures the rendered DOM via `with_ipc_handler`,
// and feeds it through the production scrape pipeline (`scrape::handle_sample`
// → anthropic::learn → extract::apply). Asserts that ≥2 posts were extracted
// with author + body populated, and that at least one parent post got a
// synthesized `vb-reply` blockquote from the depth-1 child traversal.
//
// Run:
//   cd src-tauri && cargo run --example learn_old_reddit_smoketest
//
// With a custom URL:
//   cargo run --example learn_old_reddit_smoketest -- https://old.reddit.com/r/xxx/comments/yyy/
//
// Requires `ANTHROPIC_API_KEY` in env — without it the test exits SKIP(0).
//
// This is the canonical "did our extraction and logic survive contact with
// real-world HTML?" check. Run it after touching scrape/, anthropic/, or the
// SCRAPE_RETURN_JS wiring in commands.rs.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

use forum_reader_lib::db;
use forum_reader_lib::scrape;

const DEFAULT_URL: &str =
    "https://old.reddit.com/r/gtaonline/comments/1tdwgwk/having_some_fun_on_the_hakuchou_in_first_person/";

const SETTLE_DELAY_MS: u64 = 2500;
const TIMEOUT_SECS: u64 = 180; // generous — Anthropic call can take 30s+

/// Inject after page load: wait SETTLE_DELAY_MS for any post-load JS to settle,
/// then ship the rendered HTML over IPC. Mirrors the production SCRAPE_RETURN_JS
/// shape so the assertion path is identical.
const SCRAPE_POST_JS: &str = r#"
(function() {
  function emit() {
    try {
      var html = document.documentElement && document.documentElement.outerHTML
        ? document.documentElement.outerHTML
        : "";
      if (html.length > 1200000) html = html.slice(0, 1200000);
      var payload = JSON.stringify({
        kind: "scrape_sample",
        host: location.host,
        url: location.href,
        html: html
      });
      window.ipc.postMessage(payload);
    } catch (e) {
      window.ipc.postMessage(JSON.stringify({kind: "scrape_error", error: String(e)}));
    }
  }
  if (document.readyState === "complete") {
    setTimeout(emit, SETTLE_DELAY_MS);
  } else {
    window.addEventListener("load", function() { setTimeout(emit, SETTLE_DELAY_MS); }, { once: true });
  }
})();
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,sqlx=warn")),
        )
        .try_init();

    if std::env::var("ANTHROPIC_API_KEY").is_err() {
        println!("SKIP: ANTHROPIC_API_KEY not set; can't exercise the learner end-to-end.");
        return Ok(());
    }

    let target_url = std::env::args().nth(1).unwrap_or_else(|| DEFAULT_URL.to_string());
    println!("== smoke test target: {target_url}");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()?;

    let pool = rt.block_on(async { db::open_pool("sqlite::memory:").await })?;
    let pool = std::sync::Arc::new(pool);

    let done = Arc::new(AtomicBool::new(false));
    let success = Arc::new(AtomicBool::new(false));
    // Don't process a second scrape sample after we've already started one —
    // the WebView fires the init script on every navigation, including silent
    // redirects (e.g. login banner) and we only want the canonical thread page.
    let in_flight = Arc::new(AtomicBool::new(false));

    let event_loop = EventLoopBuilder::<()>::new().build();
    let window = WindowBuilder::new()
        .with_title("Forum Reader — old.reddit smoke test")
        .with_inner_size(tao::dpi::LogicalSize::new(1280.0, 900.0))
        .build(&event_loop)?;

    let rt_handle = rt.handle().clone();
    let done_for_ipc = done.clone();
    let success_for_ipc = success.clone();
    let pool_for_ipc = pool.clone();
    let in_flight_for_ipc = in_flight.clone();
    let scrape_post_js = SCRAPE_POST_JS.replace("SETTLE_DELAY_MS", &SETTLE_DELAY_MS.to_string());

    let _webview = WebViewBuilder::new()
        .with_url(&target_url)
        .with_user_agent(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
        )
        .with_initialization_script(&scrape_post_js)
        .with_ipc_handler(move |req| {
            let body: String = req.body().clone();

            // Reject samples that don't look like the canonical thread page —
            // old.reddit redirects to interstitials, /r/X/ index pages, etc.
            // We want a /comments/<id>/<slug>/ URL with a substantial body.
            let looks_canonical = preview_url(&body)
                .map(|u| u.contains("/comments/") && u.contains(&host_for_filter(&u).unwrap_or_default()))
                .unwrap_or(false);
            let big_enough = body.len() > 50_000;
            if !looks_canonical || !big_enough {
                eprintln!(
                    "== ignoring intermediate scrape sample (url={:?}, body_len={})",
                    preview_url(&body).unwrap_or_default(),
                    body.len()
                );
                return;
            }
            if in_flight_for_ipc.swap(true, Ordering::SeqCst) {
                return; // already processing a sample
            }

            let rt = rt_handle.clone();
            let done = done_for_ipc.clone();
            let success = success_for_ipc.clone();
            let pool = pool_for_ipc.clone();
            std::thread::spawn(move || {
                let result = rt.block_on(async move {
                    process_sample(&pool, &body).await
                });
                match result {
                    Ok(true) => success.store(true, Ordering::SeqCst),
                    Ok(false) => {
                        eprintln!("FAIL: pipeline ran but extraction did not pass validation");
                    }
                    Err(e) => {
                        eprintln!("FAIL: pipeline error: {e}");
                    }
                }
                done.store(true, Ordering::SeqCst);
            });
        })
        .build(&window)?;

    let started = Instant::now();
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(150));

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }

        if done.load(Ordering::SeqCst) {
            if success.load(Ordering::SeqCst) {
                println!("PASS");
            } else {
                println!("FAIL");
            }
            *control_flow = ControlFlow::Exit;
        }

        if started.elapsed() > Duration::from_secs(TIMEOUT_SECS) {
            eprintln!("FAIL: timed out after {TIMEOUT_SECS}s waiting for page load + scrape");
            *control_flow = ControlFlow::Exit;
        }
    });
}

async fn process_sample(pool: &sqlx::SqlitePool, raw: &str) -> anyhow::Result<bool> {
    let sample = match scrape::decode_message(raw) {
        Some(s) => s,
        None => {
            anyhow::bail!("scrape callback did not return a scrape_sample message: {}", raw_preview(raw));
        }
    };
    println!(
        "== received DOM from webview: host={} url={} html_len={}",
        sample.host,
        sample.url,
        sample.html.len()
    );

    let payload = match scrape::handle_sample(
        pool,
        &sample.host,
        &sample.url,
        &sample.html,
        None,
        Some("reddit"),
    )
    .await?
    {
        Some(p) => p,
        None => {
            eprintln!("FAIL: handle_sample returned None (learner could not produce a working profile)");
            // Dump the selectors Anthropic returned so we can see what was wrong.
            if let Ok(rows) = db::list_site_profiles_for_host(pool, &sample.host).await {
                for row in rows {
                    eprintln!(
                        "== profile ({}, {}) probe={:?}\nselectors_json:\n{}",
                        row.host, row.content_type, row.content_probe, row.selectors_json
                    );
                }
            }
            return Ok(false);
        }
    };

    println!(
        "== handle_sample returned source={} post_count={}",
        payload.source,
        payload.posts.len()
    );
    for (i, post) in payload.posts.iter().take(3).enumerate() {
        println!(
            "   [{}] post_number={:?} author={:?} body_html_len={}",
            i,
            post.post_number,
            post.author,
            post.body_html.len()
        );
    }

    // Assertions
    if payload.posts.len() < 2 {
        eprintln!("FAIL: expected >=2 posts (OP + at least 1 comment), got {}", payload.posts.len());
        return Ok(false);
    }
    let with_author = payload.posts.iter().filter(|p| !p.author.is_empty()).count();
    if with_author * 5 < payload.posts.len() * 4 {
        eprintln!(
            "FAIL: only {}/{} posts had an author — selector seems off",
            with_author,
            payload.posts.len()
        );
        return Ok(false);
    }
    // Verify the depth-1 reply restructure: at least one post should be
    // a standalone reply that QUOTES an earlier post — i.e. quote_of_post_number
    // is set AND its body has a vb-quote blockquote prepended.
    let any_reply = payload.posts.iter().any(|p| {
        p.quote_of_post_number.is_some() && p.body_html.contains(r#"class="vb-quote""#)
    });
    if !any_reply {
        eprintln!(
            "FAIL: no standalone reply with vb-quote blockquote found — depth-1 child traversal failed (selectors likely missing child_comment_selector)"
        );
        return Ok(false);
    }
    let profiles = db::list_site_profiles_for_host(pool, &sample.host).await?;
    if profiles.is_empty() {
        eprintln!("FAIL: no site_profile rows were persisted");
        return Ok(false);
    }
    println!(
        "== persisted {} profile(s) for host: {:?}",
        profiles.len(),
        profiles.iter().map(|p| p.content_type.clone()).collect::<Vec<_>>()
    );

    println!("OK: extraction validated against real rendered DOM.");
    Ok(true)
}

fn raw_preview(s: &str) -> String {
    let mut out: String = s.chars().take(200).collect();
    if s.len() > 200 {
        out.push('…');
    }
    out
}

/// Cheap regex-free peek at the URL inside a scrape_sample JSON envelope so we
/// can filter intermediate samples without parsing the whole payload.
fn preview_url(body: &str) -> Option<String> {
    let key = "\"url\":\"";
    let start = body.find(key)? + key.len();
    let rest = &body[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn host_for_filter(url: &str) -> Option<String> {
    let after_scheme = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    after_scheme
        .split('/')
        .next()
        .filter(|h| !h.is_empty())
        .map(|h| h.to_string())
}
