import { test } from "@playwright/test";

const VITE_URL = process.env.VITE_DEV_URL ?? "http://localhost:1420";

// A representative fixture matching the v1 feeds.yaml layout (13 categories,
// ~80 forums). The screenshot spec exercises a stubbed Tauri runtime so the
// captured PNGs are independent of live network state.
const stub = `
  const now = Math.floor(Date.now() / 1000);
  function fkid(id, cat, kind, title, opts = {}) {
    return Object.assign({
      id, category_id: cat, kind, title,
      source_url: "x", description: null, poll_interval_s: 1800,
      thread_count: 50, post_count: 800,
      last_polled_at: now - 30, last_error: null,
      last_visited_at: opts.unread ? null : now - 30,
      latest_thread_title: "Sample headline " + id,
      latest_thread_author: "user" + id,
      latest_thread_at: now - 600,
      latest_thread_url: "https://example.com/" + id,
      unread: !!opts.unread,
    }, opts);
  }
  const cats = [
    { id: 1, name: "BMW & Wrenching", forums: [
      fkid(101,1,"generic_rss","Bimmerforums"),
      fkid(102,1,"generic_rss","BimmerPost"),
      fkid(103,1,"generic_rss","Garage Journal"),
      fkid(104,1,"generic_rss","Autopia (Detailing)"),
    ]},
    { id: 2, name: "Chicago & Suburbs", forums: [
      fkid(201,2,"generic_rss","LTH Forum", { last_error: "http 403 fetching feed", latest_thread_title: null, latest_thread_author: null, latest_thread_at: null, latest_thread_url: null, thread_count: 0, post_count: 0 }),
      fkid(202,2,"reddit","The Chicago Lounge", { unread: true }),
      fkid(203,2,"reddit","Chicago Eats"),
      fkid(204,2,"reddit","Suburbs of Chicago"),
      fkid(205,2,"reddit","Near West Suburbs"),
      fkid(206,2,"reddit","Chicago YIMBYs"),
      fkid(207,2,"reddit","Oak Park"),
      fkid(208,2,"reddit","Ask Chicago"),
      fkid(209,2,"reddit","Atlanta"),
    ]},
    { id: 3, name: "Smart Home & Network", forums: [
      fkid(301,3,"xenforo","Home Assistant Community"),
      fkid(302,3,"generic_rss","Ubiquiti Community — UniFi"),
      fkid(303,3,"generic_rss","Ubiquiti Community — Network"),
      fkid(304,3,"reddit","HomeKit Lounge"),
      fkid(305,3,"reddit","Sonos Owners"),
    ]},
    { id: 4, name: "Tech & Phones", forums: [
      fkid(401,4,"generic_rss","XDA Developers"),
      fkid(402,4,"generic_rss","Fairphone Community"),
      fkid(403,4,"reddit","Android General"),
      fkid(404,4,"reddit","Android Auto"),
      fkid(405,4,"reddit","Pixel Phones"),
      fkid(406,4,"reddit","GrapheneOS Discussion"),
      fkid(407,4,"reddit","Phone Buying Advice"),
      fkid(408,4,"reddit","Unihertz Owners"),
      fkid(409,4,"reddit","Windows 11"),
      fkid(410,4,"reddit","Windsurf (IDE)"),
      fkid(411,4,"reddit","Vintage Computing"),
      fkid(412,4,"reddit","Kubernetes"),
    ]},
    { id: 5, name: "Coffee & Espresso", forums: [
      fkid(501,5,"generic_rss","Home-Barista"),
      fkid(502,5,"reddit","Coffee Drinkers"),
      fkid(503,5,"reddit","Espresso Geeks"),
    ]},
    { id: 6, name: "Money, Cards & Travel", forums: [
      fkid(601,6,"generic_rss","FlyerTalk"),
      fkid(602,6,"generic_rss","Bogleheads"),
      fkid(603,6,"generic_rss","MMM Forum"),
      fkid(604,6,"reddit","American Express"),
      fkid(605,6,"reddit","Churning"),
      fkid(606,6,"reddit","Credit Cards"),
      fkid(607,6,"reddit","Delta Air Lines"),
      fkid(608,6,"reddit","United Airlines"),
      fkid(609,6,"reddit","Travel General"),
      fkid(610,6,"reddit","Budgeting"),
      fkid(611,6,"reddit","Frugal Living"),
      fkid(612,6,"reddit","Financial Independence"),
      fkid(613,6,"reddit","Liquid Budget"),
      fkid(614,6,"reddit","Money Diaries"),
      fkid(615,6,"reddit","Origin Financial"),
      fkid(616,6,"reddit","YNAB Official"),
    ]},
    { id: 7, name: "Music & Audio", forums: [
      fkid(701,7,"generic_rss","Head-Fi (DAPs)"),
      fkid(702,7,"generic_rss","The Gear Page (Guitar)"),
      fkid(703,7,"reddit","Music General"),
      fkid(704,7,"reddit","R&B"),
      fkid(705,7,"reddit","Breaking Benjamin"),
      fkid(706,7,"reddit","Guitar Players"),
      fkid(707,7,"reddit","Digital Audio Players"),
      fkid(708,7,"reddit","Snipd Users"),
      fkid(709,7,"reddit","Spotify"),
    ]},
    { id: 8, name: "Gaming", forums: [
      fkid(801,8,"generic_rss","ResetEra"),
      fkid(802,8,"reddit","Gaming General"),
      fkid(803,8,"reddit","GTA Online"),
      fkid(804,8,"reddit","Halo"),
      fkid(805,8,"reddit","Abiotic Factor"),
    ]},
    { id: 9, name: "Home & Lifestyle", forums: [
      fkid(901,9,"reddit","Century Homes"),
      fkid(902,9,"reddit","Menswear"),
      fkid(903,9,"reddit","Design Gallery"),
      fkid(904,9,"reddit","Map Gallery"),
      fkid(905,9,"reddit","Costco"),
      fkid(906,9,"reddit","Tattoo"),
      fkid(907,9,"reddit","Aged Tattoos"),
      fkid(908,9,"reddit","ADHD"),
      fkid(909,9,"reddit","Getting Things Done"),
      fkid(910,9,"reddit","Weight Loss"),
      fkid(911,9,"reddit","Lactose Intolerant"),
      fkid(912,9,"reddit","Zero Waste"),
      fkid(913,9,"reddit","Green Living"),
      fkid(914,9,"reddit","Eclosion (Macro)"),
    ]},
    { id: 10, name: "Expat & Ireland", forums: [
      fkid(1001,10,"reddit","Irish Citizenship"),
      fkid(1002,10,"reddit","Move to Ireland"),
      fkid(1003,10,"reddit","Expats"),
    ]},
    { id: 11, name: "News & General Chat", forums: [
      fkid(1101,11,"reddit","US News"),
      fkid(1102,11,"reddit","World News"),
      fkid(1103,11,"reddit","Ask Reddit"),
      fkid(1104,11,"reddit","No Stupid Questions"),
      fkid(1105,11,"reddit","Today I Learned"),
      fkid(1106,11,"reddit","Pictures"),
      fkid(1107,11,"reddit","Celeb Gossip"),
      fkid(1108,11,"reddit","My Brother, My Brother and Me"),
      fkid(1109,11,"reddit","Rooster Teeth"),
      fkid(1110,11,"reddit","Space"),
      fkid(1111,11,"reddit","Relay for Reddit (legacy)"),
    ]},
    { id: 12, name: "Cute & Cats", forums: [
      fkid(1201,12,"reddit","Kitty Has a Question"),
      fkid(1202,12,"reddit","Cross-Eyed Cats"),
      fkid(1203,12,"reddit","Airplane Ears"),
    ]},
    { id: 13, name: "People I Follow", forums: [
      fkid(1301,13,"reddit","A Giant Kenyan"),
      fkid(1302,13,"reddit","doppio"),
      fkid(1303,13,"reddit","exe_CUTOR"),
      fkid(1304,13,"reddit","Indian King Cobra"),
      fkid(1305,13,"reddit","Miserable Bluejay"),
    ]},
  ].map((c, i) => ({ ...c, sort_order: i }));

  window.__TAURI_INTERNALS__ = {
    invoke: async (cmd, args) => {
      if (cmd === "get_categories") return cats;
      if (cmd === "get_forum") {
        for (const c of cats) for (const f of c.forums) if (f.id === args.forumId) return f;
        return null;
      }
      if (cmd === "get_forum_threads") return [];
      if (cmd === "get_thread") return null;
      if (cmd === "mark_forum_visited") return null;
      if (cmd === "mark_all_forums_visited") return null;
      if (cmd === "open_thread_webview") return null;
      if (cmd === "close_thread_webview") return null;
      return [];
    },
  };
`;

