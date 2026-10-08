use rusqlite::{params, Connection, ErrorCode, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    error::Error,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

type CacheResult<T> = Result<T, Box<dyn Error + Send + Sync>>;
const SCHEMA_VERSION: i64 = 2;

fn unsigned(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    u64::try_from(row.get::<_, i64>(index)?).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Tokens {
    input: u64,
    cached: u64,
    output: u64,
}

impl Tokens {
    fn parse(value: &Value) -> Self {
        Self {
            input: value["input_tokens"].as_u64().unwrap_or(0),
            cached: value["cached_input_tokens"].as_u64().unwrap_or(0),
            output: value["output_tokens"].as_u64().unwrap_or(0),
        }
    }

    fn delta(self, previous: Self) -> Self {
        Self {
            input: self.input.saturating_sub(previous.input),
            cached: self.cached.saturating_sub(previous.cached),
            output: self.output.saturating_sub(previous.output),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenEvent {
    timestamp: String,
    model: String,
    session: i64,
    duration_ms: Option<i64>,
    #[serde(flatten)]
    tokens: Tokens,
}

#[derive(Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisSnapshot {
    events: Vec<TokenEvent>,
    skipped_files: usize,
    updated_at: u64,
}

#[derive(Default)]
struct SessionParser {
    model: String,
    previous: Option<Tokens>,
    timer: RequestTimer,
}

fn timestamp_ms(record: &Value) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(record["timestamp"].as_str()?)
        .ok()
        .map(|time| time.timestamp_millis())
}

// Log timestamps describe completed items, not the arrival of the first token.
// Freeze the boundary at the first output so later tool results cannot move it.
#[derive(Default)]
struct RequestTimer {
    boundary: Option<i64>,
    last_count: Option<i64>,
    start: Option<i64>,
    last_output: Option<i64>,
    tool_after_output: Option<i64>,
    usage: Option<(i64, Tokens)>,
}

impl RequestTimer {
    fn observe(&mut self, record: &Value) {
        let Some(at) = timestamp_ms(record) else {
            return;
        };
        let payload = &record["payload"];
        match record["type"].as_str() {
            Some("turn_context") => self.boundary = self.boundary.max(Some(at)),
            Some("event_msg")
                if matches!(
                    payload["type"].as_str(),
                    Some("task_started" | "turn_aborted" | "task_complete")
                ) =>
            {
                *self = Self {
                    boundary: Some(at),
                    last_count: self.last_count,
                    ..Self::default()
                };
            }
            Some("token_usage_record") => {
                self.usage = payload
                    .get("usage")
                    .filter(|value| value.is_object())
                    .map(|value| (at, Tokens::parse(value)));
            }
            Some("response_item") => {
                let kind = payload["type"].as_str().unwrap_or("");
                if kind.ends_with("_output") {
                    self.boundary = self.boundary.max(Some(at));
                    if self.last_output.is_some() {
                        self.tool_after_output = Some(at);
                    }
                } else if kind == "reasoning"
                    || kind.ends_with("_call")
                    || (kind == "message" && payload["role"] == "assistant")
                {
                    // Older logs append the previous response's items in the same flush.
                    if self.start.is_none()
                        && self.last_count.is_some_and(|count| at - count <= 100)
                    {
                        return;
                    }
                    if self.last_output.is_none() {
                        self.start = self.boundary;
                    }
                    self.last_output = Some(at);
                    self.tool_after_output = None;
                } else if kind == "message" {
                    self.boundary = self.boundary.max(Some(at));
                }
            }
            _ => {}
        }
    }

    fn finish(&mut self, at: Option<i64>, last: Option<Tokens>, tokens: Tokens) -> Option<i64> {
        let start = self.start.or(self.boundary);
        let matched_usage = self
            .usage
            .filter(|(_, usage)| last.is_some_and(|last| last == *usage) && *usage == tokens);
        let delayed_by_tools = self
            .tool_after_output
            .zip(at)
            .is_some_and(|(tool, end)| (0..=100).contains(&(end - tool)));
        let end = matched_usage.map(|(end, _)| end).or(if delayed_by_tools {
            self.last_output
        } else {
            at
        });
        *self = Self {
            boundary: self.boundary.max(at),
            last_count: at,
            ..Self::default()
        };
        let duration = end?.checked_sub(start?)?;
        (100..=3_600_000).contains(&duration).then_some(duration)
    }
}

impl SessionParser {
    fn consume(&mut self, record: &Value) -> Option<TokenEvent> {
        self.timer.observe(record);
        if record["type"] == "turn_context" {
            if let Some(model) = record["payload"]["model"].as_str() {
                self.model = model.to_owned();
            }
            return None;
        }
        if record["type"] != "event_msg" || record["payload"]["type"] != "token_count" {
            return None;
        }
        let info = &record["payload"]["info"];
        let total = info
            .get("total_token_usage")
            .filter(|value| value.is_object())?;
        let timestamp = record["timestamp"].as_str()?;
        let current = Tokens::parse(total);
        let tokens = match self.previous {
            Some(previous)
                if current.input >= previous.input && current.output >= previous.output =>
            {
                current.delta(previous)
            }
            _ => info
                .get("last_token_usage")
                .filter(|value| value.is_object())
                .map(Tokens::parse)
                .unwrap_or(current),
        };
        self.previous = Some(current);
        if tokens.input == 0 && tokens.output == 0 {
            return None;
        }
        let last = info
            .get("last_token_usage")
            .filter(|value| value.is_object())
            .map(Tokens::parse);
        let duration_ms = self.timer.finish(timestamp_ms(record), last, tokens);
        // A cumulative delta spanning multiple requests cannot share one request's timing.
        let duration_ms = if last.is_some_and(|last| last != tokens) {
            None
        } else {
            duration_ms
        };
        Some(TokenEvent {
            timestamp: timestamp.to_owned(),
            model: if self.model.is_empty() {
                "Unknown".to_owned()
            } else {
                self.model.clone()
            },
            session: 0,
            duration_ms,
            tokens: Tokens {
                cached: tokens.cached.min(tokens.input),
                ..tokens
            },
        })
    }
}

#[derive(PartialEq, Eq)]
struct Fingerprint {
    size: u64,
    modified: String,
}

impl Fingerprint {
    fn read(path: &Path) -> std::io::Result<Self> {
        let metadata = fs::metadata(path)?;
        Ok(Self {
            size: metadata.len(),
            modified: format!("{:?}", metadata.modified()?),
        })
    }
}

#[derive(Default)]
struct Inventory {
    files: HashMap<PathBuf, Fingerprint>,
    protected: Vec<PathBuf>,
    skipped: usize,
}

impl Inventory {
    fn protect(&mut self, path: &Path) {
        self.protected.push(path.to_owned());
        self.skipped += 1;
    }

    fn scan(&mut self, directory: &Path) {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(_) => {
                self.protect(directory);
                return;
            }
        };
        for entry in entries {
            let Ok(entry) = entry else {
                self.protect(directory);
                continue;
            };
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                self.protect(&path);
                continue;
            };
            if kind.is_dir() {
                self.scan(&path);
            } else if kind.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension == "jsonl")
            {
                match Fingerprint::read(&path) {
                    Ok(fingerprint) => {
                        self.files.insert(path, fingerprint);
                    }
                    Err(_) => self.protect(&path),
                }
            }
        }
    }
}

