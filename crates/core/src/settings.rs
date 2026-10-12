//! App settings, stored as one JSON document in the `settings` table.
//! Secrets (API keys) are not here: they live in the OS keychain.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::db::Db;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(default)]
pub struct Network {
    pub immich: bool,
    pub ai: bool,
    pub downloads: bool,
    pub online_tiles: bool,
    pub online_search: bool,
    pub update_checks: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(default)]
pub struct Settings {
    pub first_run_done: bool,
    /// "file_and_sidecar" or "sidecar_only".
    pub write_mode: String,
    /// "system", "light" or "dark".
    pub theme: String,
    pub row_height: i32,
    /// "off", "local" or "openrouter".
    pub ai_mode: String,
    pub ai_local_endpoint: Option<String>,
    pub ai_decision_model: Option<String>,
    pub ai_text_model: Option<String>,
    pub ai_budget_usd: f64,
    pub ai_allow_private_cloud: bool,
    pub ai_allow_non_zdr: bool,
    pub immich_url: Option<String>,
    pub network: Network,
    /// CPU threads for background work; `None` uses all but two.
    pub threads: Option<i32>,
    pub use_gpu: bool,
    pub only_when_idle: bool,
    pub pause_on_battery: bool,
    pub preview_cache_gb: f32,
    pub rename_files: bool,
    pub rename_template: String,
    /// "move" or "copy".
    pub organize_mode: String,
    pub copy_target: Option<String>,
    pub keep_folders_in_sync: bool,
    pub home: Option<Vec<f64>>,
    /// Measured precision each category must keep to auto-accept.
    pub auto_precision: f32,
    pub interpolation_minutes: i32,
    /// Camera models whose QuickTime dates are local time, not UTC.
    pub quicktime_local_models: Vec<String>,
    pub guide_done: bool,
    /// Steps of the first guide the owner has opened or marked done.
    pub guide_steps: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            first_run_done: false,
            write_mode: "file_and_sidecar".into(),
            theme: "system".into(),
            row_height: 200,
            ai_mode: "off".into(),
            ai_local_endpoint: None,
            ai_decision_model: None,
            ai_text_model: None,
            ai_budget_usd: 5.0,
            ai_allow_private_cloud: false,
            ai_allow_non_zdr: false,
            immich_url: None,
            network: Network::default(),
            threads: None,
            use_gpu: true,
            only_when_idle: false,
            pause_on_battery: true,
            preview_cache_gb: 5.0,
            rename_files: false,
            rename_template: "{date} {time} {camera}".into(),
            organize_mode: "move".into(),
            copy_target: None,
            keep_folders_in_sync: false,
            home: None,
            auto_precision: 0.98,
            interpolation_minutes: 30,
            quicktime_local_models: Vec::new(),
            guide_done: false,
            guide_steps: Vec::new(),
        }
    }
}

impl Settings {
    pub fn load(db: &Db) -> Settings {
        db.read()
            .query_row("SELECT value FROM settings WHERE key = 'app'", [], |r| r.get::<_, String>(0))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, db: &Db) -> rusqlite::Result<()> {
        db.write().execute(
            "INSERT INTO settings (key, value) VALUES ('app', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [serde_json::to_string(self).unwrap_or_default()],
        )?;
        Ok(())
    }

    pub fn thread_count(&self) -> usize {
        let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        self.threads.map(|t| t.max(1) as usize).unwrap_or_else(|| cpus.saturating_sub(2).max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_with_defaults() {
        let db = Db::memory().unwrap();
        assert_eq!(Settings::load(&db), Settings::default());
        let s = Settings { theme: "dark".into(), ..Default::default() };
        s.save(&db).unwrap();
        assert_eq!(Settings::load(&db).theme, "dark");
        // Unknown and missing keys fall back to defaults.
        db.write().execute("UPDATE settings SET value = '{\"theme\":\"light\",\"old\":1}'", []).unwrap();
        let l = Settings::load(&db);
        assert_eq!((l.theme.as_str(), l.row_height), ("light", 200));
    }
}