test("capture v1 index at 1200", async ({ page }) => {
  await page.addInitScript(stub);
  await page.setViewportSize({ width: 1200, height: 900 });
  await page.goto(VITE_URL);
  await page.waitForSelector("[data-testid='forum-row']", { timeout: 5000 });
  await page.screenshot({ path: "e2e/screenshots/v1-index-1200.png", fullPage: true });
});

test("capture v1 index at 480", async ({ page }) => {
  await page.addInitScript(stub);
  await page.setViewportSize({ width: 480, height: 900 });
  await page.goto(VITE_URL);
  await page.waitForSelector("[data-testid='forum-row']", { timeout: 5000 });
  await page.screenshot({ path: "e2e/screenshots/v1-index-480.png", fullPage: true });
});

// ─── Theme gallery ──────────────────────────────────────────────────────────
// One screenshot per highlighted skin so the README can showcase the variety.
// The stub seeds the index with a dense category so palette + typography
// differences are visible at a glance.

const GALLERY_SKINS: { slug: string; mode: "light" | "dark"; label: string }[] = [
  { slug: "vb-classic",        mode: "light", label: "vb-classic-light" },
  { slug: "vb-classic",        mode: "dark",  label: "vb-classic-dark" },
  { slug: "myspace-2006",      mode: "light", label: "myspace-2006" },
  { slug: "matrix-rain",       mode: "dark",  label: "matrix-rain" },
  { slug: "bubblegum",         mode: "light", label: "bubblegum" },
  { slug: "letterpress-paper", mode: "light", label: "letterpress-paper" },
  { slug: "geocities-90s",     mode: "light", label: "geocities-90s" },
  { slug: "bbs-amber",         mode: "dark",  label: "bbs-amber" },
];

