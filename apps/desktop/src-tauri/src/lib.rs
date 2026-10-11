//! The thin Tauri layer: commands, events and the `roll://` scheme over
//! roll-core.

mod commands;
mod scheme;

use std::sync::Arc;

use commands::AppState;
use roll_core::{LibEvent, Library};
use tauri::{Emitter, Manager};

/// Logs stay local, rotate daily, keep a week, and never contain image data.
fn init_logs(dir: &std::path::Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    std::fs::create_dir_all(dir).ok()?;
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("roll")
        .filename_suffix("log")
        .max_log_files(7)
        .build(dir)
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    tracing_subscriber::fmt().with_writer(writer).with_ansi(false).with_max_level(tracing::Level::INFO).try_init().ok()?;
    Some(guard)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let data_dir = roll_core::paths::app_data_dir();
    let log_dir = data_dir.join("logs");
    let _guard = init_logs(&log_dir);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .register_asynchronous_uri_scheme_protocol("roll", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                let state = app.state::<AppState>();
                responder.respond(scheme::handle(&state.lib, &request));
            });
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            let sink = Arc::new(move |ev: LibEvent| {
                let _ = match ev {
                    LibEvent::Jobs(list) => handle.emit("jobs", list),
                    LibEvent::Changed(what) => handle.emit("changed", what),
                    LibEvent::Scanned(report) => handle.emit("scanned", report),
                };
            });
            let lib = Library::open(&data_dir, sink)?;
            lib.start();
            app.manage(AppState { lib, log_dir: log_dir.clone() });
            // The UI shows the window after its first paint; this is a fallback.
            let win = app.get_webview_window("main").expect("main window");
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(4));
                let _ = win.show();
            });
            tracing::info!("Roll started, data in {}", data_dir.display());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::get_settings,
            commands::save_settings,
            commands::pick_folder,
            commands::roots,
            commands::add_root,
            commands::remove_root,
            commands::rescan,
            commands::timeline,
            commands::query_ids,
            commands::tiles,
            commands::asset_detail,
            commands::folders,
            commands::categories,
            commands::tag_counts,
            commands::counts,
            commands::facets,
            commands::jobs,
            commands::job_control,
            commands::show_in_folder,
            commands::open_folder,
            commands::open_with_default,
            commands::clear_logs,
            commands::set_secret,
            commands::has_secret,
            commands::delete_secret,
            commands::edit,
            commands::set_trashed,
            commands::empty_trash,
            commands::plan,
            commands::save_plan_csv,
            commands::apply_plan,
            commands::undo,
            commands::undo_last,
            commands::history,
            commands::unsaved,
            commands::add_category,
            commands::update_category,
            commands::delete_category,
            commands::rules,
            commands::save_rule,
            commands::delete_rule,
            commands::date_sets,
            commands::date_preview,
            commands::date_apply,
            commands::date_candidates,
            commands::dismiss,
            commands::places_info,
            commands::install_places,
            commands::place_search,
            commands::place_name,
            commands::tracks,
            commands::import_tracks,
            commands::delete_track,
            commands::place_groups,
            commands::accept_places,
            commands::set_location,
            commands::apply_zone,
            commands::map_points,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Roll");
}
