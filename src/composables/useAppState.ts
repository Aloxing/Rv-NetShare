import { reactive, readonly } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { AccessRecord, InitialState, PathCheck, ShareSession, SiteSession } from '../types';

// =============================================================================
// Global drag-drop handler registry.
// =============================================================================
// Tauri's webview intercepts OS-level file drops only when the JS listener is
// attached. To survive panel unmounts (SharesPanel -> HistoryPanel etc.), we
// register the listener exactly once at app startup and forward events to a
// callback the active panel installs.

let dropHandler: ((paths: string[]) => void) | null = null;

export function setDropHandler(handler: ((paths: string[]) => void) | null) {
  dropHandler = handler;
}

export function getDropHandler() {
  return dropHandler;
}

type ToastKind = 'info' | 'success' | 'error';
type Toast = { id: number; kind: ToastKind; text: string };

type ReactiveState = {
  ready: boolean;
  initial: InitialState | null;
  shares: ShareSession[];
  sites: SiteSession[];
  history: AccessRecord[];
  toasts: Toast[];
  fontSize: number;
};

const FONT_KEY = 'lan:fontSize';

function loadFontSize(): number {
  if (typeof localStorage === 'undefined') return 13;
  const raw = localStorage.getItem(FONT_KEY);
  const n = raw ? Number.parseInt(raw, 10) : NaN;
  return Number.isFinite(n) && n >= 11 && n <= 18 ? n : 13;
}

function persistFontSize(n: number) {
  if (typeof localStorage !== 'undefined') {
    localStorage.setItem(FONT_KEY, String(n));
  }
  if (typeof document !== 'undefined') {
    document.documentElement.style.setProperty('--font-base-size', n + 'px');
  }
}

const state = reactive<ReactiveState>({
  ready: false,
  initial: null,
  shares: [],
  sites: [],
  history: [],
  toasts: [],
  fontSize: loadFontSize(),
});

function mutate(fn: (s: ReactiveState) => void) {
  fn(state);
}

let toastSeq = 1;
let unlisten: UnlistenFn | null = null;
let ipRefreshTimer: number | null = null;
export async function refreshLocalIp(): Promise<string | null> {
  try {
    const ip = await invoke<string>('refresh_local_ip');
    mutate((s) => {
      if (s.initial) {
        s.initial = { ...s.initial, local_ip: ip };
      }
    });
    return ip;
  } catch (e) {
    // best-effort; keep the previous IP
    console.warn('refreshLocalIp failed', e);
    return null;
  }
}

function startIpRefresh() {
  if (ipRefreshTimer !== null) return;
  // Re-detect every 30 s and whenever the window regains focus.
  ipRefreshTimer = window.setInterval(() => { void refreshLocalIp(); }, 30_000);
  window.addEventListener('focus', refreshLocalIp);
}

function stopIpRefresh() {
  if (ipRefreshTimer !== null) {
    window.clearInterval(ipRefreshTimer);
    ipRefreshTimer = null;
  }
  window.removeEventListener('focus', refreshLocalIp);
}

export async function initAppState() {
  if (state.ready) return;
  persistFontSize(state.fontSize);

  const initial = await invoke<InitialState>('app_initial_state');
  mutate((s) => {
    s.initial = initial;
    s.shares = initial.shares;
    s.sites = initial.sites;
    s.history = initial.history;
    s.ready = true;
  });

  unlisten = await listen<AccessRecord>('share-access', (event) => {
    mutate((s) => { s.history = [event.payload, ...s.history].slice(0, 500); });
    pushToast('info', event.payload.peer + ' 下载了 ' + event.payload.share_name);
  });

  // Register a one-shot drag-drop listener on the webview. The actual handler
  // (which forwards paths to Rust) is set by whichever panel is active via
  // `setDropHandler`. This means drops work regardless of which tab the user
  // is on when they drag.
  try {
    const webview = await import('@tauri-apps/api/webview');
    const wv = webview.getCurrentWebview();
    const dropUnlisten = await wv.onDragDropEvent((event) => {
      const payload = (event as { payload: { type: string; paths?: string[] } }).payload;
      if (payload.type === 'drop') {
        const handler = getDropHandler();
        if (handler) handler(payload.paths ?? []);
      }
    });
    // Stash so disposeAppState can clean up.
    (unlisten as any).__dropUnlisten = dropUnlisten;
  } catch (e) {
    console.warn('global drag-drop listener failed', e);
  }

  startIpRefresh();
}

export function disposeAppState() {
  if (unlisten) {
    const dropUnlisten = (unlisten as any).__dropUnlisten;
    if (typeof dropUnlisten === 'function') dropUnlisten();
    unlisten();
    unlisten = null;
  }
  stopIpRefresh();
  setDropHandler(null);
}

export function useAppState() {
  return readonly(state);
}

