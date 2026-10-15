//! Commands the UI calls. They are thin: each one forwards to the library.

use std::path::PathBuf;
use std::sync::Arc;

use roll_core::Library;
use roll_core::actions::EditAction;
use roll_core::categories::CategoryPatch;
use roll_core::datefix::{Candidate, DateFix, DatePreview, DateSet};
use roll_core::decide::{AiStatus, CheckItem, CheckReport, DecidePlan, ModelOffer, RunInfo};
use roll_core::discover::Cluster;
use roll_core::dupes::DupeGroup;
use roll_core::events::EventInfo;
use roll_core::immich::{AlbumInfo, ImmichPlan, ImmichStatus, ImmichUser};
use roll_core::import::{ImportInfo, ImportOptions, ImportPlan};
use roll_core::library::PlacesInfo;
use roll_core::model::*;
use roll_core::packs::PackStatus;
use roll_core::places::{LocationResult, PlaceGroup, PlaceHit, TrackInfo};
use roll_core::planner::{PlanOptions, PlanSummary};
use roll_core::review::{ReviewItem, ReviewSummary};
use roll_core::rules::Rule;
use roll_core::search::ParsedSearch;
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

// Dates

#[tauri::command]
pub async fn date_sets(state: State<'_, AppState>) -> Res<Vec<DateSet>> {
    let lib = state.lib.clone();
    blocking(move || lib.date_sets()).await
}

#[tauri::command]
pub async fn date_preview(state: State<'_, AppState>, fix: DateFix) -> Res<DatePreview> {
    let lib = state.lib.clone();
    blocking(move || lib.date_preview(&fix)).await
}

#[tauri::command]
pub async fn date_apply(state: State<'_, AppState>, fix: DateFix) -> Res<usize> {
    let lib = state.lib.clone();
    blocking(move || lib.date_apply(&fix)).await
}

#[tauri::command]
pub async fn date_smart(state: State<'_, AppState>, ids: Vec<i64>) -> Res<Option<roll_core::datefix::SmartOffset>> {
    let lib = state.lib.clone();
    blocking(move || lib.date_smart(&ids)).await
}

#[tauri::command]
pub async fn date_candidates(state: State<'_, AppState>, ids: Vec<i64>) -> Res<Vec<Candidate>> {
    let lib = state.lib.clone();
    blocking(move || lib.date_candidates(&ids)).await
}

#[tauri::command]
pub fn dismiss(state: State<AppState>, kind: String, key: String) -> Res<()> {
    state.lib.dismiss(&kind, &key).map_err(e)
}

// Places

#[tauri::command]
pub async fn places_info(state: State<'_, AppState>) -> Res<PlacesInfo> {
    let lib = state.lib.clone();
    blocking(move || Ok(lib.places_info())).await
}

/// Ask for the folder with GeoNames files and build the place names.
#[tauri::command]
pub async fn install_places(state: State<'_, AppState>, app: tauri::AppHandle) -> Res<Option<PlacesInfo>> {
    let lib = state.lib.clone();
    blocking(move || {
        let Some(dir) = app.dialog().file().set_title("Choose the folder with the GeoNames files").blocking_pick_folder() else {
            return Ok(None);
        };
        let dir = dir.into_path().map_err(|e| anyhow::anyhow!("{e}"))?;
        lib.install_places(&dir).map(Some)
    })
    .await
}

#[tauri::command]
pub async fn place_search(state: State<'_, AppState>, query: String) -> Res<Vec<PlaceHit>> {
    let lib = state.lib.clone();
    blocking(move || Ok(lib.place_search(&query).into_iter().map(PlaceHit::from).collect())).await
}

#[tauri::command]
pub async fn place_name(state: State<'_, AppState>, lat: f64, lon: f64) -> Res<Option<String>> {
    let lib = state.lib.clone();
    blocking(move || Ok(lib.place_name(lat, lon))).await
}

#[tauri::command]
pub async fn tracks(state: State<'_, AppState>) -> Res<Vec<TrackInfo>> {
    let lib = state.lib.clone();
    blocking(move || lib.tracks()).await
}

