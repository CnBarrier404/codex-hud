use serde::Serialize;
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

#[derive(Default, Clone, Copy, Serialize)]
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenEvent {
    timestamp: String,
    model: String,
    session: usize,
    #[serde(flatten)]
    tokens: Tokens,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisSnapshot {
    events: Vec<TokenEvent>,
    skipped_files: usize,
}

#[derive(Default)]
struct SessionParser {
    model: String,
    previous: Option<Tokens>,
}

impl SessionParser {
    fn consume(&mut self, record: &Value, session: usize) -> Option<TokenEvent> {
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
        Some(TokenEvent {
            timestamp: timestamp.to_owned(),
            model: if self.model.is_empty() {
                "Unknown".to_owned()
            } else {
                self.model.clone()
            },
            session,
            tokens: Tokens {
                cached: tokens.cached.min(tokens.input),
                ..tokens
            },
        })
    }
}

fn scan(directory: &Path, snapshot: &mut AnalysisSnapshot, session: &mut usize) {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(_) => {
            snapshot.skipped_files += 1;
            return;
        }
    };
    for entry in entries {
        let Ok(entry) = entry else {
            snapshot.skipped_files += 1;
            continue;
        };
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            snapshot.skipped_files += 1;
            continue;
        };
        if kind.is_dir() {
            scan(&path, snapshot, session);
        } else if kind.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "jsonl")
        {
            let Ok(file) = fs::File::open(&path) else {
                snapshot.skipped_files += 1;
                continue;
            };
            *session += 1;
            let mut parser = SessionParser::default();
            for line in BufReader::new(file).lines() {
                let Ok(line) = line else {
                    snapshot.skipped_files += 1;
                    break;
                };
                // Skip conversation and tool content before decoding large JSON records.
                if !line.contains("\"turn_context\"") && !line.contains("\"token_count\"") {
                    continue;
                }
                // Active sessions may end with a partially written JSON line.
                let Ok(record) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if let Some(event) = parser.consume(&record, *session) {
                    snapshot.events.push(event);
                }
            }
        }
    }
}

#[tauri::command]
pub async fn read_analysis() -> Result<AnalysisSnapshot, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let home = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("USERPROFILE")
                    .or_else(|| std::env::var_os("HOME"))
                    .map(|home| PathBuf::from(home).join(".codex"))
            })
            .ok_or("Unable to locate local Codex sessions.")?;
        let mut snapshot = AnalysisSnapshot::default();
        let mut session = 0;
        scan(&home.join("sessions"), &mut snapshot, &mut session);
        scan(&home.join("archived_sessions"), &mut snapshot, &mut session);
        Ok(snapshot)
    })
    .await
    .map_err(|_| "Unable to read local Codex sessions.".to_owned())?
}
