import { invoke, isTauri } from "@tauri-apps/api/core";
import type { AnalysisSnapshot } from "./analysis-data";

type CacheState = {
  snapshot: AnalysisSnapshot | null;
  loading: boolean;
  error: string | null;
  checkedAt: number;
};

let state: CacheState = { snapshot: null, loading: false, error: null, checkedAt: 0 };
const listeners = new Set<() => void>();
let loaded = false;
let loadPromise: Promise<void> | null = null;
let refreshPromise: Promise<void> | null = null;

function update(patch: Partial<CacheState>) {
  state = { ...state, ...patch };
  for (const listener of listeners) listener();
}

export function getAnalysisCache() {
  return state;
}

export function subscribeAnalysisCache(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

function loadCache() {
  if (loaded) return Promise.resolve();
  if (!loadPromise) {
    loadPromise = invoke<AnalysisSnapshot | null>("read_analysis").then((snapshot) => {
      update({ snapshot, error: null });
      loaded = true;
    }).finally(() => { loadPromise = null; });
  }
  return loadPromise;
}

export function refreshAnalysis(force = false): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  if (refreshPromise) return refreshPromise;
  if (!force && loaded && Date.now() - state.checkedAt < 60_000) return Promise.resolve();
  update({ loading: true });
  refreshPromise = (async () => {
    // Show the persisted snapshot before checking session files in the background.
    await loadCache();
    const snapshot = await invoke<AnalysisSnapshot>("refresh_analysis", { force });
    update({ snapshot, checkedAt: Date.now(), error: null });
  })().catch((failure: unknown) => {
    update({ error: typeof failure === "string" ? failure : "Unable to refresh local usage. Try again." });
  }).finally(() => {
    refreshPromise = null;
    update({ loading: false });
  });
  return refreshPromise;
}