/// Ask for location history files and import them.
#[tauri::command]
pub async fn import_tracks(state: State<'_, AppState>, app: tauri::AppHandle) -> Res<Vec<TrackInfo>> {
    let lib = state.lib.clone();
    blocking(move || {
        let Some(files) = app
            .dialog()
            .file()
            .set_title("Import location history")
            .add_filter("Location history", &["gpx", "kml", "geojson", "json"])
            .blocking_pick_files()
        else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for f in files {
            let p = f.into_path().map_err(|e| anyhow::anyhow!("{e}"))?;
            out.push(lib.import_track(&p)?);
        }
        Ok(out)
    })
    .await
}

#[tauri::command]
pub fn delete_track(state: State<AppState>, id: i64) -> Res<()> {
    state.lib.delete_track(id).map_err(e)
}

#[tauri::command]
pub async fn place_groups(state: State<'_, AppState>) -> Res<Vec<PlaceGroup>> {
    let lib = state.lib.clone();
    blocking(move || lib.place_groups()).await
}

#[tauri::command]
pub async fn accept_places(state: State<'_, AppState>, key: String) -> Res<LocationResult> {
    let lib = state.lib.clone();
    blocking(move || lib.accept_places(&key).map(|(n, zones)| LocationResult { count: n as i32, zones })).await
}

#[tauri::command]
pub async fn set_location(
    state: State<'_, AppState>,
    ids: Vec<i64>,
    lat: Option<f64>,
    lon: Option<f64>,
    accuracy_m: Option<f64>,
    name: Option<String>,
) -> Res<LocationResult> {
    let lib = state.lib.clone();
    blocking(move || {
        let place = lat.zip(lon).map(|(lat, lon)| roll_geo::Place { lat, lon, accuracy_m: accuracy_m.unwrap_or(10.0) });
        lib.set_location(&ids, place, name.as_deref()).map(|(n, zones)| LocationResult { count: n as i32, zones })
    })
    .await
}

#[tauri::command]
pub async fn apply_zone(state: State<'_, AppState>, ids: Vec<i64>, offset_min: i32, zone: String) -> Res<usize> {
    let lib = state.lib.clone();
    blocking(move || lib.apply_zone(&ids, offset_min, &zone)).await
}

#[tauri::command]
pub async fn map_points(state: State<'_, AppState>, query: Query) -> Res<Response> {
    let lib = state.lib.clone();
    blocking(move || lib.map_points(&query)).await.map(Response::new)
}

// Duplicates

#[tauri::command]
pub async fn dupe_groups(state: State<'_, AppState>) -> Res<Vec<DupeGroup>> {
    let lib = state.lib.clone();
    blocking(move || lib.dupe_groups()).await
}

#[tauri::command]
pub async fn merge_dupes(state: State<'_, AppState>, keep: i64, others: Vec<i64>) -> Res<usize> {
    let lib = state.lib.clone();
    blocking(move || lib.merge_dupes(keep, &others)).await
}

#[tauri::command]
pub async fn best_shots(state: State<'_, AppState>, ids: Vec<i64>) -> Res<Vec<roll_core::bestshot::ShotScore>> {
    let lib = state.lib.clone();
    blocking(move || lib.best_shots(&ids)).await
}

#[tauri::command]
pub async fn exact_plan(state: State<'_, AppState>) -> Res<roll_core::dupes::ExactPlan> {
    let lib = state.lib.clone();
    blocking(move || lib.exact_plan()).await
}

#[tauri::command]
pub async fn merge_all_exact(state: State<'_, AppState>) -> Res<usize> {
    let lib = state.lib.clone();
    blocking(move || lib.merge_all_exact()).await
}

// Review

#[tauri::command]
pub async fn review_summary(state: State<'_, AppState>) -> Res<ReviewSummary> {
    let lib = state.lib.clone();
    blocking(move || lib.review_summary()).await
}

#[tauri::command]
pub async fn review_items(state: State<'_, AppState>, key: String, limit: i64) -> Res<Vec<ReviewItem>> {
    let lib = state.lib.clone();
    blocking(move || lib.review_items(&key, limit)).await
}

#[tauri::command]
pub async fn accept_above(state: State<'_, AppState>, key: String, min: f32) -> Res<usize> {
    let lib = state.lib.clone();
    blocking(move || lib.accept_above(&key, min)).await
}

// Discover