fn parse_file(path: &Path, fingerprint: &Fingerprint) -> std::io::Result<Vec<TokenEvent>> {
    let file = fs::File::open(path)?;
    let mut parser = SessionParser::default();
    let mut events = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if ![
            "\"turn_context\"",
            "\"event_msg\"",
            "\"response_item\"",
            "\"token_usage_record\"",
        ]
        .iter()
        .any(|kind| line.contains(kind))
        {
            continue;
        }
        // The final line of an active session can still be incomplete.
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(event) = parser.consume(&record) {
            events.push(event);
        }
    }
    // Retry a file that changed during parsing instead of caching a mixed snapshot.
    if Fingerprint::read(path)? != *fingerprint {
        return Err(std::io::Error::other("Session changed while being read"));
    }
    Ok(events)
}

fn initialize(connection: &mut Connection) -> rusqlite::Result<()> {
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version != SCHEMA_VERSION {
        let transaction = connection.transaction()?;
        transaction.execute_batch(
            "DROP TABLE IF EXISTS token_events;
             DROP TABLE IF EXISTS session_files;
             DROP TABLE IF EXISTS sources;
             CREATE TABLE sources (
                 home TEXT PRIMARY KEY,
                 initialized INTEGER NOT NULL DEFAULT 0,
                 updated_at INTEGER NOT NULL DEFAULT 0,
                 skipped_files INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE session_files (
                 id INTEGER PRIMARY KEY,
                 source TEXT NOT NULL REFERENCES sources(home) ON DELETE CASCADE,
                 path TEXT NOT NULL,
                 size INTEGER NOT NULL,
                 modified TEXT NOT NULL,
                 UNIQUE(source, path)
             );
             CREATE TABLE token_events (
                 file_id INTEGER NOT NULL REFERENCES session_files(id) ON DELETE CASCADE,
                 sequence INTEGER NOT NULL,
                 timestamp TEXT NOT NULL,
                 model TEXT NOT NULL,
                 input INTEGER NOT NULL,
                 cached INTEGER NOT NULL,
                 output INTEGER NOT NULL,
                 duration_ms INTEGER,
                 PRIMARY KEY(file_id, sequence)
             );
             CREATE INDEX token_events_timestamp ON token_events(timestamp);
             PRAGMA user_version = 2;",
        )?;
        transaction.commit()?;
    }
    Ok(())
}

fn with_database<T>(
    path: &Path,
    mut operation: impl FnMut(&mut Connection) -> CacheResult<T>,
) -> CacheResult<T> {
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory)?;
    }
    let mut execute = || -> CacheResult<T> {
        let mut connection = Connection::open(path)?;
        initialize(&mut connection)?;
        operation(&mut connection)
    };
    match execute() {
        Err(error)
            if error
                .downcast_ref::<rusqlite::Error>()
                .is_some_and(|error| {
                    matches!(
                        error.sqlite_error_code(),
                        Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase)
                    )
                }) =>
        {
            // The database contains derived usage only. Preserve the corrupt cache for recovery.
            let suffix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
            let backup = path.with_file_name(format!("analysis-corrupt-{suffix}.sqlite"));
            fs::rename(path, &backup)?;
            let journal = PathBuf::from(format!("{}-journal", path.display()));
            if journal.exists() {
                fs::rename(
                    journal,
                    PathBuf::from(format!("{}-journal", backup.display())),
                )?;
            }
            execute()
        }
        result => result,
    }
}

