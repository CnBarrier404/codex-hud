import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { clearUsageCache, readUsageCache, saveUsageCache, type UsageSnapshot } from "./usage-cache";
import AnalysisView from "./AnalysisView";
import { refreshAnalysis } from "./analysis-cache";
import "./App.css";

const limits = [
  { id: "five-hour", title: "5h Limit", key: "fiveHour" },
  { id: "weekly", title: "Weekly Limit", key: "weekly" },
] as const;

type UsageError = { code: string; message: string };
type View = "limits" | "analysis";
const resetTimeFormat = new Intl.DateTimeFormat("en-GB", {
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
  hour12: false,
});

function resetDisplay(resetsAt: number, dateOnly: boolean) {
  const date = new Date(resetsAt * 1000);
  if (!dateOnly) return resetTimeFormat.format(date);
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function percent(value: number) {
  return `${Number(value.toFixed(1))}%`;
}

function subscriptionLabel(plan: string | null | undefined) {
  if (!plan) return "-";
  return plan.split(/[_-]/).map((word) => word.charAt(0).toUpperCase() + word.slice(1)).join(" ");
}

function resetCountdown(resetsAt: number | null, now: number) {
  if (resetsAt === null) return "-";
  const remaining = resetsAt - Math.floor(now / 1000);
  if (remaining <= 0) return "-";
  const totalMinutes = Math.ceil(remaining / 60);
  const days = Math.floor(totalMinutes / 1440);
  const hours = Math.floor(totalMinutes / 60) % 24;
  const minutes = totalMinutes % 60;
  const time = `${hours}h ${String(minutes).padStart(2, "0")}m`;
  return days > 0 ? `${days}d ${time}` : time;
}

function App() {
  const [view, setView] = useState<View>("limits");
  const [usage, setUsage] = useState<UsageSnapshot | null>(readUsageCache);
  const [usageError, setUsageError] = useState<UsageError | null>(null);
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    if (!isTauri()) return;
    let active = true;
    let inFlight = false;
    let unlisten: (() => void) | undefined;
    const window = getCurrentWindow();
    const refresh = async () => {
      if (!active || inFlight) return;
      inFlight = true;
      try {
        const snapshot = await invoke<UsageSnapshot>("read_usage");
        if (active) {
          setUsage(snapshot);
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
            clearUsageCache();
          }
        }
      } finally {
        inFlight = false;
      }
    };
    const refreshAll = () => {
      void refresh();
      void refreshAnalysis();
    };
    void window.onFocusChanged(({ payload: focused }) => {
      if (focused) refreshAll();
    }).then((stop) => {
      if (active) unlisten = stop;
      else stop();
    }).catch(() => {});
    refreshAll();
    const poll = setInterval(refreshAll, 60_000);
    const clock = setInterval(() => setNow(Date.now()), 1_000);
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
        <h1 id="app-title">Codex HUD</h1>
        <div className="account-info" aria-label="Account and subscription">
          <span className="account-email" title={usage?.account?.email ?? undefined}>
            {usage?.account?.email ?? "-"}
          </span>
          <span className="subscription-tier" aria-label="Subscription tier"
            title={subscriptionLabel(usage?.account?.planType)}>
            {subscriptionLabel(usage?.account?.planType)}
          </span>
        </div>
      </header>

      <nav className="panel-nav" aria-label="HUD pages">
        {(["limits", "analysis"] as const).map((page) => (
          <button className={view === page ? "active" : ""} type="button" key={page}
            aria-current={view === page ? "page" : undefined} onClick={() => setView(page)}>
            {page === "limits" ? "Limits" : "Analysis"}
          </button>
        ))}
      </nav>

      <div className="panel-content">
      {view === "limits" && <>
      <div className="limits">
        {limits.map(({ id, title, key }) => {
          const window = usage?.[key];
          const expired = window?.resetsAt != null && window.resetsAt * 1000 <= now;
          const remaining = window && !expired ? 100 - window.usedPercent : null;
          return (
            <section className="limit" key={id} aria-labelledby={id}>
              <div className="limit-heading">
                <h2 id={id}>{title}</h2>
              </div>
              <div className="usage-track" aria-hidden="true">
                {remaining !== null && (
                  <div className="usage-fill"
                    data-level={remaining <= 10 ? "low" : remaining <= 25 ? "warning" : "normal"}
                    style={{ width: `${remaining}%` }} />
                )}
              </div>
              <dl className="limit-details">
                <div className="limit-remaining">
                  <dt>left</dt>
                  <dd>{remaining === null ? "—" : percent(remaining)}</dd>
                </div>
                <div className="reset-details">
                  <div>
                    <dt>Resets in</dt>
                    <dd>{window ? resetCountdown(window.resetsAt, now) : "-"}</dd>
                  </div>
                  <div>
                    <dt>Resets at</dt>
                    <dd title={window?.resetsAt ? new Date(window.resetsAt * 1000).toLocaleString() : undefined}>
                      {window?.resetsAt && !expired
                        ? resetDisplay(window.resetsAt, key === "weekly") : "-"}
                    </dd>
                  </div>
                </div>
              </dl>
            </section>
          );
        })}
      </div>

      {usageError && (
        <p className="usage-status" role="status" title={usageError?.message}>
          {`${usage ? "Last reading · " : ""}${usageError.message}`}
        </p>
      )}
      </>}
      {view === "analysis" && <AnalysisView />}
      </div>

    </main>
  );
}

export default App;
