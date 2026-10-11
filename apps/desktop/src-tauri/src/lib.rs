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

/// A native crash (a decoder or driver fault) would otherwise leave nothing
/// behind. Writes the faulting thread's backtrace to `logs/crash-<ms>.txt`
/// synchronously, since the log writer's thread may not get to run again.
#[cfg(windows)]
fn install_crash_report(dir: &std::path::Path) {
    use windows_sys::Win32::System::Diagnostics::Debug::{EXCEPTION_POINTERS, SetUnhandledExceptionFilter};

    /// Where the report goes; set once before the filter is installed.
    static CRASH_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

    unsafe extern "system" fn on_crash(info: *const EXCEPTION_POINTERS) -> i32 {
        let (code, addr) = unsafe { ((*(*info).ExceptionRecord).ExceptionCode, (*(*info).ExceptionRecord).ExceptionAddress) };
        if let Some(dir) = CRASH_DIR.get() {
            let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
            let report = format!(
                "Roll {} crashed: exception {code:#010x} at {addr:?} on thread {:?}

{}
",
                env!("CARGO_PKG_VERSION"),
                std::thread::current().name(),
                std::backtrace::Backtrace::force_capture()
            );
            let _ = std::fs::write(dir.join(format!("crash-{ms}.txt")), report);
        }
        0 // EXCEPTION_CONTINUE_SEARCH: Windows still reports the crash.
    }

    let _ = CRASH_DIR.set(dir.to_path_buf());
    unsafe { SetUnhandledExceptionFilter(Some(on_crash)) };
}

#[cfg(not(windows))]
fn install_crash_report(_dir: &std::path::Path) {}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let data_dir = roll_core::paths::app_data_dir();
    let log_dir = data_dir.join("logs");
    let _guard = init_logs(&log_dir);
    install_crash_report(&log_dir);

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
            commands::dupe_groups,
            commands::merge_dupes,
            commands::review_summary,
            commands::review_items,
            commands::accept_above,
            commands::discover,
            commands::discover_ignore,
            commands::parse_search,
            commands::packs,
            commands::download_pack,
            commands::cancel_pack,
            commands::remove_pack,
            commands::events,
            commands::event_confirm,
            commands::event_rename,
            commands::event_dismiss,
            commands::event_merge,
            commands::event_split,
            commands::event_own_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Roll");
}
