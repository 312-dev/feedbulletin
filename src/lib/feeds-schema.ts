import type { Diagnostic } from "@codemirror/lint";
import {
  parseDocument,
  isMap,
  isSeq,
  isScalar,
  LineCounter,
  type Node,
} from "yaml";
import { SKINS } from "./theme/skins";

const ALLOWED_THEMES = new Set<string>(SKINS.map((s) => s.slug));

/** Mirrors Rust's `ForumKind::as_str` — keep in sync with src-tauri/src/config.rs. */
const ALLOWED_KINDS = new Set([
  "reddit",
  "xenforo",
  "vbulletin",
  "discourse",
  "phpbb",
  "smf",
  "ipb",
  "ubiquiti",
  "generic_rss",
]);

type RangeNode = Node & { range?: [number, number, number] };

function diag(
  node: RangeNode | undefined,
  severity: Diagnostic["severity"],
  message: string,
  textLen: number,
): Diagnostic {
  if (!node || !node.range) {
    return { from: 0, to: Math.min(textLen, 1), severity, message, source: "feeds" };
  }
  const [start, , end] = node.range;
  return {
    from: Math.min(start, textLen),
    to: Math.min(end, textLen),
    severity,
    message,
    source: "feeds",
  };
}

function scalarString(node: unknown): string | undefined {
  if (typeof node === "string") return node;
  if (node && isScalar(node as Node)) {
    const v = (node as { value?: unknown }).value;
    return typeof v === "string" ? v : undefined;
  }
  return undefined;
}

function looksLikeUrl(s: string): boolean {
  return /^https?:\/\//i.test(s);
}

function trimForMsg(s: string): string {
  return s.length <= 60 ? s : s.slice(0, 60) + "…";
}

function defaultTitle(
  kind: string | undefined,
  sub: string | undefined,
  user: string | undefined,
  url: string | undefined,
): string | undefined {
  if (kind === "reddit") {
    if (sub) return sub.charAt(0).toUpperCase() + sub.slice(1);
    if (user) return user.charAt(0).toUpperCase() + user.slice(1);
    return "Reddit";
  }
  return url;
}