#[tauri::command]
pub async fn discover(state: State<'_, AppState>, split: Option<String>) -> Res<Vec<Cluster>> {
    let lib = state.lib.clone();
    blocking(move || lib.discover(split.as_deref())).await
}

#[tauri::command]
pub fn discover_ignore(state: State<AppState>, key: String) -> Res<()> {
    state.lib.discover_ignore(&key).map_err(e)
}

#[tauri::command]
pub async fn setup_local_ai(state: State<'_, AppState>) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.setup_local_ai()).await
}

// People

#[tauri::command]
pub async fn people(state: State<'_, AppState>) -> Res<Vec<roll_core::people::Person>> {
    let lib = state.lib.clone();
    blocking(move || lib.people()).await
}

#[tauri::command]
pub async fn person_faces(state: State<'_, AppState>, person: i64) -> Res<Vec<roll_core::people::FaceBox>> {
    let lib = state.lib.clone();
    blocking(move || lib.person_faces(person)).await
}

#[tauri::command]
pub async fn faces_in(state: State<'_, AppState>, asset: i64) -> Res<Vec<roll_core::people::FaceBox>> {
    let lib = state.lib.clone();
    blocking(move || lib.faces_in(asset)).await
}

#[tauri::command]
pub async fn name_person(state: State<'_, AppState>, person: i64, name: Option<String>) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.name_person(person, name.as_deref())).await
}

#[tauri::command]
pub async fn merge_people(state: State<'_, AppState>, into: i64, others: Vec<i64>) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.merge_people(into, &others)).await
}

#[tauri::command]
pub async fn not_person(state: State<'_, AppState>, faces: Vec<i64>) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.not_person(&faces)).await
}

#[tauri::command]
pub async fn assign_faces(state: State<'_, AppState>, faces: Vec<i64>, person: Option<i64>, name: Option<String>) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.assign_faces(&faces, person, name.as_deref())).await
}

#[tauri::command]
pub async fn hide_person(state: State<'_, AppState>, person: i64, hidden: bool) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.hide_person(person, hidden)).await
}

#[tauri::command]
pub async fn name_cluster(state: State<'_, AppState>, ids: Vec<i64>) -> Res<roll_core::discover::NameSuggestion> {
    let lib = state.lib.clone();
    blocking(move || lib.name_cluster(&ids)).await
}

// Teach

#[tauri::command]
pub async fn teach(state: State<'_, AppState>, key: String, ids: Vec<i64>) -> Res<roll_core::teach::Taught> {
    let lib = state.lib.clone();
    blocking(move || lib.teach(&key, &ids)).await
}

#[tauri::command]
pub async fn similar_items(state: State<'_, AppState>, ids: Vec<i64>, limit: usize) -> Res<Vec<i64>> {
    let lib = state.lib.clone();
    blocking(move || lib.similar_items(&ids, limit)).await
}

#[tauri::command]
pub async fn parse_search(state: State<'_, AppState>, text: String) -> Res<ParsedSearch> {
    let lib = state.lib.clone();
    blocking(move || Ok(lib.parse_search(&text))).await
}

#[tauri::command]
pub async fn packs(state: State<'_, AppState>) -> Res<Vec<PackStatus>> {
    let lib = state.lib.clone();
    blocking(move || Ok(lib.packs())).await
}

/// The UI asks the owner first (size, source, license).
#[tauri::command]
pub async fn download_pack(state: State<'_, AppState>, id: String) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.download_pack(&id)).await
}

