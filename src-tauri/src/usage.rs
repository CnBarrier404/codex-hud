use serde::Serialize;
use serde_json::{json, Value};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, ChildStdout, Command},
    sync::Mutex,
    time::{timeout, Instant},
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    used_percent: f64,
    resets_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    email: Option<String>,
    plan_type: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    account: Option<AccountInfo>,
    five_hour: Option<UsageWindow>,
    weekly: Option<UsageWindow>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageError {
    code: &'static str,
    message: &'static str,
}

fn error(code: &'static str, message: &'static str) -> UsageError {
    UsageError { code, message }
}

#[derive(Default)]
pub struct UsageState(Mutex<Option<(Instant, UsageSnapshot)>>);

#[tauri::command]
pub async fn read_usage(state: tauri::State<'_, UsageState>) -> Result<UsageSnapshot, UsageError> {
    let mut cache = state.0.lock().await;
    if let Some((at, snapshot)) = cache.as_ref() {
        if at.elapsed() < Duration::from_secs(10) {
            return Ok(snapshot.clone());
        }
    }
    match fetch_usage().await {
        Ok(snapshot) => {
            *cache = Some((Instant::now(), snapshot.clone()));
            Ok(snapshot)
        }
        Err(error) => {
            *cache = None;
            Err(error)
        }
    }
}

fn codex_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            candidates.push(directory.join(if cfg!(windows) { "codex.exe" } else { "codex" }));
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local).join("Programs/OpenAI/Codex/bin/codex.exe"));
    }
    if let Some(roaming) = std::env::var_os("APPDATA") {
        let package = PathBuf::from(roaming).join("npm/node_modules/@openai/codex");
        candidates.push(package.join("vendor/x86_64-pc-windows-msvc/codex/codex.exe"));
        candidates.push(package.join(
            "node_modules/@openai/codex-win32-x64/vendor/x86_64-pc-windows-msvc/codex/codex.exe",
        ));
        candidates.push(package.join(
            "node_modules/@openai/codex-win32-arm64/vendor/aarch64-pc-windows-msvc/codex/codex.exe",
        ));
    }
    candidates
}

async fn send(stdin: &mut ChildStdin, message: Value) -> Result<(), UsageError> {
    let mut bytes = message.to_string().into_bytes();
    bytes.push(b'\n');
    stdin
        .write_all(&bytes)
        .await
        .map_err(|_| connection_error())?;
    stdin.flush().await.map_err(|_| connection_error())
}

fn connection_error() -> UsageError {
    error("connection", "Unable to communicate with Codex. Try again.")
}

#[cfg(windows)]
struct QueryJob(usize);

#[cfg(windows)]
impl QueryJob {
    fn attach(process: std::os::windows::io::RawHandle) -> Result<Self, UsageError> {
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(connection_error());
            }
            let job = Self(handle as usize);
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const std::ffi::c_void,
                std::mem::size_of_val(&info) as u32,
            ) == 0
                || AssignProcessToJobObject(handle, process) == 0
            {
                return Err(connection_error());
            }
            Ok(job)
        }
    }
}

#[cfg(windows)]
impl Drop for QueryJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0 as *mut std::ffi::c_void);
        }
    }
}

async fn request(
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
    id: u64,
    method: &str,
    params: Value,
) -> Result<Value, UsageError> {
    send(
        stdin,
        json!({ "id": id, "method": method, "params": params }),
    )
    .await?;
    loop {
        let mut line = String::new();
        if stdout
            .read_line(&mut line)
            .await
            .map_err(|_| connection_error())?
            == 0
        {
            return Err(connection_error());
        }
        let message: Value = serde_json::from_str(&line).map_err(|_| connection_error())?;
        if message.get("id").and_then(Value::as_u64) != Some(id) {
            continue;
        }
        if let Some(failure) = message.get("error") {
            let text = failure.to_string().to_lowercase();
            if [
                "unauthorized",
                "401",
                "revoked",
                "refresh token",
                "not authenticated",
                "sign in",
                "not logged",
            ]
            .iter()
            .any(|word| text.contains(word))
            {
                return Err(error(
                    "login",
                    "Sign in to Codex with your ChatGPT account.",
                ));
            }
            if text.contains("method not found") || failure["code"] == -32601 {
                return Err(error(
                    "unsupported",
                    "Update Codex to read subscription limits.",
                ));
            }
            return Err(error(
                "unavailable",
                "Unable to refresh usage. Check your connection.",
            ));
        }
        return message.get("result").cloned().ok_or_else(connection_error);
    }
}