/** Validate feeds.yaml text. Errors block save; warnings don't. */
export function validateFeedsYaml(text: string): Diagnostic[] {
  const diags: Diagnostic[] = [];
  const textLen = text.length;
  if (text.trim().length === 0) {
    diags.push({ from: 0, to: 0, severity: "error", message: "feeds.yaml is empty", source: "feeds" });
    return diags;
  }

  const lc = new LineCounter();
  const doc = parseDocument(text, { lineCounter: lc, keepSourceTokens: true });

  for (const err of doc.errors) {
    const [from, to] = err.pos ?? [0, 1];
    diags.push({
      from: Math.min(from, textLen),
      to: Math.min(to, textLen),
      severity: "error",
      message: `YAML: ${err.message}`,
      source: "yaml",
    });
  }
  if (diags.length > 0) return diags;

  const root = doc.contents as RangeNode | null;
  if (!root || !isMap(root)) {
    diags.push(diag(root ?? undefined, "error",
      "root must be a mapping with a 'categories:' key", textLen));
    return diags;
  }

  for (const item of root.items) {
    const k = scalarString(item.key);
    if (k !== "categories") {
      diags.push(diag(item.key as RangeNode, "warning",
        `unknown top-level key '${k ?? "?"}' (only 'categories' is recognized)`, textLen));
    }
  }

  const categoriesNode = root.get("categories", true) as RangeNode | undefined;
  if (!categoriesNode) {
    diags.push(diag(root, "error", "missing required key 'categories:'", textLen));
    return diags;
  }
  if (!isSeq(categoriesNode)) {
    diags.push(diag(categoriesNode, "error",
      "'categories' must be a list (start with `- name: ...`)", textLen));
    return diags;
  }
  if (categoriesNode.items.length === 0) {
    diags.push(diag(categoriesNode, "warning",
      "no categories defined — the home screen will be empty", textLen));
  }

  const seenCategoryNames = new Set<string>();
  for (let ci = 0; ci < categoriesNode.items.length; ci++) {
    const cat = categoriesNode.items[ci] as RangeNode;
    if (!isMap(cat)) {
      diags.push(diag(cat, "error",
        `category #${ci + 1} must be a mapping with 'name:' and 'forums:'`, textLen));
      continue;
    }
    const nameNode = cat.get("name", true) as RangeNode | undefined;
    const name = scalarString(cat.get("name"));
    if (!name || name.trim() === "") {
      diags.push(diag((nameNode ?? cat) as RangeNode, "error",
        `category #${ci + 1} missing 'name:' (or empty)`, textLen));
    } else if (seenCategoryNames.has(name)) {
      diags.push(diag(nameNode as RangeNode, "error",
        `duplicate category name '${name}' — names must be unique`, textLen));
    } else {
      seenCategoryNames.add(name);
    }

    const forumsNode = cat.get("forums", true) as RangeNode | undefined;
    if (!forumsNode) {
      diags.push(diag(cat, "error",
        `category '${name ?? "?"}' missing 'forums:' list`, textLen));
      continue;
    }
    if (!isSeq(forumsNode)) {
      diags.push(diag(forumsNode, "error",
        `category '${name ?? "?"}' 'forums' must be a list`, textLen));
      continue;
    }
    if (forumsNode.items.length === 0) {
      diags.push(diag(forumsNode, "warning",
        `category '${name ?? "?"}' has no forums`, textLen));
    }

    for (const item of cat.items) {
      const ck = scalarString(item.key);
      if (ck !== "name" && ck !== "forums") {
        diags.push(diag(item.key as RangeNode, "warning",
          `unknown category key '${ck ?? "?"}' (recognized: name, forums)`, textLen));
      }
    }

    const seenForumTitles = new Set<string>();
    for (let fi = 0; fi < forumsNode.items.length; fi++) {
      const forum = forumsNode.items[fi] as RangeNode;
      if (!isMap(forum)) {
        diags.push(diag(forum, "error",
          `forum #${fi + 1} in '${name}' must be a mapping`, textLen));
        continue;
      }

      const kindNode = forum.get("kind", true) as RangeNode | undefined;
      const kind = scalarString(forum.get("kind"));
      if (!kind) {
        diags.push(diag(forum, "error",
          `forum in '${name}' missing required 'kind:'`, textLen));
      } else if (!ALLOWED_KINDS.has(kind)) {
        const allowed = Array.from(ALLOWED_KINDS).sort().join(", ");
        diags.push(diag(kindNode as RangeNode, "error",
          `unknown kind '${kind}'. Allowed: ${allowed}`, textLen));
      }

      const urlNode = forum.get("url", true) as RangeNode | undefined;
      const url = scalarString(forum.get("url"));
      const subNode = forum.get("sub", true) as RangeNode | undefined;
      const sub = scalarString(forum.get("sub"));
      const userNode = forum.get("user", true) as RangeNode | undefined;
      const user = scalarString(forum.get("user"));

      if (kind === "reddit") {
        if (!url && !sub && !user) {
          diags.push(diag(forum, "error",
            `Reddit forum needs one of 'sub:', 'user:', or 'url:'`, textLen));
        }
        if (sub && user) {
          diags.push(diag(subNode as RangeNode, "warning",
            `Reddit forum has both 'sub:' and 'user:' — only one is used`, textLen));
        }
      } else if (kind) {
        if (!url) {
          diags.push(diag(forum, "error",
            `'${kind}' forum requires 'url:'`, textLen));
        }
        if (sub || user) {
          diags.push(diag((subNode ?? userNode) as RangeNode, "warning",
            `'sub:'/'user:' only apply to kind: reddit — ignored for '${kind}'`, textLen));
        }
      }

      if (url && !looksLikeUrl(url)) {
        diags.push(diag(urlNode as RangeNode, "error",
          `url must start with http:// or https:// — got '${trimForMsg(url)}'`, textLen));
      }

      const title = scalarString(forum.get("title")) ?? defaultTitle(kind, sub, user, url);
      if (title) {
        if (seenForumTitles.has(title)) {
          diags.push(diag(forum, "warning",
            `duplicate forum title '${title}' within category '${name}'`, textLen));
        } else {
          seenForumTitles.add(title);
        }
      }

      const pollNode = forum.get("poll_interval_s", true) as RangeNode | undefined;
      const pollRaw = forum.get("poll_interval_s");
      if (pollNode && pollRaw !== undefined && pollRaw !== null) {
        const n = Number(pollRaw);
        if (!Number.isFinite(n) || n <= 0 || Math.floor(n) !== n) {
          diags.push(diag(pollNode, "error",
            `poll_interval_s must be a positive integer (seconds)`, textLen));
        } else if (n < 60) {
          diags.push(diag(pollNode, "warning",
            `poll_interval_s < 60s is aggressive — most forums rate-limit aggressively below 60`, textLen));
        }
      }

      const themeNode = forum.get("theme", true) as RangeNode | undefined;
      const themeVal = scalarString(forum.get("theme"));
      if (themeVal !== undefined && !ALLOWED_THEMES.has(themeVal)) {
        const allowed = Array.from(ALLOWED_THEMES).sort().join(", ");
        diags.push(diag(themeNode as RangeNode, "error",
          `unknown theme '${themeVal}'. Allowed: ${allowed}`, textLen));
      }

      const knownForumKeys = new Set([
        "kind", "title", "url", "sub", "user",
        "description", "poll_interval_s", "theme", "disabled",
      ]);
      for (const item of forum.items) {
        const fk = scalarString(item.key);
        if (fk && !knownForumKeys.has(fk)) {
          diags.push(diag(item.key as RangeNode, "warning",
            `unknown forum key '${fk}' (recognized: ${Array.from(knownForumKeys).join(", ")})`, textLen));
        }
      }
    }
  }

  return diags;
}

/** True if validation produced any error-severity diagnostic. */
export function hasErrors(diags: Diagnostic[]): boolean {
  return diags.some((d) => d.severity === "error");
}
