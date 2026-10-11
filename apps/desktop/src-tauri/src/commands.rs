//! Commands the UI calls. They are thin: each one forwards to the library.

use std::path::PathBuf;
use std::sync::Arc;

use roll_core::Library;
use roll_core::actions::EditAction;
use roll_core::categories::CategoryPatch;
use roll_core::model::*;
use roll_core::planner::{PlanOptions, PlanSummary};
use roll_core::rules::Rule;
use roll_core::settings::Settings;
use tauri::State;
use tauri::ipc::Response;
use tauri_plugin_dialog::DialogExt;

pub struct AppState {
    pub lib: Arc<Library>,
    pub log_dir: PathBuf,
}

type Res<T> = Result<T, String>;

fn e(err: impl std::fmt::Display) -> String {
    err.to_string()
}

/// Run blocking library work off the async runtime.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(e)?.map_err(e)
}

#[tauri::command]
pub fn app_info(state: State<AppState>, app: tauri::AppHandle) -> AppInfo {
    let lib = &state.lib;
    AppInfo {
        version: app.package_info().version.to_string(),
        data_dir: lib.data_dir.to_string_lossy().into_owned(),
        exiftool: lib.exiftool.as_ref().map(|p| p.exe().to_string_lossy().into_owned()),
        ffmpeg: lib.ffmpeg.as_ref().map(|f| f.ffmpeg.to_string_lossy().into_owned()),
        os: std::env::consts::OS.into(),
    }
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Settings {
    state.lib.settings()
}

#[tauri::command]
pub fn save_settings(state: State<AppState>, settings: Settings) -> Res<()> {
    state.lib.save_settings(&settings).map_err(e)
}

#[tauri::command]
pub async fn pick_folder(app: tauri::AppHandle, title: String) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog().file().set_title(title).blocking_pick_folder().and_then(|p| p.into_path().ok())
    })
    .await
    .ok()
    .flatten()
    .map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn roots(state: State<AppState>) -> Vec<RootInfo> {
    state.lib.roots()
}

#[tauri::command]
pub fn add_root(state: State<AppState>, path: String) -> Res<RootInfo> {
    state.lib.add_root(std::path::Path::new(&path)).map_err(e)
}

#[tauri::command]
pub fn remove_root(state: State<AppState>, id: i64) -> Res<()> {
    state.lib.remove_root(id).map_err(e)
}

#[tauri::command]
pub fn rescan(state: State<AppState>) -> Res<()> {
    state.lib.rescan().map_err(e)
}

#[tauri::command]
pub async fn timeline(state: State<'_, AppState>, query: Query) -> Res<Response> {
    let lib = state.lib.clone();
    blocking(move || lib.timeline(&query)).await.map(Response::new)
}

#[tauri::command]
pub async fn query_ids(state: State<'_, AppState>, query: Query) -> Res<Vec<i64>> {
    let lib = state.lib.clone();
    blocking(move || lib.ids(&query)).await
}

#[tauri::command]
pub async fn tiles(state: State<'_, AppState>, ids: Vec<i64>) -> Res<Vec<TileInfo>> {
    let lib = state.lib.clone();
    blocking(move || lib.tiles(&ids)).await
}

#[tauri::command]
pub async fn asset_detail(state: State<'_, AppState>, id: i64) -> Res<Option<AssetDetail>> {
    let lib = state.lib.clone();
    blocking(move || lib.detail(id)).await
}

#[tauri::command]
pub async fn folders(state: State<'_, AppState>) -> Res<Vec<FolderNode>> {
    let lib = state.lib.clone();
    blocking(move || lib.folders()).await
}

#[tauri::command]
pub async fn categories(state: State<'_, AppState>) -> Res<Vec<Category>> {
    let lib = state.lib.clone();
    blocking(move || lib.categories()).await
}

#[tauri::command]
pub async fn tag_counts(state: State<'_, AppState>) -> Res<Vec<TagCount>> {
    let lib = state.lib.clone();
    blocking(move || lib.tag_counts()).await
}

#[tauri::command]
pub async fn counts(state: State<'_, AppState>) -> Res<Counts> {
    let lib = state.lib.clone();
    blocking(move || lib.counts()).await
}

#[tauri::command]
pub async fn facets(state: State<'_, AppState>, query: Query) -> Res<Facets> {
    let lib = state.lib.clone();
    blocking(move || lib.facets(&query)).await
}

#[tauri::command]
pub fn jobs(state: State<AppState>) -> Vec<JobInfo> {
    state.lib.jobs.list()
}

#[tauri::command]
pub fn job_control(state: State<AppState>, id: i64, action: String) {
    match action.as_str() {
        "pause" => state.lib.jobs.pause(id),
        "resume" => state.lib.jobs.resume(id),
        "cancel" => state.lib.jobs.cancel(id),
        _ => {}
    }
}

fn asset_path(state: &AppState, id: i64) -> Res<PathBuf> {
    state.lib.asset_file(id).map_err(e)?.map(|(p, _, _)| p).ok_or_else(|| "This item is no longer in the library.".to_string())
}