for (const skin of GALLERY_SKINS) {
  test(`gallery: ${skin.label}`, async ({ page }) => {
    await page.addInitScript(stub);
    // Pre-seat the theme so it lands before the layout's $effect fires and
    // tries to read localStorage / OS-pref.
    await page.addInitScript(
      ({ slug, mode }) => {
        try {
          localStorage.setItem("fr_skin", slug);
          localStorage.setItem("fr_theme", mode);
        } catch {}
      },
      { slug: skin.slug, mode: skin.mode },
    );
    await page.setViewportSize({ width: 1200, height: 900 });
    await page.goto(VITE_URL);
    await page.waitForSelector("[data-testid='forum-row']", { timeout: 5000 });
    await page.screenshot({
      path: `e2e/screenshots/gallery-${skin.label}.png`,
      fullPage: false,
    });
  });
}

// ─── Native thread view: Reddit-style with avatars + author column metadata
// ─────────────────────────────────────────────────────────────────────────
//
// Demonstrates the enrichment pipeline: each post carries avatar_url, rank,
// post count, join date, and location. The renderer flows them into the
// classic vB4 author column on the left of every post.
test("native-thread: reddit-style with avatars + author column metadata", async ({ page }) => {
  const ts = Math.floor(Date.now() / 1000);
  // Stub data: 5 posts in a Reddit-style discussion, all with full enrichment.
  // Inline SVG avatars per author as data: URIs so the screenshot is
  // self-contained — no network dep, no broken-image icons in CI, while
  // still demonstrating real avatar rendering (vs the geopattern fallback).
  const AVATAR_CENTURY = "data:image/svg+xml;utf8," + encodeURIComponent(
    `<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'>
       <defs><linearGradient id='g' x1='0' x2='0' y1='0' y2='1'>
         <stop offset='0' stop-color='#7CB342'/><stop offset='1' stop-color='#33691E'/>
       </linearGradient></defs>
       <rect width='64' height='64' fill='url(#g)'/>
       <path d='M14 50 L32 18 L50 50 Z' fill='#FFF8E1' opacity='0.9'/>
       <rect x='29' y='38' width='6' height='12' fill='#5D4037'/>
     </svg>`
  );
  const AVATAR_BOILER = "data:image/svg+xml;utf8," + encodeURIComponent(
    `<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'>
       <rect width='64' height='64' fill='#1565C0'/>
       <circle cx='32' cy='24' r='10' fill='#FFCCBC'/>
       <path d='M14 56 C14 42 50 42 50 56 Z' fill='#FFCCBC'/>
       <rect x='10' y='14' width='44' height='6' fill='#FFA000'/>
       <text x='32' y='52' text-anchor='middle' font-family='monospace' font-size='8' font-weight='bold' fill='#FFFFFF'>HVAC</text>
     </svg>`
  );
  const AVATAR_RESTORER = "data:image/svg+xml;utf8," + encodeURIComponent(
    `<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'>
       <rect width='64' height='64' fill='#6D4C41'/>
       <rect x='12' y='28' width='40' height='28' fill='#A1887F'/>
       <polygon points='8,28 32,8 56,28' fill='#3E2723'/>
       <rect x='28' y='38' width='8' height='18' fill='#3E2723'/>
       <rect x='16' y='34' width='6' height='8' fill='#FFECB3'/>
       <rect x='42' y='34' width='6' height='8' fill='#FFECB3'/>
     </svg>`
  );

  const richPostsStub = stub + `
    const oldInvoke = window.__TAURI_INTERNALS__.invoke;
    const AV_CENTURY = ${JSON.stringify(AVATAR_CENTURY)};
    const AV_BOILER = ${JSON.stringify(AVATAR_BOILER)};
    const AV_RESTORER = ${JSON.stringify(AVATAR_RESTORER)};
    const POSTS = [
      {
        post_number: 1,
        author: "centuryhomesfan",
        author_url: "https://www.reddit.com/u/centuryhomesfan",
        avatar_url: AV_CENTURY,
        author_rank: "Top 1% contributor",
        author_post_count: "2,148 posts",
        author_join_date: "Joined Jan 2019",
        author_karma: "184,520 karma",
        timestamp: "2 hours ago",
        body_html: "<p>Picked up a 1923 Foursquare in Oak Park last week. Hardwood is in surprisingly good shape under the carpet but the steam radiators in the upstairs bedrooms aren't kicking on. Anyone troubleshot a stuck Hoffman valve before? Pictures attached.</p>",
        like_count: 142,
      },
      {
        post_number: 2,
        author: "boilerdad",
        author_url: "https://www.reddit.com/u/boilerdad",
        avatar_url: AV_BOILER,
        author_rank: "HVAC contractor",
        author_post_count: "9,712 posts",
        author_join_date: "Joined May 2014",
        author_karma: "612,033 karma",
        author_location: "Chicago, IL",
        timestamp: "1 hour ago",
        body_html: "<blockquote>the steam radiators in the upstairs bedrooms aren't kicking on</blockquote><p>Three things to check before you replace anything: (1) is the boiler's water line at the right level, (2) are the radiator valves actually open all the way (they go bad open or closed), (3) does each radiator have a Hoffman 1A or similar air vent at the far end. The vent is the most common stuck part.</p>",
        like_count: 318,
        quote_of_post_number: 1,
      },
      {
        post_number: 3,
        author: "centuryhomesfan",
        author_url: "https://www.reddit.com/u/centuryhomesfan",
        avatar_url: AV_CENTURY,
        author_rank: "Top 1% contributor",
        author_post_count: "2,148 posts",
        author_join_date: "Joined Jan 2019",
        author_karma: "184,520 karma",
        timestamp: "55 minutes ago",
        body_html: "<p>Boiler water line looks normal, valves all turn freely. The vents on the cold radiators look ancient — probably original 1923. Replacement time?</p>",
        like_count: 27,
      },
      {
        post_number: 4,
        author: "oldhouserestorer",
        author_url: "https://www.reddit.com/u/oldhouserestorer",
        avatar_url: AV_RESTORER,
        author_rank: "Trusted restorer",
        author_post_count: "4,503 posts",
        author_join_date: "Joined Aug 2016",
        author_karma: "298,114 karma",
        author_location: "Berwyn, IL",
        timestamp: "42 minutes ago",
        body_html: "<p>Yes — Hoffman 1A vents are like $8 each from any plumbing supply, and a 100-year-old one is almost guaranteed to be the issue. Soak the old one in vinegar overnight before you swap; sometimes that's all it takes. Don't crank them on or off though, they only have one position (open) — adjustment is by venting speed (size 4 vs 5).</p>",
        like_count: 89,
      },
      {
        post_number: 5,
        author: "boilerdad",
        author_url: "https://www.reddit.com/u/boilerdad",
        avatar_url: AV_BOILER,
        author_rank: "HVAC contractor",
        author_post_count: "9,712 posts",
        author_join_date: "Joined May 2014",
        author_karma: "612,033 karma",
        author_location: "Chicago, IL",
        timestamp: "30 minutes ago",
        body_html: "<p>To add — once you've replaced the vents, run the system for an hour and listen carefully near each one. A working vent makes a quiet hiss when steam reaches it, then closes silently. A bad one either won't hiss at all (still stuck closed) or will keep hissing forever and spitting water (stuck open).</p>",
        like_count: 64,
      }
    ];
    window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
      if (cmd === "get_thread") return {
        id: 1923,
        forum_id: 901,
        source_url: "https://www.reddit.com/r/centuryhomes/comments/abc/1923_foursquare_steam_radiator_help",
        title: "1923 Foursquare — upstairs steam radiators aren't kicking on, Hoffman valve advice?",
        op_author: "centuryhomesfan",
        op_author_url: "https://www.reddit.com/u/centuryhomesfan",
        pubdate: ${ts} - 7200,
        reply_count: 5,
        last_poster: "boilerdad",
        last_post_at: ${ts} - 1800,
        excerpt: null,
        read: false,
      };
      if (cmd === "get_forum_threads") return [
        { id: 1922, forum_id: 901, source_url: "u/1922", title: "Prev thread", op_author: "a", pubdate: null, reply_count: 1, last_poster: null, last_post_at: null, excerpt: null, read: true },
        { id: 1923, forum_id: 901, source_url: "x", title: "1923 Foursquare", op_author: "b", pubdate: null, reply_count: 5, last_poster: null, last_post_at: null, excerpt: null, read: false },
        { id: 1924, forum_id: 901, source_url: "u/1924", title: "Next thread", op_author: "c", pubdate: null, reply_count: 2, last_poster: null, last_post_at: null, excerpt: null, read: false },
      ];
      if (cmd === "get_cached_posts") return { posts: POSTS, scraped_at: ${ts} };
      if (cmd === "get_render_mode") return "native";
      if (cmd === "get_render_mode_for_host") return "native";
      if (cmd === "is_thread_in_favorites") return false;
      if (cmd === "list_post_favorites_for_thread") return [];
      if (cmd === "get_last_read_post_index") return null;
      if (cmd === "set_last_read_post_index") return null;
      return oldInvoke(cmd, args);
    };
  `;
  await page.addInitScript(richPostsStub);
  await page.setViewportSize({ width: 1280, height: 1600 });
  await page.goto(`${VITE_URL}/thread/1923`);
  // Wait for the native renderer to flush its first post body
  await page.waitForSelector(".vb-post", { timeout: 5000 });
  // Allow the avatar tinting / pattern pass to settle
  await page.waitForTimeout(500);
  await page.screenshot({
    path: "e2e/screenshots/native-thread-reddit-avatars.png",
    fullPage: true,
  });
});

