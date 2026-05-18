import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { goto } from "$app/navigation";
import type {
  Category,
  Forum,
  CategoryWithForums,
  Thread,
  SiteProfileSummary,
  CachedPostsResponse,
  ProfileRevision,
  RefineResponse,
  DebugField,
  FavoritePost,
  PostFavoriteSnapshot,
  HistoryRow,
} from "./types";

export const api = {
  async getCategories(): Promise<CategoryWithForums[]> {
    return await invoke<CategoryWithForums[]>("get_categories");
  },
  async getForum(forumId: number): Promise<Forum | null> {
    return await invoke<Forum | null>("get_forum", { forumId });
  },
  async getThread(threadId: number): Promise<Thread | null> {
    return await invoke<Thread | null>("get_thread", { threadId });
  },
  async getForumThreads(
    forumId: number,
    limit = 50,
    offset = 0,
    sort: "new" | "hot" = "new"
  ): Promise<Thread[]> {
    return await invoke<Thread[]>("get_forum_threads", { forumId, limit, offset, sort });
  },
  async forumSupportsHot(forumId: number): Promise<boolean> {
    return await invoke<boolean>("forum_supports_hot", { forumId });
  },
  async getCategory(categoryId: number): Promise<Category | null> {
    return await invoke<Category | null>("get_category", { categoryId });
  },
  async getCategoryThreads(
    categoryId: number,
    limit = 50,
    offset = 0,
    sort: "new" | "hot" = "new"
  ): Promise<Thread[]> {
    return await invoke<Thread[]>("get_category_threads", { categoryId, limit, offset, sort });
  },
  async categoryThreadCount(categoryId: number): Promise<number> {
    return await invoke<number>("category_thread_count", { categoryId });
  },
  async categorySupportsHot(categoryId: number): Promise<boolean> {
    return await invoke<boolean>("category_supports_hot", { categoryId });
  },
  async searchThreads(query: string, forumId?: number, limit = 100): Promise<Thread[]> {
    return await invoke<Thread[]>("search_threads", { query, forumId: forumId ?? null, limit });
  },
  async markForumVisited(forumId: number): Promise<void> {
    await invoke("mark_forum_visited", { forumId });
  },
  async markAllForumsVisited(): Promise<void> {
    await invoke("mark_all_forums_visited");
  },
  async markThreadRead(threadId: number): Promise<void> {
    await invoke("mark_thread_read", { threadId });
  },
  async markForumThreadsRead(forumId: number): Promise<void> {
    await invoke("mark_forum_threads_read", { forumId });
  },
  async refreshForum(forumId: number): Promise<void> {
    await invoke("refresh_forum", { forumId });
  },
  async refreshAll(): Promise<void> {
    await invoke("refresh_all");
  },
  async openExternal(url: string): Promise<void> {
    await openUrl(url);
  },
  async openThread(threadId: number, _url: string, _title: string): Promise<void> {
    await goto(`/thread/${threadId}`);
  },
  async openThreadByUrl(url: string, title: string): Promise<void> {
    const params = new URLSearchParams({ url, title });
    await goto(`/external?${params.toString()}`);
  },
  async openThreadWebview(
    url: string,
    opts?: { threadId?: number; platformHint?: string }
  ): Promise<void> {
    await invoke("open_thread_webview", {
      url,
      threadId: opts?.threadId ?? null,
      platformHint: opts?.platformHint ?? null,
    });
  },
  async closeThreadWebview(): Promise<void> {
    await invoke("close_thread_webview");
  },
  /** Shrink the thread webview to 1x1 while keeping it loaded — used when
   * switching to native mode so we can still drive lazy-load against it. */
  async shrinkThreadWebview(): Promise<void> {
    await invoke("shrink_thread_webview");
  },
  /** Restore the thread webview to full size beneath the toolbar. */
  async restoreThreadWebview(): Promise<void> {
    await invoke("restore_thread_webview");
  },
  /** Ask the backend to drive the live webview to load more posts
   * (click the load-more button / scroll / navigate per strategy), then
   * re-scrape and emit posts_ready with the expanded set. */
  async loadMorePosts(threadId: number): Promise<void> {
    await invoke("load_more_posts", { threadId });
  },
  /** Apply a zoom factor to both the main UI webview and the child thread
   * webview (when present). Returns the clamped value actually applied. */
  async setZoomLevel(level: number): Promise<number> {
    return await invoke<number>("set_zoom_level", { level });
  },
  /** Tell the backend whether the app is currently in dark mode so the
   * embedded source-site webview gets a matching `color-scheme` hint.
   * Modern sites that honor prefers-color-scheme will follow; legacy
   * sites are unaffected. */
  async setWebviewColorScheme(dark: boolean): Promise<void> {
    await invoke("set_webview_color_scheme", { dark });
  },
  async getSiteProfile(host: string): Promise<SiteProfileSummary[]> {
    return await invoke<SiteProfileSummary[]>("get_site_profile", { host });
  },
  async getCachedPosts(threadId: number): Promise<CachedPostsResponse | null> {
    return await invoke<CachedPostsResponse | null>("get_cached_posts", { threadId });
  },
  async setRenderPreference(host: string, mode: "native" | "webview"): Promise<void> {
    await invoke("set_render_preference", { update: { host, mode } });
  },
  async getRenderPreferences(): Promise<Record<string, "native" | "webview">> {
    return await invoke<Record<string, "native" | "webview">>("get_render_preferences");
  },
  async relearnSiteProfile(host: string): Promise<void> {
    await invoke("relearn_site_profile", { host });
  },
  /** Alias for relearnSiteProfile — semantically "clear all stored layout
   * data for this host so the next page visit re-learns from scratch." */
  async clearLayoutForHost(host: string): Promise<void> {
    await invoke("relearn_site_profile", { host });
  },
  async refineSiteProfile(
    host: string,
    contentType: string,
    field: DebugField,
    elementHtml: string,
    complaint: string
  ): Promise<RefineResponse> {
    return await invoke<RefineResponse>("refine_site_profile", {
      req: { host, contentType, fieldName: field, elementHtml, complaint },
    });
  },
  async listSiteProfileRevisions(
    host: string,
    contentType: string
  ): Promise<ProfileRevision[]> {
    return await invoke<ProfileRevision[]>("list_site_profile_revisions", {
      host,
      contentType,
    });
  },
  async undoSiteProfileRevision(host: string, contentType: string): Promise<boolean> {
    return await invoke<boolean>("undo_site_profile_revision", { host, contentType });
  },
  async resetSiteProfile(host: string, contentType: string): Promise<boolean> {
    return await invoke<boolean>("reset_site_profile", { host, contentType });
  },
  async favoritePost(
    threadId: number,
    postIndex: number,
    snapshot: PostFavoriteSnapshot
  ): Promise<void> {
    await invoke("favorite_post", { threadId, postIndex, snapshot });
  },
  async unfavoritePost(threadId: number, postIndex: number): Promise<void> {
    await invoke("unfavorite_post", { threadId, postIndex });
  },
  async isPostFavorited(threadId: number, postIndex: number): Promise<boolean> {
    return await invoke<boolean>("is_post_favorited", { threadId, postIndex });
  },
  async listThreadFavoriteIndexes(threadId: number): Promise<number[]> {
    return await invoke<number[]>("list_thread_favorite_indexes", { threadId });
  },
  async listFavoritePosts(): Promise<FavoritePost[]> {
    return await invoke<FavoritePost[]>("list_favorite_posts");
  },
  async threadHasFavorites(threadId: number): Promise<boolean> {
    return await invoke<boolean>("thread_has_favorites", { threadId });
  },
  async findThreadIdBySourceUrl(url: string): Promise<number | null> {
    return await invoke<number | null>("find_thread_id_by_source_url", { url });
  },
  async setLastReadPostIndex(threadId: number, postIndex: number): Promise<void> {
    await invoke("set_last_read_post_index", { threadId, postIndex });
  },
  async getLastReadPostIndex(threadId: number): Promise<number | null> {
    return await invoke<number | null>("get_last_read_post_index", { threadId });
  },
  async getFeedsYaml(): Promise<string> {
    return await invoke<string>("get_feeds_yaml");
  },
  /** Persist feeds.yaml. The backend auto-formats (title-first, canonical
   * struct order) and returns the formatted text so the editor can re-sync. */
  async saveFeedsYaml(yaml: string): Promise<string> {
    return await invoke<string>("save_feeds_yaml", { yaml });
  },
  async getFeedsPath(): Promise<string> {
    return await invoke<string>("get_feeds_path");
  },
  async recordThreadVisit(threadId: number): Promise<void> {
    await invoke("record_thread_visit", { threadId });
  },
  async listThreadHistory(limit = 10): Promise<HistoryRow[]> {
    return await invoke<HistoryRow[]>("list_thread_history", { limit });
  },
  async clearThreadHistoryEntry(threadId: number): Promise<void> {
    await invoke("clear_thread_history_entry", { threadId });
  },
  async clearThreadHistoryAll(): Promise<void> {
    await invoke("clear_thread_history_all");
  },
  async feedsAgentSendMessage(
    history: ChatMessage[],
    message: string,
  ): Promise<{ messages: ChatMessage[]; did_modify_feeds: boolean; final_text: string }> {
    return await invoke("feeds_agent_send_message", { history, message });
  },
  async feedsAgentRevert(): Promise<string | null> {
    return await invoke<string | null>("feeds_agent_revert");
  },
};

/** A single message in the agent conversation. `content` is either a plain
 * string (user prompt) or an array of Anthropic-style content blocks (text +
 * tool_use + tool_result mixed). Treat as opaque — round-trip back. */
export type ChatMessage = {
  role: "user" | "assistant";
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  content: string | any[];
};