export async function refreshShares() {
  const list = await invoke<ShareSession[]>('list_shares');
  mutate((s) => { s.shares = list; });
}

export async function createShare(path: string) {
  const session = await invoke<ShareSession>('create_share', { path });
  mutate((s) => { s.shares = [session, ...s.shares]; });
  return session;
}

export async function removeShare(id: string) {
  await invoke<void>('remove_share', { id });
  mutate((s) => { s.shares = s.shares.filter((x) => x.id !== id); });
}

export async function clearShares() {
  await invoke<void>('clear_shares');
  mutate((s) => { s.shares = []; });
  pushToast('info', '已停止全部分享');
}

export async function refreshSites() {
  const list = await invoke<SiteSession[]>('list_sites');
  mutate((s) => { s.sites = list; });
}

export async function createSite(path: string) {
  const session = await invoke<SiteSession>('create_site', { path });
  mutate((s) => {
    if (!s.sites.some((x) => x.id === session.id)) {
      s.sites = [session, ...s.sites];
    }
  });
  return session;
}

export async function removeSite(id: string) {
  await invoke<void>('remove_site', { id });
  mutate((s) => { s.sites = s.sites.filter((x) => x.id !== id); });
}

export async function clearSites() {
  await invoke<void>('clear_sites');
  mutate((s) => { s.sites = []; });
  pushToast('info', '已移除全部站点');
}

export async function openPath(path: string) {
  return invoke<void>('open_path', { path });
}

export async function clearHistory() {
  await invoke<void>('clear_history');
  mutate((s) => { s.history = []; });
  pushToast('info', '已清空下载记录');
}

export async function removeHistory(id: string) {
  await invoke<void>('remove_history_record', { id });
  mutate((s) => { s.history = s.history.filter((x) => x.id !== id); });
  pushToast('info', '记录已删除');
}

export async function getSaveDir(): Promise<string> {
  return invoke<string>('get_save_dir');
}

export async function setSaveDir(path: string): Promise<string> {
  const resolved = await invoke<string>('set_save_dir', { path });
  mutate((s) => {
    if (s.initial) s.initial = { ...s.initial, save_dir: resolved };
  });
  return resolved;
}

export async function setSharePort(port: number): Promise<number> {
  const bound = await invoke<number>('set_share_port', { port });
  mutate((s) => {
    if (s.initial) {
      s.initial = { ...s.initial, port: bound, share_port: port };
    }
  });
  return bound;
}

export async function setSitePort(port: number): Promise<number> {
  const bound = await invoke<number>('set_site_port', { port });
  await refreshSites();
  mutate((s) => {
    if (s.initial) {
      s.initial = { ...s.initial, site_port: port };
    }
  });
  return bound;
}

export function setFontSize(n: number) {
  const clamped = Math.min(18, Math.max(11, Math.round(n)));
  mutate((s) => { s.fontSize = clamped; });
  persistFontSize(clamped);
}

export function pushToast(kind: ToastKind, text: string) {
  const id = toastSeq++;
  mutate((s) => { s.toasts.push({ id, kind, text }); });
  setTimeout(() => {
    mutate((s) => { s.toasts = s.toasts.filter((t) => t.id !== id); });
  }, 3500);
}

export function buildShareUrl(localIp: string, port: number, id: string, suffix = '') {
  const base = 'http://' + localIp + ':' + port + '/s/' + id;
  return suffix ? base + '/' + suffix : base;
}

export function buildSiteUrl(localIp: string, port: number, suffix = '') {
  const base = 'http://' + localIp + ':' + port + '/';
  return suffix ? base + suffix : base;
}

// ============================================================================
// Rust-driven file-system operations.
// ============================================================================
// The Rust backend owns all access to the real filesystem so that quoting,
// normalisation, and existence checks happen in one place. The frontend
// receives fully validated `PathCheck` / `ShareSession` objects.

export async function checkPaths(paths: string[]): Promise<PathCheck[]> {
  return invoke<PathCheck[]>('check_paths', { paths });
}

export async function readFileMeta(path: string): Promise<PathCheck> {
  return invoke<PathCheck>('read_file_meta', { path });
}

/**
 * Convert a list of raw paths (possibly quoted, possibly with mixed
 * separators) into a list of share sessions, creating new shares on
 * the Rust side. Existing shares pointing at the same canonical path are
 * reused instead of duplicated.
 */
export async function resolveSharePaths(paths: string[]): Promise<ShareSession[]> {
  if (!paths.length) return [];
  const sessions = await invoke<ShareSession[]>('resolve_share_paths', { paths });
  mutate((s) => {
    // Merge with existing shares, dedup by id.
    const seen = new Set(s.shares.map((x) => x.id));
    for (const sess of sessions) {
      if (!seen.has(sess.id)) {
        s.shares = [sess, ...s.shares];
        seen.add(sess.id);
      }
    }
  });
  return sessions;
}