#[tauri::command]
pub fn show_in_folder(state: State<AppState>, id: i64) -> Res<()> {
    tauri_plugin_opener::reveal_item_in_dir(asset_path(&state, id)?).map_err(e)
}

#[tauri::command]
pub fn open_folder(state: State<AppState>, root_id: i64, path: String) -> Res<()> {
    let root = state.lib.roots().into_iter().find(|r| r.id as i64 == root_id).ok_or("Unknown folder")?;
    let full = roll_core::paths::join_rel(std::path::Path::new(&root.path), &path);
    tauri_plugin_opener::open_path(full, None::<&str>).map_err(e)
}

#[tauri::command]
pub fn open_with_default(state: State<AppState>, id: i64) -> Res<()> {
    tauri_plugin_opener::open_path(asset_path(&state, id)?, None::<&str>).map_err(e)
}

#[tauri::command]
pub fn clear_logs(state: State<AppState>) -> Res<()> {
    if let Ok(rd) = std::fs::read_dir(&state.log_dir) {
        for f in rd.flatten() {
            let _ = std::fs::remove_file(f.path());
        }
    }
    Ok(())
}

#[tauri::command]
pub fn set_secret(name: String, value: String) -> Res<()> {
    roll_core::secrets::set(&name, &value).map_err(e)
}

#[tauri::command]
pub fn has_secret(name: String) -> bool {
    roll_core::secrets::has(&name)
}

#[tauri::command]
pub fn delete_secret(name: String) -> Res<()> {
    roll_core::secrets::delete(&name).map_err(e)
}

// Edits, plans and history

#[tauri::command]
pub async fn edit(state: State<'_, AppState>, ids: Vec<i64>, action: EditAction) -> Res<usize> {
    let lib = state.lib.clone();
    blocking(move || lib.edit(&ids, &action)).await
}

#[tauri::command]
pub async fn set_trashed(state: State<'_, AppState>, ids: Vec<i64>, trashed: bool) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.set_trashed(&ids, trashed)).await
}

#[tauri::command]
pub async fn empty_trash(state: State<'_, AppState>, ids: Option<Vec<i64>>) -> Res<usize> {
    let lib = state.lib.clone();
    blocking(move || lib.empty_trash(ids.as_deref())).await
}

#[tauri::command]
pub async fn plan(state: State<'_, AppState>, options: PlanOptions) -> Res<PlanSummary> {
    let lib = state.lib.clone();
    blocking(move || lib.plan(&options)).await
}

/// Ask where to save the plan as CSV and write it there.
#[tauri::command]
pub async fn save_plan_csv(state: State<'_, AppState>, app: tauri::AppHandle, batch: i64) -> Res<bool> {
    let lib = state.lib.clone();
    blocking(move || {
        let csv = lib.plan_csv(batch)?;
        let Some(path) = app
            .dialog()
            .file()
            .set_title("Export the plan")
            .set_file_name("Roll plan.csv")
            .add_filter("CSV", &["csv"])
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let path = path.into_path().map_err(|e| anyhow::anyhow!("{e}"))?;
        std::fs::write(path, csv)?;
        Ok(true)
    })
    .await
}

#[tauri::command]
pub fn apply_plan(state: State<AppState>, batch: i64) -> Res<()> {
    state.lib.apply(batch).map_err(e)
}

#[tauri::command]
pub async fn undo(state: State<'_, AppState>, batch: i64) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.undo(batch)).await
}

#[tauri::command]
pub async fn undo_last(state: State<'_, AppState>) -> Res<Option<String>> {
    let lib = state.lib.clone();
    blocking(move || lib.undo_last()).await
}

#[tauri::command]
pub async fn history(state: State<'_, AppState>, limit: i64) -> Res<Vec<BatchInfo>> {
    let lib = state.lib.clone();
    blocking(move || lib.history(limit)).await
}

#[tauri::command]
pub async fn unsaved(state: State<'_, AppState>) -> Res<i64> {
    let lib = state.lib.clone();
    blocking(move || lib.unsaved()).await
}

// Categories and rules

#[tauri::command]
pub fn add_category(state: State<AppState>, name: String, parent: Option<String>, definition: String) -> Res<String> {
    state.lib.add_category(&name, parent.as_deref(), &definition).map_err(e)
}

#[tauri::command]
pub fn update_category(state: State<AppState>, key: String, patch: CategoryPatch) -> Res<()> {
    state.lib.update_category(&key, &patch).map_err(e)
}

#[tauri::command]
pub fn delete_category(state: State<AppState>, key: String) -> Res<()> {
    state.lib.delete_category(&key).map_err(e)
}

#[tauri::command]
pub fn rules(state: State<AppState>) -> Res<Vec<Rule>> {
    state.lib.rules().map_err(e)
}

#[tauri::command]
pub fn save_rule(state: State<AppState>, rule: Rule) -> Res<i64> {
    state.lib.save_rule(&rule).map_err(e)
}

#[tauri::command]
pub fn delete_rule(state: State<AppState>, id: i64) -> Res<()> {
    state.lib.delete_rule(id).map_err(e)
}
