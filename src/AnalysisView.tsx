import { useEffect, useMemo, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { aggregateAnalysis, type AnalysisSnapshot, type AnalysisRange } from "./analysis-data";
import "./AnalysisView.css";

const compact = new Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 1 });
const exact = new Intl.NumberFormat("en");
const dateLabel = new Intl.DateTimeFormat("en", { month: "short", day: "numeric" });
const monthLabel = new Intl.DateTimeFormat("en", { month: "short", year: "numeric" });

function hourLabel(date: Date) {
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

export default function AnalysisView() {
  const [range, setRange] = useState<AnalysisRange>(1);
  const [snapshot, setSnapshot] = useState<AnalysisSnapshot | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selectedBucket, setSelectedBucket] = useState<string | null>(null);
  const [updatedAt, setUpdatedAt] = useState<Date | null>(null);
  const active = useRef(false);
  const inFlight = useRef(false);

  async function refresh() {
    if (!isTauri() || inFlight.current) return;
    inFlight.current = true;
    setLoading(true);
    try {
      const result = await invoke<AnalysisSnapshot>("read_analysis");
      if (active.current) {
        setSnapshot(result);
        setUpdatedAt(new Date());
        setError(null);
      }
    } catch (failure) {
      if (active.current) setError(typeof failure === "string" ? failure : "Unable to read local sessions. Try again.");
    } finally {
      inFlight.current = false;
      if (active.current) setLoading(false);
    }
  }

  useEffect(() => {
    active.current = true;
    void refresh();
    return () => { active.current = false; };
  }, []);

  const now = useMemo(() => new Date(), [snapshot, range, updatedAt]);
  const analysis = useMemo(() => aggregateAnalysis(snapshot?.events ?? [], range, now), [snapshot, range, now]);

  const { totals, buckets, models } = analysis;
  const total = totals.input + totals.output;
  const cacheRate = totals.input ? Math.round(totals.cached / totals.input * 100) : 0;
  const peak = Math.max(...buckets.map((bucket) => bucket.tokens), 1);
  const selected = buckets.find((bucket) => bucket.key === selectedBucket) ?? buckets[buckets.length - 1];
  const bucketLabel = (date: Date) => range === 1 ? hourLabel(date)
    : range === "lifetime" ? monthLabel.format(date) : dateLabel.format(date);
  const hasData = total > 0;

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
          onClick={() => void refresh()} aria-label="Refresh token usage" title="Refresh local sessions">
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
        <h2>Daily activity</h2>
        <span className="analysis-chart-value" aria-live="polite">
          {bucketLabel(selected.date)} · {snapshot ? `${compact.format(selected.tokens)} tokens` : "—"}
        </span>
      </div>
      <div className="analysis-chart" aria-label={range === "lifetime" ? "Monthly total tokens over local lifetime" : range === 1 ? "Hourly total tokens today" : "Daily total tokens"}>
        {buckets.map((bucket) => {
          const value = bucket.tokens;
          const label = range === 1
            ? `${hourLabel(bucket.date)}–${hourLabel(bucket.end > now ? now : bucket.end)}`
            : bucketLabel(bucket.date);
          return (
            <button type="button" key={bucket.key} className="analysis-chart-day"
              data-selected={selected.key === bucket.key} aria-pressed={selected.key === bucket.key}
              aria-label={`${label}: ${exact.format(value)} tokens`}
              title={`${label}: ${exact.format(value)} tokens`}
              onMouseEnter={() => setSelectedBucket(bucket.key)}
              onFocus={() => setSelectedBucket(bucket.key)} onClick={() => setSelectedBucket(bucket.key)}>
              <span className="analysis-bar" style={{ height: `${value ? Math.max(value / peak * 100, 4) : 3}%` }} data-empty={value === 0} />
            </button>
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
        <h2>Models</h2><span>{models.length ? `${models.length} active` : "—"}</span>
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
