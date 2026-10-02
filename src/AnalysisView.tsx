import { useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { aggregateAnalysis, type AnalysisRange } from "./analysis-data";
import { getAnalysisCache, refreshAnalysis, subscribeAnalysisCache } from "./analysis-cache";
import "./AnalysisView.css";

const compact = new Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 1 });
const exact = new Intl.NumberFormat("en");
const dateLabel = new Intl.DateTimeFormat("en", { month: "short", day: "numeric" });
const monthLabel = new Intl.DateTimeFormat("en", { month: "short", year: "numeric" });

function hourLabel(date: Date) {
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

function barPath(x: number, y: number, width: number, height: number) {
  const radius = Math.min(4, width / 2, height);
  return `M${x} ${y + height}V${y + radius}Q${x} ${y} ${x + radius} ${y}` +
    `H${x + width - radius}Q${x + width} ${y} ${x + width} ${y + radius}V${y + height}Z`;
}

export default function AnalysisView() {
  const [range, setRange] = useState<AnalysisRange>(1);
  const { snapshot, loading, error, checkedAt } = useSyncExternalStore(subscribeAnalysisCache, getAnalysisCache);
  const [selectedBucket, setSelectedBucket] = useState<string | null>(null);
  const chartRef = useRef<HTMLDivElement>(null);
  const [barLayout, setBarLayout] = useState<{ width: number; height: number; lefts: number[] }>({ width: 0, height: 0, lefts: [] });

  useEffect(() => {
    if (!isTauri()) return;
    let active = true;
    let visible = false;
    let unlisten: (() => void) | undefined;
    const window = getCurrentWindow();
    void refreshAnalysis();
    void window.onFocusChanged(({ payload: focused }) => {
      visible = focused;
      if (focused) void refreshAnalysis();
      else setSelectedBucket(null);
    }).then((stop) => {
      if (active) unlisten = stop;
      else stop();
    }).catch(() => {});
    void window.isVisible().then((shown) => { if (active) visible = shown; }).catch(() => {});
    const poll = setInterval(() => { if (visible) void refreshAnalysis(); }, 60_000);
    return () => {
      active = false;
      unlisten?.();
      clearInterval(poll);
    };
  }, []);

  const now = useMemo(() => new Date(), [snapshot, range, checkedAt]);
  const analysis = useMemo(() => aggregateAnalysis(snapshot?.events ?? [], range, now), [snapshot, range, now]);

  const { totals, buckets, models } = analysis;
  const total = totals.input + totals.output;
  const cacheRate = totals.input ? Math.round(totals.cached / totals.input * 100) : 0;
  const peak = Math.max(...buckets.map((bucket) => bucket.tokens), 1);
  const selected = buckets.find((bucket) => bucket.key === selectedBucket);
  const displayed = selected ?? buckets[buckets.length - 1];
  const bucketLabel = (date: Date) => range === 1 ? hourLabel(date)
    : range === "lifetime" ? monthLabel.format(date) : dateLabel.format(date);
  const hasData = total > 0;

  useLayoutEffect(() => {
    const chart = chartRef.current;
    if (!chart) return;
    const measure = () => {
      const scale = window.devicePixelRatio;
      const bounds = chart.getBoundingClientRect();
      const columns = Array.from(chart.querySelectorAll(".analysis-chart-day"), (column) => column.getBoundingClientRect());
      if (!columns.length) return;
      // Equal flex columns can fall between device pixels. Snap both bar widths and edges.
      const pixelWidth = Math.max(1, Math.floor(Math.min(28, ...columns.map((column) => column.width)) * scale));
      const width = pixelWidth / scale;
      const firstPixel = Math.ceil(bounds.left * scale);
      const lastPixel = Math.floor(bounds.right * scale) - pixelWidth;
      const lefts = columns.map((column) => {
        const left = Math.round((column.left + (column.width - width) / 2) * scale);
        return Math.max(firstPixel, Math.min(lastPixel, left)) / scale - bounds.left;
      });
      setBarLayout({ width, height: chart.clientHeight, lefts });
    };
    let resolution: MediaQueryList;
    const watchScale = () => {
      resolution?.removeEventListener("change", watchScale);
      measure();
      resolution = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
      resolution.addEventListener("change", watchScale);
    };
    watchScale();
    const observer = new ResizeObserver(measure);
    observer.observe(chart);
    return () => {
      observer.disconnect();
      resolution.removeEventListener("change", watchScale);
    };
  }, [buckets.length]);

  return (
    <section className="analysis-view" aria-label="Token usage analysis" aria-busy={loading}>
      <div className="analysis-toolbar">
        <div className="analysis-ranges" aria-label="Analysis period">
          {([1, 7, 30, "lifetime"] as const).map((value) => (
            <button type="button" key={value} aria-pressed={range === value}
              onClick={() => { setRange(value); setSelectedBucket(null); }}>
              {value === "lifetime" ? "Lifetime" : value === 1 ? "Today" : `${value} days`}
            </button>
          ))}
        </div>
        <button type="button" className="analysis-refresh" disabled={loading || !isTauri()}
          onClick={() => void refreshAnalysis(true)} aria-label="Refresh token usage" title="Refresh local sessions">
          <svg viewBox="0 0 16 16" width="14" height="14" fill="none" aria-hidden="true">
            <path d="M13 6a5.2 5.2 0 1 0 .1 3M13 2.5V6H9.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        </button>
      </div>

      <div className="analysis-total">
        <span className="analysis-label">Total tokens</span>
        <div><strong title={exact.format(total)}>{snapshot ? compact.format(total) : "—"}</strong></div>
      </div>
      <dl className="analysis-metrics">
        <div><dt>Input</dt><dd title={exact.format(totals.input)}>{snapshot ? compact.format(totals.input) : "—"}</dd></div>
        <div><dt>Output</dt><dd title={exact.format(totals.output)}>{snapshot ? compact.format(totals.output) : "—"}</dd></div>
        <div><dt>Cache hit</dt><dd title={`${exact.format(totals.cached)} cached input tokens`}>{snapshot && hasData ? `${cacheRate}%` : "—"}</dd></div>
      </dl>

      <div className="analysis-section-heading">
        <h2>{range === 1 ? "Hourly activity" : range === "lifetime" ? "Monthly activity" : "Daily activity"}</h2>
        <span className="analysis-chart-value" aria-live="polite">
          {bucketLabel(displayed.date)} · {snapshot ? `${compact.format(displayed.tokens)} tokens` : "—"}
        </span>
      </div>
      <div className="analysis-chart" ref={chartRef} aria-label={range === "lifetime" ? "Monthly total tokens over local lifetime" : range === 1 ? "Hourly total tokens today" : "Daily total tokens"}>
        <svg className="analysis-chart-plot" aria-hidden="true" focusable="false">
          {buckets.map((bucket, index) => {
            const height = barLayout.height * (bucket.tokens ? Math.max(bucket.tokens / peak, 0.04) : 0.03);
            return <path key={bucket.key} className="analysis-bar"
              d={barPath(barLayout.lefts[index] ?? 0, barLayout.height - height, barLayout.width, height)}
              data-empty={bucket.tokens === 0} data-selected={selected?.key === bucket.key} />;
          })}
        </svg>
        {buckets.map((bucket) => {
          const value = bucket.tokens;
          const label = range === 1
            ? `${hourLabel(bucket.date)}–${hourLabel(bucket.end > now ? now : bucket.end)}`
            : bucketLabel(bucket.date);
          return (
            <button type="button" key={bucket.key} className="analysis-chart-day"
              data-selected={selected?.key === bucket.key} aria-pressed={selected?.key === bucket.key}
              aria-label={`${label}: ${exact.format(value)} tokens`}
              title={`${label}: ${exact.format(value)} tokens`}
              onMouseEnter={() => setSelectedBucket(bucket.key)}
              onMouseLeave={() => setSelectedBucket(null)}
              onFocus={() => setSelectedBucket(bucket.key)} onBlur={() => setSelectedBucket(null)}
              onClick={() => setSelectedBucket(bucket.key)} />
          );
        })}
      </div>
      <div className="analysis-chart-axis">
        <span>{bucketLabel(buckets[0].date)}</span>
        <span>{range === "lifetime" ? "Tokens / month" : "Tokens"}</span>
        <span>{range === "lifetime" ? "This month" : range === 1 ? "Now" : "Today"}</span>
      </div>

      {loading && !snapshot && <p className="analysis-message" role="status">Reading local sessions…</p>}
      {error && <p className="analysis-message analysis-error" role="alert">{error}</p>}
      {!isTauri() && <p className="analysis-message">Open the desktop app to view your local Codex usage.</p>}
      {snapshot && !hasData && <p className="analysis-message">No token usage in this period. Try a longer range or start a Codex session.</p>}

      <div className="analysis-section-heading analysis-model-heading">
        <h2>Models</h2>
      </div>
      <ul className="analysis-models">
        {models.map((model) => {
          const value = model.input + model.output;
          const share = total ? value / total * 100 : 0;
          return (
            <li key={model.name}>
              <div className="analysis-model-label"><span title={model.name}>{model.name}</span>
                <strong title={`${exact.format(model.input)} input + ${exact.format(model.output)} output`}>{compact.format(value)}</strong></div>
              <div className="analysis-model-detail"><div className="analysis-model-track"><span style={{ width: `${share}%` }} /></div>
                <span>{share < 1 ? "<1" : Math.round(share)}%</span></div>
            </li>
          );
        })}
      </ul>
      {snapshot && snapshot.skippedFiles > 0 && <p className="analysis-message" role="status">Some session files could not be read.</p>}
    </section>
  );
}
