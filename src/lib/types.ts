export type Forum = {
  id: number;
  category_id: number;
  kind: string;
  title: string;
  source_url: string;
  description: string | null;
  poll_interval_s: number;
  thread_count: number;
  post_count: number;
  last_polled_at: number | null;
  last_error: string | null;
  last_visited_at: number | null;
  latest_thread_title: string | null;
  latest_thread_author: string | null;
  latest_thread_at: number | null;
  latest_thread_url: string | null;
  theme: string | null;
  unread: boolean;
};

export type Category = {
  id: number;
  name: string;
  sort_order: number;
};

export type CategoryWithForums = Category & {
  forums: Forum[];
};

export type Thread = {
  id: number;
  forum_id: number;
  source_url: string;
  title: string;
  op_author: string | null;
  op_author_url: string | null;
  pubdate: number | null;
  reply_count: number | null;
  last_poster: string | null;
  last_poster_url: string | null;
  last_post_at: number | null;
  excerpt: string | null;
  read: boolean;
};

/** A single rendered post in the native vBulletin-style view. */
export type Post = {
  post_number?: number;
  author: string;
  author_url?: string;
  avatar_url?: string;
  author_rank?: string;
  author_post_count?: string;
  author_join_date?: string;
  author_location?: string;
  author_karma?: string;
  author_last_active?: string;
  timestamp?: string;
  body_html: string;
  signature_html?: string;
  like_count?: number;
  quote_of_post_number?: number;
};

export type PostsReadyPayload = {
  host: string;
  url: string;
  posts: Post[];
  source: string; // "anthropic" | "manual"
  content_type: string;
};

/** Backend-side enrichment fires this after fetching each unique author's
 * profile (Reddit about.json, XenForo member page, etc.) and extracting
 * avatar + sidebar metadata via agent-supplied locators. Frontend matches
 * by `author` and patches the matching fields into each Post. */
export type AvatarsResolvedPayload = {
  host: string;
  updates: {
    author: string;
    avatar_url?: string;
    post_count?: string;
    karma?: string;
    join_date?: string;
    last_active?: string;
    location?: string;
    rank?: string;
  }[];
};

export type SiteProfileSummary = {
  host: string;
  content_type: string;
  content_probe: string;
  source: string;
  success_count: number;
  fail_count: number;
  last_validated_at: number;
};

export type CachedPostsResponse = {
  posts: Post[];
  scraped_at: number;
};

/** Snapshot fields persisted with a favorite so the /favorites view stays
 * readable even if the parent thread is later pruned. Sent camelCase to
 * match the backend's `#[serde(rename_all = "camelCase")]` on
 * PostFavoriteSnapshot. */
export type PostFavoriteSnapshot = {
  hasPostNumber: boolean;
  author?: string | null;
  timestampRaw?: string | null;
  bodyHtml: string;
  bodyExcerpt?: string | null;
  threadTitle: string;
  threadSourceUrl: string;
};

/** A single favorited post row returned from `list_favorite_posts`. */
export type FavoritePost = {
  thread_id: number;
  post_index: number;
  has_post_number: boolean;
  author: string | null;
  timestamp_raw: string | null;
  body_html: string;
  body_excerpt: string | null;
  thread_title: string;
  thread_source_url: string;
  favorited_at: number;
};

/** Recently-viewed thread row from `list_thread_history`. */
export type HistoryRow = {
  thread_id: number;
  title: string;
  source_url: string;
  forum_id: number | null;
  forum_title: string | null;
  visited_at: number;
};

export type ProfileRevision = {
  id: number;
  host: string;
  content_type: string;
  parent_revision_id: number | null;
  source: string;
  user_complaint: string | null;
  created_at: number;
  is_current: boolean;
};

export type RefineResponse = {
  revision_id: number;
  content_type: string;
  selectors_json: string;
};

