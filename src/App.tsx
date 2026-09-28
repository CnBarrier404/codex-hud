import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { clearUsageCache, readUsageCache, saveUsageCache, type UsageSnapshot } from "./usage-cache";
import "./App.css";

const limits = [
  { id: "five-hour", title: "5h Limit", key: "fiveHour" },
  { id: "weekly", title: "Weekly Limit", key: "weekly" },
] as const;

type UsageError = { code: string; message: string };

function percent(value: number) {
  return `${Number(value.toFixed(1))}%`;
}

function subscriptionLabel(plan: string | null | undefined) {
  if (!plan) return "-";
  return plan.split(/[_-]/).map((word) => word.charAt(0).toUpperCase() + word.slice(1)).join(" ");
}

function resetCountdown(resetsAt: number | null, now: number) {
  if (resetsAt === null) return "-";
  const seconds = resetsAt - Math.floor(now / 1000);
  if (seconds <= 0) return "Refreshing…";
  const minutes = Math.ceil(seconds / 60);
  const days = Math.floor(minutes / 1440);
  const hours = Math.floor((minutes % 1440) / 60);
  if (days > 0) return `${days}d ${hours}h`;
  return `${hours}h ${minutes % 60}m`;
}

function App() {
  const [usage, setUsage] = useState<UsageSnapshot | null>(readUsageCache);
  const [cached, setCached] = useState(usage !== null);
  const [usageError, setUsageError] = useState<UsageError | null>(null);
  const [loading, setLoading] = useState(true);
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    if (!isTauri()) {
      setLoading(false);
      return;
    }
    let active = true;
    let inFlight = false;
    let visible = false;
    let unlisten: (() => void) | undefined;
    const window = getCurrentWindow();
    const refresh = async () => {
      if (!active || inFlight) return;
      inFlight = true;
      setLoading(true);
      try {
        const snapshot = await invoke<UsageSnapshot>("read_usage");
        if (active) {
          setUsage(snapshot);
          setCached(false);
          saveUsageCache(snapshot);
          setUsageError(null);
          setNow(Date.now());
        }
      } catch (failure: unknown) {
        if (active) {
          const error: UsageError = typeof failure === "object" && failure !== null &&
            "code" in failure && "message" in failure
            ? failure as UsageError
            : { code: "unavailable", message: "Unable to refresh usage. Try again." };
          setUsageError(error);
          if (["login", "auth_mode", "missing", "no_windows"].includes(error.code)) {
            setUsage(null);
            setCached(false);
            clearUsageCache();
          }
        }
      } finally {
        inFlight = false;
        if (active) setLoading(false);
      }
    };
    void window.onFocusChanged(({ payload: focused }) => {
      visible = focused;
      if (focused) void refresh();
    }).then((stop) => {
      if (active) unlisten = stop;
      else stop();
    }).catch(() => {});
    void window.isVisible().then((shown) => {
      if (active) {
        visible = shown;
        void refresh();
      }
    }).catch(() => { void refresh(); });
    const poll = setInterval(() => { if (visible) void refresh(); }, 60_000);
    const clock = setInterval(() => { if (visible) setNow(Date.now()); }, 1_000);
    return () => {
      active = false;
      unlisten?.();
      clearInterval(poll);
      clearInterval(clock);
    };
  }, []);

  useEffect(() => {
    let active = true;
    if (isTauri()) {
      invoke<boolean>("mica_enabled")
        .then((enabled) => {
          if (active && enabled) document.documentElement.dataset.mica = "true";
        })
        .catch((error: unknown) => console.error("Unable to read window appearance:", error));
    }
    return () => {
      active = false;
      delete document.documentElement.dataset.mica;
    };
  }, []);

  return (
    <main
      className="usage-panel"
      aria-labelledby="app-title"
      onContextMenu={(event) => event.preventDefault()}
    >
      <header className="panel-header">
        <h1 id="app-title">Codex</h1>
        <div className="account-info" aria-label="Account and subscription">
          <span className="account-email" title={usage?.account?.email ?? undefined}>
            {usage?.account?.email ?? "-"}
          </span>
          <span className="subscription-tier" aria-label="Subscription tier">
            {subscriptionLabel(usage?.account?.planType)}
          </span>
        </div>
      </header>

      <div className="limits">
        {limits.map(({ id, title, key }) => {
          const window = usage?.[key];
          const expired = window?.resetsAt != null && window.resetsAt * 1000 <= now;
          const remaining = window && !expired ? 100 - window.usedPercent : null;
          return (
            <section className="limit" key={id} aria-labelledby={id}>
              <div className="limit-heading">
                <h2 id={id}>{title}</h2>
                <span className="usage-value" title="Remaining quota"
                  aria-label={remaining === null ? "Usage unavailable" : `${percent(remaining)} remaining`}>
                  {remaining === null ? "-" : percent(remaining)}
                </span>
              </div>
              <div className="usage-track" aria-hidden="true">
                {remaining !== null && (
                  <div className="usage-fill"
                    data-level={remaining <= 10 ? "low" : remaining <= 25 ? "warning" : "normal"}
                    style={{ width: `${remaining}%` }} />
                )}
              </div>
              <dl className="limit-details">
                <div>
                  <dt>Used</dt>
                  <dd>{window && !expired ? percent(window.usedPercent) : "-"}</dd>
                </div>
                <div>
                  <dt>Resets in</dt>
                  <dd>{window ? resetCountdown(window.resetsAt, now) : "-"}</dd>
                </div>
              </dl>
            </section>
          );
        })}
      </div>

      {(usageError || cached || (loading && !usage)) && (
        <p className="usage-status" role="status" title={usageError?.message}>
          {usageError ? `${usage ? "Last reading · " : ""}${usageError.message}`
            : cached ? "Cached · Refreshing…" : "Loading usage…"}
        </p>
      )}

      <footer className="panel-footer">
        <span>Codex HUD</span>
      </footer>
    </main>
  );
}

export default App;
