export type TokenEvent = {
  timestamp: string;
  model: string;
  session: number;
  input: number;
  cached: number;
  output: number;
  durationMs?: number | null;
};
export type AnalysisSnapshot = { events: TokenEvent[]; skippedFiles: number };
export type AnalysisRange = 1 | 7 | 30 | "lifetime";
type Totals = { input: number; cached: number; output: number };
const emptyTotals = (): Totals => ({ input: 0, cached: 0, output: 0 });
const hour = 60 * 60 * 1000;

function add(target: Totals, event: TokenEvent) {
  target.input += event.input;
  target.cached += event.cached;
  target.output += event.output;
}

export function aggregateAnalysis(events: TokenEvent[], range: AnalysisRange, now: Date) {
  const validEvents = events.map((event) => ({ event, timestamp: new Date(event.timestamp).getTime() }))
    .filter(({ timestamp }) => Number.isFinite(timestamp) && timestamp <= now.getTime());
  const today = new Date(now);
  today.setHours(0, 0, 0, 0);
  const start = new Date(today);
  if (range === "lifetime") {
    start.setTime(validEvents.reduce((earliest, { timestamp }) => Math.min(earliest, timestamp), now.getTime()));
    start.setHours(0, 0, 0, 0);
    start.setDate(1);
  } else {
    start.setDate(start.getDate() - range + 1);
  }
  const count = range === "lifetime"
    ? (now.getFullYear() - start.getFullYear()) * 12 + now.getMonth() - start.getMonth() + 1
    : range === 1 ? Math.floor((now.getTime() - start.getTime()) / hour) + 1 : range;
  const buckets = Array.from({ length: count }, (_, index) => {
    const date = new Date(start);
    if (range === 1) date.setTime(start.getTime() + index * hour);
    else if (range === "lifetime") date.setMonth(start.getMonth() + index);
    else date.setDate(start.getDate() + index);
    const end = new Date(date);
    if (range === 1) end.setTime(date.getTime() + hour);
    else if (range === "lifetime") end.setMonth(date.getMonth() + 1);
    else end.setDate(date.getDate() + 1);
    return { key: String(date.getTime()), date, end, tokens: 0 };
  });
  const totals = emptyTotals();
  const models = new Map<string, Totals>();
  const sessions = new Set<number>();
  const speed = { output: 0, durationMs: 0, samples: 0 };
  for (const { event, timestamp } of validEvents) {
    if (timestamp < start.getTime()) continue;
    const bucket = buckets.find((bucket) => timestamp >= bucket.date.getTime() && timestamp < bucket.end.getTime());
    if (!bucket) continue;
    bucket.tokens += event.input + event.output;
    add(totals, event);
    if (Number.isFinite(event.output) && event.output >= 200 && event.durationMs != null
      && Number.isFinite(event.durationMs) && event.durationMs >= 1_000 && event.durationMs <= 3_600_000) {
      speed.output += event.output;
      speed.durationMs += event.durationMs;
      speed.samples += 1;
    }
    sessions.add(event.session);
    const model = models.get(event.model) ?? emptyTotals();
    add(model, event);
    models.set(event.model, model);
  }
  return {
    buckets, totals, sessions: sessions.size,
    speed: { samples: speed.samples, tokensPerSecond: speed.durationMs > 0 ? speed.output * 1_000 / speed.durationMs : null },
    models: [...models.entries()].map(([name, tokens]) => ({ name, ...tokens }))
      .sort((a, b) => (b.input + b.output) - (a.input + a.output)),
  };
}