fn snapshot(connection: &Connection, home: &str) -> CacheResult<Option<AnalysisSnapshot>> {
    let metadata = connection
        .query_row(
            "SELECT updated_at, skipped_files FROM sources WHERE home = ?1 AND initialized = 1",
            [home],
            |row| Ok((unsigned(row, 0)?, unsigned(row, 1)? as usize)),
        )
        .optional()?;
    let Some((updated_at, skipped_files)) = metadata else {
        return Ok(None);
    };
    let mut query = connection.prepare(
        "SELECT e.timestamp, e.model, e.file_id, e.input, e.cached, e.output, e.duration_ms
         FROM token_events e JOIN session_files f ON f.id = e.file_id
         WHERE f.source = ?1 ORDER BY e.file_id, e.sequence",
    )?;
    let events = query
        .query_map([home], |row| {
            Ok(TokenEvent {
                timestamp: row.get(0)?,
                model: row.get(1)?,
                session: row.get(2)?,
                duration_ms: row.get(6)?,
                tokens: Tokens {
                    input: unsigned(row, 3)?,
                    cached: unsigned(row, 4)?,
                    output: unsigned(row, 5)?,
                },
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Some(AnalysisSnapshot {
        events,
        skipped_files,
        updated_at,
    }))
}

pub fn load_cache(path: &Path, home: &Path) -> CacheResult<Option<AnalysisSnapshot>> {
    with_database(path, |connection| {
        snapshot(connection, &home.to_string_lossy())
    })
}

pub fn refresh_cache(path: &Path, home: &Path) -> CacheResult<AnalysisSnapshot> {
    // Do not discard cached history if the source itself is temporarily unavailable.
    if !fs::metadata(home)?.is_dir() {
        return Err("Codex data directory is unavailable".into());
    }
    with_database(path, |connection| {
        let source = home.to_string_lossy();
        connection.execute(
            "INSERT OR IGNORE INTO sources(home) VALUES (?1)",
            [source.as_ref()],
        )?;
        let mut cached = HashMap::new();
        {
            let mut query = connection
                .prepare("SELECT id, path, size, modified FROM session_files WHERE source = ?1")?;
            for row in query.query_map([source.as_ref()], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    Fingerprint {
                        size: unsigned(row, 2)?,
                        modified: row.get(3)?,
                    },
                ))
            })? {
                let (id, relative, fingerprint) = row?;
                cached.insert(home.join(relative), (id, fingerprint));
            }
        }
        let mut inventory = Inventory::default();
        inventory.scan(&home.join("sessions"));
        inventory.scan(&home.join("archived_sessions"));
        let mut updates = Vec::new();
        for (file, fingerprint) in &inventory.files {
            if cached
                .get(file)
                .is_some_and(|(_, existing)| existing == fingerprint)
            {
                continue;
            }
            match parse_file(file, fingerprint) {
                Ok(events) => updates.push((file, fingerprint, events)),
                Err(_) => inventory.skipped += 1,
            }
        }
        let transaction = connection.transaction()?;
        {
            let mut insert = transaction.prepare(
                "INSERT INTO token_events(file_id, sequence, timestamp, model, input, cached, output, duration_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for (file, fingerprint, events) in &updates {
                let relative = file.strip_prefix(home)?.to_string_lossy();
                transaction.execute(
                    "INSERT INTO session_files(source, path, size, modified) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(source, path) DO UPDATE SET size = excluded.size, modified = excluded.modified",
                    params![source.as_ref(), relative.as_ref(), i64::try_from(fingerprint.size)?, fingerprint.modified],
                )?;
                let id: i64 = transaction.query_row(
                    "SELECT id FROM session_files WHERE source = ?1 AND path = ?2",
                    params![source.as_ref(), relative.as_ref()],
                    |row| row.get(0),
                )?;
                transaction.execute("DELETE FROM token_events WHERE file_id = ?1", [id])?;
                for (sequence, event) in events.iter().enumerate() {
                    insert.execute(params![
                        id,
                        i64::try_from(sequence)?,
                        event.timestamp,
                        event.model,
                        i64::try_from(event.tokens.input)?,
                        i64::try_from(event.tokens.cached)?,
                        i64::try_from(event.tokens.output)?,
                        event.duration_ms
                    ])?;
                }
            }
        }
        for (file, (id, _)) in &cached {
            if !inventory.files.contains_key(file)
                && !inventory
                    .protected
                    .iter()
                    .any(|prefix| file.starts_with(prefix))
            {
                transaction.execute("DELETE FROM session_files WHERE id = ?1", [id])?;
            }
        }
        let updated_at = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
        transaction.execute(
            "UPDATE sources SET initialized = 1, updated_at = ?2, skipped_files = ?3 WHERE home = ?1",
            params![source.as_ref(), i64::try_from(updated_at)?, i64::try_from(inventory.skipped)?],
        )?;
        transaction.commit()?;
        #[cfg(debug_assertions)]
        eprintln!(
            "Analysis cache: parsed {} of {} session files",
            updates.len(),
            inventory.files.len()
        );
        snapshot(connection, &source)?.ok_or_else(|| "Usage cache was not initialized".into())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record(second: u32, kind: &str, payload: Value) -> Value {
        json!({ "timestamp": format!("2026-10-08T12:{:02}:{:02}.000Z", second / 60, second % 60), "type": kind, "payload": payload })
    }

    fn usage(output: u64) -> Value {
        json!({ "input_tokens": output, "cached_input_tokens": 0, "output_tokens": output })
    }

    fn count(second: u32, total: u64, last: u64) -> Value {
        record(
            second,
            "event_msg",
            json!({ "type": "token_count", "info": {
            "total_token_usage": usage(total), "last_token_usage": usage(last)
        } }),
        )
    }

    fn item(parser: &mut SessionParser, second: u32, kind: &str) {
        parser.consume(&record(second, "response_item", json!({ "type": kind })));
    }

    #[test]
    fn matching_usage_ends_before_tools_and_duration_round_trips() {
        let mut parser = SessionParser::default();
        parser.consume(&record(1, "turn_context", json!({ "model": "test" })));
        item(&mut parser, 5, "reasoning");
        item(&mut parser, 9, "function_call");
        parser.consume(&record(
            10,
            "token_usage_record",
            json!({ "usage": usage(600) }),
        ));
        item(&mut parser, 30, "function_call_output");
        let event = parser.consume(&count(30, 600, 600)).unwrap();
        assert_eq!(event.duration_ms, Some(9_000));
        let mut connection = Connection::open_in_memory().unwrap();
        initialize(&mut connection).unwrap();
        connection
            .execute(
                "INSERT INTO sources(home, initialized) VALUES ('test', 1)",
                [],
            )
            .unwrap();
        connection.execute("INSERT INTO session_files(id, source, path, size, modified) VALUES (1, 'test', 'test.jsonl', 0, '')", []).unwrap();
        connection
            .execute(
                "INSERT INTO token_events VALUES (1, 0, ?1, 'test', 100, 0, 600, ?2)",
                params![event.timestamp, event.duration_ms],
            )
            .unwrap();
        let loaded = snapshot(&connection, "test").unwrap().unwrap();
        assert_eq!(loaded.events[0].duration_ms, Some(9_000));
        assert_eq!(
            serde_json::to_value(&loaded.events[0]).unwrap()["durationMs"],
            9_000
        );
    }

    #[test]
    fn old_logs_exclude_tools_and_duplicate_counts_do_not_move_start() {
        let mut parser = SessionParser::default();
        parser.consume(&record(1, "turn_context", json!({})));
        item(&mut parser, 6, "function_call");
        item(&mut parser, 16, "function_call_output");
        assert_eq!(
            parser.consume(&count(16, 600, 600)).unwrap().duration_ms,
            Some(5_000)
        );
        assert!(parser.consume(&count(17, 600, 600)).is_none());
        item(&mut parser, 20, "reasoning");
        assert_eq!(
            parser.consume(&count(22, 1300, 700)).unwrap().duration_ms,
            Some(6_000)
        );
    }

    #[test]
    fn trailing_old_items_and_user_idle_do_not_extend_next_request() {
        let mut parser = SessionParser::default();
        parser.consume(&record(1, "turn_context", json!({})));
        assert_eq!(
            parser.consume(&count(8, 600, 600)).unwrap().duration_ms,
            Some(7_000)
        );
        item(&mut parser, 8, "reasoning");
        item(&mut parser, 8, "function_call");
        parser.consume(&record(
            100,
            "response_item",
            json!({ "type": "message", "role": "user" }),
        ));
        assert_eq!(
            parser.consume(&count(110, 1300, 700)).unwrap().duration_ms,
            Some(10_000)
        );
    }

    #[test]
    fn aborted_request_resets_timer_but_mid_response_message_does_not() {
        let mut parser = SessionParser::default();
        parser.consume(&record(1, "turn_context", json!({})));
        item(&mut parser, 10, "reasoning");
        parser.consume(&record(20, "event_msg", json!({ "type": "turn_aborted" })));
        parser.consume(&record(100, "event_msg", json!({ "type": "task_started" })));
        item(&mut parser, 110, "reasoning");
        parser.consume(&record(
            115,
            "response_item",
            json!({ "type": "message", "role": "developer" }),
        ));
        assert_eq!(
            parser.consume(&count(120, 600, 600)).unwrap().duration_ms,
            Some(20_000)
        );
        // A reset in cumulative usage keeps the last-request token fallback.
        assert_eq!(
            parser.consume(&count(130, 300, 300)).unwrap().tokens.output,
            300
        );
    }

    #[test]
    fn mismatched_usage_falls_back_and_missing_or_invalid_times_stay_unknown() {
        let mut parser = SessionParser::default();
        assert!(parser
            .consume(&count(1, 600, 600))
            .unwrap()
            .duration_ms
            .is_none());
        parser.consume(&record(10, "turn_context", json!({})));
        item(&mut parser, 15, "reasoning");
        parser.consume(&record(
            18,
            "token_usage_record",
            json!({ "usage": usage(999) }),
        ));
        assert_eq!(
            parser.consume(&count(20, 1300, 700)).unwrap().duration_ms,
            Some(10_000)
        );
        assert!(parser
            .consume(&count(20, 1600, 300))
            .unwrap()
            .duration_ms
            .is_none());
        // The next delta spans more than last_token_usage: timing cannot describe it.
        assert!(parser
            .consume(&count(30, 2200, 300))
            .unwrap()
            .duration_ms
            .is_none());
        let mut timer = RequestTimer {
            boundary: Some(0),
            ..RequestTimer::default()
        };
        assert!(timer
            .finish(Some(3_600_001), None, Tokens::default())
            .is_none());
    }

    #[test]
    fn upgrading_old_cache_invalidates_file_fingerprints_for_history_backfill() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE sources(home TEXT PRIMARY KEY); CREATE TABLE session_files(id INTEGER PRIMARY KEY); CREATE TABLE token_events(file_id INTEGER); INSERT INTO session_files VALUES (1); PRAGMA user_version = 1;").unwrap();
        initialize(&mut connection).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM session_files", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            SCHEMA_VERSION
        );
        assert!(snapshot(&connection, "test").unwrap().is_none());
    }
}