fn parse_usage(result: &Value) -> Result<UsageSnapshot, UsageError> {
    let bucket = result
        .pointer("/rateLimitsByLimitId/codex")
        .filter(|value| value.is_object())
        .or_else(|| {
            result.get("rateLimits").filter(|value| {
                value.is_object()
                    && value
                        .get("limitId")
                        .and_then(Value::as_str)
                        .is_none_or(|id| id == "codex")
            })
        })
        .ok_or_else(|| error("unavailable", "Codex usage limits are unavailable."))?;
    let mut snapshot = UsageSnapshot {
        account: None,
        five_hour: None,
        weekly: None,
    };
    for name in ["primary", "secondary"] {
        let window = &bucket[name];
        let Some(used) = window["usedPercent"]
            .as_f64()
            .filter(|used| (0.0..=100.0).contains(used))
        else {
            continue;
        };
        let parsed = UsageWindow {
            used_percent: used,
            resets_at: window["resetsAt"].as_i64().filter(|at| *at > 0),
        };
        match window["windowDurationMins"].as_u64() {
            Some(300) => snapshot.five_hour = Some(parsed),
            Some(10080) => snapshot.weekly = Some(parsed),
            _ => {}
        }
    }
    if snapshot.five_hour.is_none() && snapshot.weekly.is_none() {
        return Err(error(
            "no_windows",
            "No 5h or weekly quota is available for this account.",
        ));
    }
    Ok(snapshot)
}

async fn fetch_usage() -> Result<UsageSnapshot, UsageError> {
    let executable = codex_candidates()
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            error(
                "missing",
                "Install Codex and sign in with your ChatGPT account.",
            )
        })?;
    let mut command = Command::new(executable);
    command
        .args(["app-server", "--listen", "stdio://"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|_| connection_error())?;
    #[cfg(windows)]
    let _job = QueryJob::attach(child.raw_handle().ok_or_else(connection_error)?)?;
    let mut stdin = child.stdin.take().ok_or_else(connection_error)?;
    let mut stdout = BufReader::new(child.stdout.take().ok_or_else(connection_error)?);
    let result = timeout(Duration::from_secs(30), async {
        request(&mut stdin, &mut stdout, 1, "initialize", json!({
            "clientInfo": { "name": "codex_hud", "title": "Codex HUD", "version": env!("CARGO_PKG_VERSION") }
        })).await?;
        send(&mut stdin, json!({ "method": "initialized", "params": {} })).await?;
        let account = request(&mut stdin, &mut stdout, 2, "account/read", json!({ "refreshToken": false })).await?;
        let kind = account.pointer("/account/type").and_then(Value::as_str);
        if kind.is_none() {
            return Err(error("login", "Sign in to Codex with your ChatGPT account."));
        }
        if matches!(kind, Some("apiKey" | "amazonBedrock")) {
            return Err(error("auth_mode", "ChatGPT sign-in is required for subscription limits."));
        }
        let result = request(&mut stdin, &mut stdout, 3, "account/rateLimits/read", json!({})).await?;
        let mut snapshot = parse_usage(&result)?;
        snapshot.account = Some(AccountInfo {
            email: account.pointer("/account/email").and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty()).map(str::to_owned),
            plan_type: account.pointer("/account/planType").and_then(Value::as_str)
                .or_else(|| result.pointer("/rateLimitsByLimitId/codex/planType").and_then(Value::as_str))
                .or_else(|| result.pointer("/rateLimits/planType").and_then(Value::as_str))
                .filter(|value| !value.trim().is_empty()).map(str::to_owned),
        });
        Ok(snapshot)
    }).await.unwrap_or_else(|_| Err(error("timeout", "Usage request timed out. Try again.")));
    drop(stdin);
    drop(stdout);
    if timeout(Duration::from_secs(2), child.wait()).await.is_err() {
        let _ = child.kill().await;
    }
    result
}