/** Fields the user can point at when filing a debugger-mode complaint. */
export type DebugField =
  | "author"
  | "author_url"
  | "avatar"
  | "author_rank"
  | "author_post_count"
  | "author_join_date"
  | "author_location"
  | "timestamp"
  | "body"
  | "post_number"
  | "post"; // whole post (selector or traversal_mode)

export function fmtRelative(unix?: number | null): string {
  if (!unix) return "—";
  const now = Math.floor(Date.now() / 1000);
  const diff = Math.max(0, now - unix);
  if (diff < 60) return `${diff}s ago`;
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  if (diff < 86400 * 30) return `${Math.floor(diff / 86400)}d ago`;
  if (diff < 86400 * 365) return `${Math.floor(diff / (86400 * 30))}mo ago`;
  return `${Math.floor(diff / (86400 * 365))}y ago`;
}

export function fmtCount(n?: number | null): string {
  if (n === null || n === undefined) return "—";
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return String(n);
}

/**
 * Format a Date as classic-vB style:
 *   Today           → "Today, 10:42 PM"
 *   Yesterday       → "Yesterday, 02:15 AM"
 *   Within 7 days   → "Mon, 02:15 PM"
 *   Older           → "12-15-2023, 10:42 PM"
 *
 * Used by fmtVbAbsolute (Unix timestamps from feed/poller data) AND
 * fmtVbPostTimestamp (ISO/text strings from agent-extracted post timestamps)
 * so every place in the app renders dates the same way.
 */
function formatVbDate(d: Date): string {
  const now = new Date();
  const sameDay = d.toDateString() === now.toDateString();
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  const isYesterday = d.toDateString() === yesterday.toDateString();

  const hh12 = ((d.getHours() + 11) % 12) + 1;
  const mm = String(d.getMinutes()).padStart(2, "0");
  const ampm = d.getHours() < 12 ? "AM" : "PM";
  const time = `${String(hh12).padStart(2, "0")}:${mm} ${ampm}`;

  if (sameDay) return `Today, ${time}`;
  if (isYesterday) return `Yesterday, ${time}`;

  const diffDays = Math.floor((now.getTime() - d.getTime()) / 86_400_000);
  if (diffDays > 0 && diffDays < 7) {
    const dayNames = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    return `${dayNames[d.getDay()]}, ${time}`;
  }
  const M = String(d.getMonth() + 1).padStart(2, "0");
  const D = String(d.getDate()).padStart(2, "0");
  return `${M}-${D}-${d.getFullYear()}, ${time}`;
}

/** Decode HTML entities (`&amp;`, `&#39;`, named or numeric) into their
 * actual characters. Used to clean titles that arrive from feed parsers
 * still containing escape sequences from the source HTML — e.g. the feed
 * title "Reviews on Northside Atlanta Labor &amp; Delivery" should display
 * as "Reviews on Northside Atlanta Labor & Delivery". */