#[tauri::command]
pub async fn cancel_pack(state: State<'_, AppState>, id: String) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || {
        lib.cancel_pack(&id);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn remove_pack(state: State<'_, AppState>, id: String) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.remove_pack(&id)).await
}

#[tauri::command]
pub async fn events(state: State<'_, AppState>) -> Res<Vec<EventInfo>> {
    let lib = state.lib.clone();
    blocking(move || lib.events()).await
}

#[tauri::command]
pub async fn event_confirm(state: State<'_, AppState>, id: i64, name: Option<String>) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.event_confirm(id, name.as_deref())).await
}

#[tauri::command]
pub async fn event_rename(state: State<'_, AppState>, id: i64, name: String) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.event_rename(id, &name)).await
}

#[tauri::command]
pub async fn event_dismiss(state: State<'_, AppState>, id: i64) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.event_dismiss(id)).await
}

#[tauri::command]
pub async fn event_merge(state: State<'_, AppState>, ids: Vec<i64>) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.event_merge(&ids)).await
}

#[tauri::command]
pub async fn event_split(state: State<'_, AppState>, id: i64, at: i64, name: String) -> Res<bool> {
    let lib = state.lib.clone();
    blocking(move || lib.event_split(id, at, &name)).await
}

#[tauri::command]
pub async fn event_own_folder(state: State<'_, AppState>, id: i64, on: bool) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.event_own_folder(id, on)).await
}

// Add photos

/// Start an import of `path`, or of a folder the owner picks.
#[tauri::command]
pub async fn import_start(state: State<'_, AppState>, app: tauri::AppHandle, path: Option<String>) -> Res<Option<i64>> {
    let lib = state.lib.clone();
    blocking(move || {
        let path = match path {
            Some(p) => PathBuf::from(p),
            None => {
                let Some(dir) = app.dialog().file().set_title("Choose the folder to add").blocking_pick_folder() else {
                    return Ok(None);
                };
                dir.into_path().map_err(|e| anyhow::anyhow!("{e}"))?
            }
        };
        lib.import_start(&path).map(Some)
    })
    .await
}

#[tauri::command]
pub async fn imports(state: State<'_, AppState>) -> Res<Vec<ImportInfo>> {
    let lib = state.lib.clone();
    blocking(move || lib.imports()).await
}

#[tauri::command]
pub async fn import_info(state: State<'_, AppState>, id: i64) -> Res<ImportInfo> {
    let lib = state.lib.clone();
    blocking(move || lib.import_info(id)).await
}

#[tauri::command]
pub async fn import_name(state: State<'_, AppState>, id: i64, name: Option<String>) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.import_name(id, name.as_deref())).await
}

#[tauri::command]
pub async fn import_mark(state: State<'_, AppState>, id: i64, dates_ok: Option<bool>, places_ok: Option<bool>) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.import_mark(id, dates_ok, places_ok)).await
}

#[tauri::command]
pub async fn import_leave_out(state: State<'_, AppState>, id: i64, ids: Vec<i64>, out: bool) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.import_leave_out(id, &ids, out)).await
}

#[tauri::command]
pub async fn import_places(state: State<'_, AppState>, id: i64) -> Res<Vec<PlaceGroup>> {
    let lib = state.lib.clone();
    blocking(move || lib.import_places(id)).await
}

#[tauri::command]
pub async fn import_plan(state: State<'_, AppState>, id: i64, opts: ImportOptions) -> Res<ImportPlan> {
    let lib = state.lib.clone();
    blocking(move || lib.import_plan(id, &opts)).await
}

#[tauri::command]
pub async fn import_apply(state: State<'_, AppState>, id: i64, opts: ImportOptions) -> Res<i64> {
    let lib = state.lib.clone();
    blocking(move || lib.import_apply(id, &opts)).await
}

#[tauri::command]
pub async fn import_close(state: State<'_, AppState>, id: i64) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.import_close(id)).await
}

// Immich

/// Open a web page in the browser (provider privacy policies). Only https.
#[tauri::command]
pub fn open_link(url: String) -> Res<()> {
    if !url.starts_with("https://") {
        return Err("Only https links open from Roll.".into());
    }
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(e)
}

#[tauri::command]
pub async fn best_matches(state: State<'_, AppState>, query: Query) -> Res<roll_core::search::BestMatches> {
    let lib = state.lib.clone();
    blocking(move || lib.best_matches(&query)).await
}

#[tauri::command]
pub async fn rerank(state: State<'_, AppState>, query: Query) -> Res<roll_core::search::BestMatches> {
    let lib = state.lib.clone();
    blocking(move || lib.rerank(&query)).await
}

#[tauri::command]
pub async fn ai_search(state: State<'_, AppState>, text: String) -> Res<roll_core::search::ParsedSearch> {
    let lib = state.lib.clone();
    blocking(move || lib.ai_search(&text)).await
}

#[tauri::command]
pub async fn ai_status(state: State<'_, AppState>) -> Res<AiStatus> {
    let lib = state.lib.clone();
    blocking(move || Ok(lib.ai_status())).await
}

#[tauri::command]
pub async fn ai_offers(state: State<'_, AppState>) -> Res<Vec<ModelOffer>> {
    let lib = state.lib.clone();
    blocking(move || lib.ai_offers()).await
}

#[tauri::command]
pub async fn ai_pick(state: State<'_, AppState>, model: String, offer: Option<ModelOffer>) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.ai_pick(&model, offer.as_ref())).await
}

#[tauri::command]
pub async fn ai_plan(state: State<'_, AppState>) -> Res<DecidePlan> {
    let lib = state.lib.clone();
    blocking(move || lib.ai_plan()).await
}

#[tauri::command]
pub async fn ai_start(state: State<'_, AppState>, budget: Option<f64>) -> Res<i64> {
    let lib = state.lib.clone();
    blocking(move || lib.ai_start(budget)).await
}

#[tauri::command]
pub async fn ai_resume(state: State<'_, AppState>, run: i64, budget: Option<f64>) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.ai_resume(run, budget)).await
}

#[tauri::command]
pub async fn ai_runs(state: State<'_, AppState>) -> Res<Vec<RunInfo>> {
    let lib = state.lib.clone();
    blocking(move || lib.ai_runs()).await
}

#[tauri::command]
pub async fn ai_check_sample(state: State<'_, AppState>) -> Res<usize> {
    let lib = state.lib.clone();
    blocking(move || lib.ai_check_sample()).await
}

#[tauri::command]
pub async fn ai_check_items(state: State<'_, AppState>) -> Res<Vec<CheckItem>> {
    let lib = state.lib.clone();
    blocking(move || lib.ai_check_items()).await
}

#[tauri::command]
pub async fn ai_check_label(state: State<'_, AppState>, id: i64, key: Option<String>) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.ai_check_label(id, key.as_deref())).await
}

#[tauri::command]
pub async fn ai_check_start(state: State<'_, AppState>, models: Vec<String>) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.ai_check_start(&models)).await
}

#[tauri::command]
pub async fn ai_check_report(state: State<'_, AppState>) -> Res<CheckReport> {
    let lib = state.lib.clone();
    blocking(move || lib.ai_check_report()).await
}

#[tauri::command]
pub async fn immich_status(state: State<'_, AppState>) -> Res<ImmichStatus> {
    let lib = state.lib.clone();
    blocking(move || Ok(lib.immich_status())).await
}

#[tauri::command]
pub async fn immich_plan(state: State<'_, AppState>) -> Res<ImmichPlan> {
    let lib = state.lib.clone();
    blocking(move || lib.immich_plan()).await
}

#[tauri::command]
pub async fn immich_sync(state: State<'_, AppState>, remove: bool) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.immich_sync(remove)).await
}

#[tauri::command]
pub async fn immich_users(state: State<'_, AppState>) -> Res<Vec<ImmichUser>> {
    let lib = state.lib.clone();
    blocking(move || lib.immich_users()).await
}

#[tauri::command]
pub async fn immich_albums(state: State<'_, AppState>) -> Res<Vec<AlbumInfo>> {
    let lib = state.lib.clone();
    blocking(move || lib.immich_albums()).await
}

#[tauri::command]
pub async fn immich_album(state: State<'_, AppState>, event: i64, send: bool, share: Vec<String>) -> Res<()> {
    let lib = state.lib.clone();
    blocking(move || lib.immich_album(event, send, &share)).await
}

/// Save the Immich CLI script where the owner chooses; false when cancelled.
#[tauri::command]
pub async fn save_immich_script(state: State<'_, AppState>, app: tauri::AppHandle) -> Res<bool> {
    let lib = state.lib.clone();
    blocking(move || {
        let windows = cfg!(windows);
        let script = lib.immich_script(windows)?;
        let (name, ext) = if windows { ("Send to Immich.ps1", "ps1") } else { ("send-to-immich.sh", "sh") };
        let Some(path) = app
            .dialog()
            .file()
            .set_title("Save the Immich CLI script")
            .set_file_name(name)
            .add_filter("Script", &[ext])
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let path = path.into_path().map_err(|e| anyhow::anyhow!("{e}"))?;
        std::fs::write(path, script)?;
        Ok(true)
    })
    .await
}