test("capture v1 thread-view toolbar", async ({ page }) => {
  // Reuse the index stub but extend to return a single thread for the route.
  const threadStub = stub + `
    const oldInvoke = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
      if (cmd === "get_thread") return {
        id: 42, forum_id: 202, source_url: "https://www.reddit.com/r/chicago/comments/abc/",
        title: "Best Italian beef sandwich 2026 — long thread title that should be ellipsised", op_author: "deepdishfan",
        pubdate: Math.floor(Date.now()/1000)-3600, reply_count: 88, last_poster: null,
        last_post_at: null, excerpt: null, read: false,
      };
      if (cmd === "get_forum_threads") return [
        { id: 41, forum_id: 202, source_url: "u/41", title: "Prev thread", op_author: "a", pubdate: null, reply_count: 1, last_poster: null, last_post_at: null, excerpt: null, read: false },
        { id: 42, forum_id: 202, source_url: "https://www.reddit.com/r/chicago/comments/abc/", title: "Best Italian beef sandwich 2026", op_author: "b", pubdate: null, reply_count: 88, last_poster: null, last_post_at: null, excerpt: null, read: false },
        { id: 43, forum_id: 202, source_url: "u/43", title: "Next thread", op_author: "c", pubdate: null, reply_count: 2, last_poster: null, last_post_at: null, excerpt: null, read: false },
      ];
      return oldInvoke(cmd, args);
    };
  `;
  await page.addInitScript(threadStub);
  await page.setViewportSize({ width: 1200, height: 800 });
  await page.goto(`${VITE_URL}/thread/42`);
  await page.waitForSelector("[data-testid='thread-toolbar']", { timeout: 5000 });
  await page.screenshot({ path: "e2e/screenshots/v1-thread-toolbar.png", fullPage: false });
});
