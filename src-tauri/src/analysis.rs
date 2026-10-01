use crate::analysis_store::{load_cache, refresh_cache, AnalysisSnapshot};
use std::{
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::Manager;

#[derive(Default)]
struct RefreshState {
    loaded: bool,
    checked_at: Option<Instant>,
}

#[derive(Default)]
pub struct AnalysisState {
    snapshot: Mutex<Option<AnalysisSnapshot>>,
    refresh: tokio::sync::Mutex<RefreshState>,
}

impl AnalysisState {
    fn cached(&self) -> Result<Option<AnalysisSnapshot>, String> {
        self.snapshot
            .lock()
            .map(|snapshot| snapshot.clone())
            .map_err(|_| "Unable to read usage cache.".to_owned())
    }

    fn save(&self, snapshot: AnalysisSnapshot) -> Result<(), String> {
        *self
            .snapshot
            .lock()
            .map_err(|_| "Unable to update usage cache.".to_owned())? = Some(snapshot);
        Ok(())
    }
}

fn cache_paths(app: &tauri::AppHandle) -> Result<(PathBuf, PathBuf), String> {
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(|home| PathBuf::from(home).join(".codex"))
        })
        .ok_or("Unable to locate local Codex sessions.")?;
    let home = if home.is_absolute() {
        home
    } else {
        std::env::current_dir()
            .map_err(|_| "Unable to locate local Codex sessions.")?
            .join(home)
    };
    let home = home.canonicalize().unwrap_or(home);
    let database = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "Unable to locate the usage cache directory.")?
        .join("analysis.sqlite");
    Ok((database, home))
}

#[tauri::command]
pub async fn read_analysis(
    app: tauri::AppHandle,
    state: tauri::State<'_, AnalysisState>,
) -> Result<Option<AnalysisSnapshot>, String> {
    if let Some(snapshot) = state.cached()? {
        return Ok(Some(snapshot));
    }
    let mut refresh = state.refresh.lock().await;
    if refresh.loaded {
        return state.cached();
    }
    let (database, home) = cache_paths(&app)?;
    let snapshot = tauri::async_runtime::spawn_blocking(move || load_cache(&database, &home))
        .await
        .map_err(|_| "Unable to load usage cache.".to_owned())?
        .map_err(|error| {
            eprintln!("Analysis cache load failed: {error}");
            "Unable to load usage cache.".to_owned()
        })?;
    if let Some(snapshot) = &snapshot {
        state.save(snapshot.clone())?;
    }
    refresh.loaded = true;
    Ok(snapshot)
}

#[tauri::command]
pub async fn refresh_analysis(
    app: tauri::AppHandle,
    state: tauri::State<'_, AnalysisState>,
    force: bool,
) -> Result<AnalysisSnapshot, String> {
    let requested_at = Instant::now();
    let mut refresh = state.refresh.lock().await;
    if refresh.checked_at.is_some_and(|checked| {
        checked >= requested_at || (!force && checked.elapsed() < Duration::from_secs(60))
    }) {
        if let Some(snapshot) = state.cached()? {
            return Ok(snapshot);
        }
    }
    let (database, home) = cache_paths(&app)?;
    let snapshot = tauri::async_runtime::spawn_blocking(move || refresh_cache(&database, &home))
        .await
        .map_err(|_| "Unable to refresh local usage.".to_owned())?
        .map_err(|error| {
            eprintln!("Analysis cache refresh failed: {error}");
            "Unable to refresh local usage. Try again.".to_owned()
        })?;
    state.save(snapshot.clone())?;
    refresh.loaded = true;
    refresh.checked_at = Some(Instant::now());
    Ok(snapshot)
}