export function decodeHtmlEntities(s?: string | null): string {
  if (!s) return "";
  if (typeof document === "undefined") {
    // SSR fallback — handle the common named entities + numeric refs by hand.
    return s
      .replace(/&amp;/g, "&")
      .replace(/&lt;/g, "<")
      .replace(/&gt;/g, ">")
      .replace(/&quot;/g, '"')
      .replace(/&#39;/g, "'")
      .replace(/&apos;/g, "'")
      .replace(/&nbsp;/g, " ")
      .replace(/&#(\d+);/g, (_m, n) => String.fromCharCode(Number(n)))
      .replace(/&#x([0-9a-fA-F]+);/g, (_m, h) => String.fromCharCode(parseInt(h, 16)));
  }
  // Use a textarea so the browser handles every named entity natively
  // without risking HTML injection via innerHTML on a live element.
  const ta = document.createElement("textarea");
  ta.innerHTML = s;
  return ta.value;
}

/** Format a Unix-seconds timestamp in classic vB style. */
export function fmtVbAbsolute(unix?: number | null): string {
  if (!unix) return "—";
  return formatVbDate(new Date(unix * 1000));
}

/** Coerce a value that might be a Unix timestamp (seconds or ms, as string
 * or number), an ISO string, or already-human-readable text into the
 * classic-vB date format. Used for profile-page-derived dates which arrive
 * as strings — Reddit's about.json returns `data.created_utc` as a float
 * unix-seconds value (e.g. 1738883427.0), Discourse returns ISO. Pass-through
 * when the value doesn't parse as a date so platform-specific relative
 * strings ("3 years ago") still render. */
export function fmtSmartDate(raw?: string | null): string {
  if (!raw) return "";
  const t = raw.trim();
  if (!t) return "";

  // Numeric (possibly with trailing .0) — treat as Unix seconds.
  // Seconds-since-epoch values up to year 9999 fit in 10 digits + fractional;
  // millis would be 13. Detect by integer-part length.
  if (/^-?\d+(\.\d+)?$/.test(t)) {
    const n = parseFloat(t);
    if (!Number.isNaN(n) && Math.abs(n) > 1_000_000) {
      const intLen = Math.floor(Math.abs(n)).toString().length;
      const ms = intLen >= 12 ? n : n * 1000;
      const d = new Date(ms);
      if (!Number.isNaN(d.getTime())) return formatVbDate(d);
    }
  }

  // ISO / RFC2822 / Date.parse-able.
  const tryParse = new Date(t);
  if (!Number.isNaN(tryParse.getTime())) return formatVbDate(tryParse);

  return t;
}

/**
 * Reformat a post timestamp into classic-vBulletin style:
 *   - Today          → "Today, 10:42 PM"
 *   - Yesterday      → "Yesterday, 02:15 AM"
 *   - This week      → "Mon, 02:15 PM"
 *   - Older          → "12-15-2023, 10:42 PM"
 *
 * Accepts ISO strings (which is what the extractor pulls from <time datetime>
 * attributes), Unix seconds, or already-human-readable text. If the input
 * doesn't parse as a date it's returned unchanged so platform-specific
 * relative strings ("3 days ago") survive.
 */
export function fmtVbPostTimestamp(raw?: string | null): string {
  if (!raw) return "";
  const trimmed = raw.trim();
  if (!trimmed) return "";

  let d: Date | null = null;
  // Try ISO / RFC2822 / etc.
  const tryParse = new Date(trimmed);
  if (!isNaN(tryParse.getTime())) {
    d = tryParse;
  } else if (/^\d{10,13}$/.test(trimmed)) {
    // Bare numeric — Unix seconds or millis.
    const n = Number(trimmed);
    d = new Date(trimmed.length === 10 ? n * 1000 : n);
  }
  if (!d) return trimmed; // un-parseable, pass through
  return formatVbDate(d);
}

export function fmtVbFooterTime(now: Date = new Date()): string {
  const hh = String(now.getHours()).padStart(2, "0");
  const mm = String(now.getMinutes()).padStart(2, "0");
  // Compute local GMT offset in hours like "GMT -5"
  const tzOffsetMinutes = -now.getTimezoneOffset();
  const sign = tzOffsetMinutes >= 0 ? "+" : "-";
  const absHrs = Math.floor(Math.abs(tzOffsetMinutes) / 60);
  return `All times are GMT ${sign}${absHrs}. The time now is ${hh}:${mm}.`;
}

export function platformLabel(kind: string): string {
  switch (kind) {
    case "reddit":     return "Reddit";
    case "xenforo":    return "XenForo";
    case "vbulletin":  return "vBulletin";
    case "discourse":  return "Discourse";
    case "phpbb":      return "phpBB";
    case "smf":        return "SMF";
    case "ipb":        return "IPB";
    case "ubiquiti":   return "Ubiquiti";
    case "generic_rss":return "RSS";
    default:           return kind;
  }
}
