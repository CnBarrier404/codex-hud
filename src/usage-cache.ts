export type UsageWindow = { usedPercent: number; resetsAt: number | null };
export type AccountInfo = { email: string | null; planType: string | null };
export type UsageSnapshot = {
  account: AccountInfo | null;
  fiveHour: UsageWindow | null;
  weekly: UsageWindow | null;
};

const cacheKey = "codex-hud:usage:v1";
const maxAge = 24 * 60 * 60 * 1000;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isWindow(value: unknown): value is UsageWindow | null {
  return value === null || (isRecord(value) &&
    typeof value.usedPercent === "number" && Number.isFinite(value.usedPercent) &&
    value.usedPercent >= 0 && value.usedPercent <= 100 &&
    (value.resetsAt === null || (typeof value.resetsAt === "number" &&
      Number.isSafeInteger(value.resetsAt) && value.resetsAt > 0)));
}

function isSnapshot(value: unknown): value is UsageSnapshot {
  return isRecord(value) && isWindow(value.fiveHour) && isWindow(value.weekly) &&
    (value.account === null || (isRecord(value.account) &&
      (value.account.email === null || typeof value.account.email === "string") &&
      (value.account.planType === null || typeof value.account.planType === "string")));
}

export function readUsageCache(): UsageSnapshot | null {
  try {
    const raw = localStorage.getItem(cacheKey);
    if (!raw) return null;
    const cached: unknown = JSON.parse(raw);
    if (!isRecord(cached) || cached.version !== 1 ||
      typeof cached.savedAt !== "number" || !Number.isFinite(cached.savedAt) ||
      cached.savedAt > Date.now() || Date.now() - cached.savedAt > maxAge ||
      !isSnapshot(cached.snapshot)) {
      clearUsageCache();
      return null;
    }
    return cached.snapshot;
  } catch {
    clearUsageCache();
    return null;
  }
}

export function saveUsageCache(snapshot: UsageSnapshot) {
  try {
    localStorage.setItem(cacheKey, JSON.stringify({ version: 1, savedAt: Date.now(), snapshot }));
  } catch {}
}

export function clearUsageCache() {
  try {
    localStorage.removeItem(cacheKey);
  } catch {}
}
