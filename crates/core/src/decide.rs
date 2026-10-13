//! Decision runs (PROMPT 5.2–5.4): the items rules and local models left
//! uncertain go to a decision model in numbered batches, and its answers
//! become proposals in Review.
//!
//! - **Resumable.** Every item of a run has a row (pending, sent, done,
//!   failed). A run that stopped or crashed continues with what is not
//!   done; an item the same model and prompt version already answered is
//!   never planned again.
//! - **Budget.** Before a batch is sent, its worst-case cost is reserved
//!   against the run's cap, so the measured spend never passes it.
//! - **Throughput.** Up to 8 batches in flight, starting at 4, halved after
//!   a rate limit or server error; retries wait with jitter.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use roll_ai::chat::{ChatError, Endpoint, Reply};
use roll_ai::cost::{self, Assumptions, Price};
use roll_ai::decide::{self as prompt, CategoryDef, Hints, Item, Verdict};

use crate::db::{Db, now_ms};
use crate::edits::{self, Change};

/// Proposals at or above this confidence count as settled; "Accept all at
/// 95% or higher" in Review takes them.
pub const SETTLED: f64 = 0.95;

/// Where a run's requests go.
#[derive(Debug, Clone)]
pub struct Provider {
    pub endpoint: Endpoint,
    /// "openrouter", or a local server's base URL.
    pub name: String,
    /// "OpenRouter", "Ollama"…, for the UI.
    pub label: String,
    pub model: String,
    pub price: Price,
    /// The cheapest ZDR provider behind OpenRouter, when known.
    pub upstream: Option<String>,
    pub allow_non_zdr: bool,
}

impl Provider {
    pub fn cloud(&self) -> bool {
        self.endpoint.is_openrouter()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LocalServer {
    pub name: String,
    pub base: String,
    /// Installed models first, then suggestions to download; best first.
    pub models: Vec<LocalModelInfo>,
}

/// A local model, installed or suggested.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LocalModelInfo {
    pub name: String,
    pub installed: bool,
    pub download_gb: Option<f64>,
    /// Graphics memory to run it fully on the card, about.
    pub memory_gb: Option<f64>,
    /// "Best", "Very good", "Good", "Basic", or "Not rated".
    pub quality: String,
    /// Where the quality comes from.
    pub basis: Option<String>,
    /// Accuracy measured on the owner's items by the model check.
    pub measured: Option<f64>,
    /// Fits the graphics card's memory, when that is known.
    pub fits: Option<bool>,
    /// The highest quality in the list.
    pub best: bool,
}

/// What the AI page shows about the configured provider.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AiStatus {
    /// "off", "local" or "openrouter".
    pub mode: String,
    /// Local servers answering on their default ports.
    pub local: Vec<LocalServer>,
    /// The local server in use (set or detected).
    pub endpoint: Option<String>,
    pub model: Option<String>,
    pub has_key: bool,
    pub network_on: bool,
    /// Graphics card memory, when it can be read.
    pub gpu_memory_gb: Option<f64>,
    /// What is missing before a run can start, in plain words.
    pub problem: Option<String>,
}

/// One model in the OpenRouter picker.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ModelOffer {
    pub id: String,
    pub name: String,
    pub provider: Option<String>,
    /// USD per million tokens.
    pub prompt_per_m: f64,
    pub completion_per_m: f64,
    /// USD per image, when charged separately.
    pub image_usd: f64,
    pub zdr: bool,
    pub structured: bool,
    pub per_100k_usd: f64,
    /// "Best", "Very good", "Good", "Basic", or "Not rated".
    pub quality: String,
    pub basis: Option<String>,
    /// Accuracy measured on the owner's items by the model check.
    pub measured: Option<f64>,
    /// The highest quality in the list.
    pub best: bool,
    /// How the provider handles the thumbnails, in plain words.
    pub data: String,
    /// The provider's home country (ISO code) and privacy policy.
    pub country: Option<String>,
    pub policy_url: Option<String>,
}

/// Quality order: measured accuracy, then the tier, then price.
fn quality_key(measured: Option<f64>, tier: Option<u8>, price: f64) -> (u8, i64, u8, i64) {
    (u8::from(measured.is_none()), -(measured.unwrap_or(0.0) * 1000.0).round() as i64, tier.unwrap_or(9), (price * 1e6) as i64)
}

/// Accuracy per model from the latest model check runs.
pub fn measured(c: &Connection) -> rusqlite::Result<HashMap<String, f64>> {
    Ok(check_report(c)?.models.into_iter().filter(|m| m.answered >= 20).map(|m| (m.model, m.accuracy)).collect())
}

/// OpenRouter offers with quality, data handling and the best one marked.
pub fn rank_offers(
    offers: Vec<roll_ai::openrouter::Offer>,
    providers: &HashMap<String, roll_ai::openrouter::ProviderInfo>,
    measured: &HashMap<String, f64>,
) -> Vec<ModelOffer> {
    let mut out: Vec<(Option<u8>, ModelOffer)> = offers
        .into_iter()
        .map(|o| {
            let known = roll_ai::catalog::cloud(&o.id);
            let p = o.provider.as_deref().and_then(|n| providers.get(&n.to_lowercase()));
            let mut m: ModelOffer = o.into();
            m.measured = measured.get(&m.id).copied();
            m.quality = known.as_ref().map_or("Not rated", |k| k.tier.label()).to_string();
            m.basis = known.as_ref().map(|k| k.basis.to_string());
            m.country = p.and_then(|p| p.headquarters.clone());
            m.policy_url = p.and_then(|p| p.privacy_policy_url.clone());
            (known.map(|k| k.tier.0), m)
        })
        .collect();
    out.sort_by_key(|(t, m)| quality_key(m.measured, *t, m.per_100k_usd));
    if let Some((_, first)) = out.first_mut() {
        first.best = true;
    }
    out.into_iter().map(|(_, m)| m).collect()
}

/// Installed models on a local server plus the catalogue's suggestions,
/// best first. `sizes` are installed sizes in bytes where the server says.
pub fn rank_local(
    installed: &[String],
    sizes: &HashMap<String, u64>,
    gpu_gb: Option<f64>,
    measured: &HashMap<String, f64>,
    ollama: bool,
) -> Vec<LocalModelInfo> {
    let mut rows: Vec<(Option<u8>, LocalModelInfo)> = Vec::new();
    let mut known_tags = std::collections::HashSet::new();
    for name in installed {
        let k = roll_ai::catalog::local(name);
        if let Some(k) = k {
            known_tags.insert(k.tag);
        }
        let memory = k.map(|k| k.memory_gb);
        rows.push((
            k.map(|k| k.tier.0),
            LocalModelInfo {
                name: name.clone(),
                installed: true,
                download_gb: sizes.get(name).map(|b| (*b as f64 / 1e9 * 10.0).round() / 10.0).or(k.map(|k| k.download_gb)),
                memory_gb: memory,
                quality: k.map_or("Not rated", |k| k.tier.label()).to_string(),
                basis: k.map(|k| k.basis.to_string()),
                measured: measured.get(name).copied(),
                fits: gpu_gb.zip(memory).map(|(g, m)| m <= g),
                best: false,
            },
        ));
    }
    // Suggestions only where Roll can say how to get them (Ollama).
    if ollama {
        for k in roll_ai::catalog::LOCAL.iter().filter(|k| !known_tags.contains(k.tag)) {
            rows.push((
                Some(k.tier.0),
                LocalModelInfo {
                    name: k.tag.to_string(),
                    installed: false,
                    download_gb: Some(k.download_gb),
                    memory_gb: Some(k.memory_gb),
                    quality: k.tier.label().to_string(),
                    basis: Some(k.basis.to_string()),
                    measured: None,
                    fits: gpu_gb.map(|g| k.memory_gb <= g),
                    best: false,
                },
            ));
        }
    }
    // Best first; models that do not fit the card go after those that do.
    rows.sort_by_key(|(t, m)| (u8::from(m.fits == Some(false)), quality_key(m.measured, *t, 0.0), u8::from(!m.installed)));
    if let Some((_, first)) = rows.first_mut() {
        first.best = true;
    }
    rows.into_iter().map(|(_, m)| m).collect()
}

impl From<roll_ai::openrouter::Offer> for ModelOffer {
    fn from(o: roll_ai::openrouter::Offer) -> Self {
        ModelOffer {
            id: o.id,
            name: o.name,
            provider: o.provider,
            prompt_per_m: o.price.prompt * 1e6,
            completion_per_m: o.price.completion * 1e6,
            image_usd: o.price.image,
            zdr: o.zdr,
            structured: o.structured,
            per_100k_usd: o.per_100k,
            quality: String::new(),
            basis: None,
            measured: None,
            best: false,
            data: if o.zdr {
                "Keeps no data: zero data retention".into()
            } else {
                "May keep requests for a while; Roll only routes to providers that say they don't collect or train on data".into()
            },
            country: None,
            policy_url: None,
        }
    }
}

/// Remember a model's price (and the provider behind it) for estimates.
pub fn save_price(c: &Connection, provider: &str, model: &str, price: &Price, upstream: Option<&str>) -> rusqlite::Result<()> {
    let mut v = serde_json::to_value(price).unwrap_or_default();
    if let Some(u) = upstream {
        v["upstream"] = serde_json::json!(u);
    }
    c.execute(
        "INSERT INTO ai_models (provider, model, price, updated_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (provider, model) DO UPDATE SET price = ?3, updated_at = ?4",
        params![provider, model, v.to_string(), now_ms()],
    )?;
    Ok(())
}

/// A saved price and upstream provider.
pub fn saved_price(c: &Connection, provider: &str, model: &str) -> rusqlite::Result<Option<(Price, Option<String>)>> {
    let v: Option<Option<String>> =
        c.query_row("SELECT price FROM ai_models WHERE provider = ?1 AND model = ?2", params![provider, model], |r| r.get(0)).optional()?;
    Ok(v.flatten().and_then(|s| {
        let v: serde_json::Value = serde_json::from_str(&s).ok()?;
        Some((serde_json::from_value(v.clone()).ok()?, v["upstream"].as_str().map(String::from)))
    }))
}

/// The plan shown before a run, with the consent text for cloud models.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DecidePlan {
    pub provider: String,
    pub host: String,
    pub model: String,
    pub cloud: bool,
    pub items: i32,
    /// Private items left out (cloud models only, unless allowed).
    pub private_skipped: i32,
    /// Already answered by this model and prompt version.
    pub already_done: i32,
    pub estimate_usd: f64,
    pub per_100k_usd: f64,
    pub budget_usd: Option<f64>,
    pub batch: i32,
    /// The estimate uses token counts measured in earlier runs.
    pub measured: bool,
    pub consent: String,
    pub samples: Vec<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RunInfo {
    pub id: i32,
    pub kind: String,
    pub provider: String,
    pub model: String,
    pub state: String,
    pub items: i32,
    pub done: i32,
    pub failed: i32,
    pub unsure: i32,
    pub proposed: i32,
    pub estimate_usd: f64,
    pub budget_usd: Option<f64>,
    pub spent_usd: f64,
    pub seconds: f64,
    pub note: Option<String>,
    pub created_at: f64,
}

/// "3,412".
pub fn thousands(n: i64) -> String {
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// "$0.41", or "$0.0042" for sums under a cent.
pub fn usd(x: f64) -> String {
    if x > 0.0 && x < 0.01 { format!("${x:.4}") } else { format!("${x:.2}") }
}

fn live_items(alias: &str) -> String {
    format!(
        "{alias}.missing = 0 AND {alias}.trashed_at IS NULL AND ({alias}.group_role IS NULL OR {alias}.group_role = 'primary')
         AND {alias}.kind IN (0, 1) AND {alias}.thumb != 2"
    )
}

/// Uncertain items for `p`: undecided, or proposed below [`SETTLED`], not
/// reviewed, not answered by this model and prompt before.
pub fn candidates(c: &Connection, p: &Provider, allow_private_cloud: bool) -> rusqlite::Result<(Vec<i64>, usize, usize)> {
    let base = format!(
        "SELECT a.id, a.private FROM assets a WHERE {} AND (a.review = 0 OR (a.review = 1 AND coalesce(a.category_conf, 0) < ?1))",
        live_items("a")
    );
    let mut st = c.prepare(&format!(
        "{base} AND NOT EXISTS (SELECT 1 FROM ai_items i JOIN ai_runs r ON r.id = i.run_id
                    WHERE i.asset_id = a.id AND i.state = 2 AND r.kind = 'decide' AND r.model = ?2 AND r.prompt_version = ?3)
         ORDER BY coalesce(a.taken_utc, 0), a.id"
    ))?;
    let rows: Vec<(i64, bool)> =
        st.query_map(params![SETTLED, p.model, prompt::PROMPT_VERSION], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    let total: i64 = c.query_row(&format!("SELECT count(*) FROM ({base})"), [SETTLED], |r| r.get(0))?;
    let total = total as usize;
    let keep_private = !p.cloud() || allow_private_cloud;
    let private = rows.iter().filter(|(_, pr)| *pr && !keep_private).count();
    let ids: Vec<i64> = rows.into_iter().filter(|(_, pr)| !*pr || keep_private).map(|(id, _)| id).collect();
    let done = total.saturating_sub(ids.len() + private);
    Ok((ids, private, done))
}

/// Token counts for a model: measured in earlier runs when there are any.
pub fn assumptions(c: &Connection, p: &Provider, batch: usize) -> rusqlite::Result<(Assumptions, bool)> {
    let mut a = Assumptions { batch, ..Default::default() };
    let m: Option<(Option<f64>, Option<f64>)> = c
        .query_row(
            "SELECT input_per_item, output_per_item FROM ai_models WHERE provider = ?1 AND model = ?2",
            params![p.name, p.model],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((Some(input), Some(output))) = m {
        a.image_tokens = (input - a.hint_tokens - a.prompt_tokens / batch.max(1) as f64).max(50.0);
        a.output_tokens = output;
        return Ok((a, true));
    }
    Ok((a, false))
}

pub fn plan(c: &Connection, p: &Provider, batch: usize, budget: Option<f64>, allow_private_cloud: bool) -> rusqlite::Result<DecidePlan> {
    let (ids, private, done) = candidates(c, p, allow_private_cloud)?;
    let (a, measured) = assumptions(c, p, batch)?;
    let estimate = cost::per_item(&p.price, &a) * ids.len() as f64;
    let host = p.endpoint.host().to_string();
    let consent = if p.cloud() {
        let to = p
            .upstream
            .as_deref()
            .map(|u| format!("{u} through OpenRouter"))
            .unwrap_or_else(|| "a provider that keeps no data, through OpenRouter".into());
        format!("{} thumbnails (512 px, no metadata) will be sent to {to}. Estimated cost {}.", thousands(ids.len() as i64), usd(estimate))
    } else {
        format!("{} thumbnails go to {} on this computer ({host}). Nothing leaves the machine.", thousands(ids.len() as i64), p.label)
    };
    Ok(DecidePlan {
        provider: p.label.clone(),
        host,
        model: p.model.clone(),
        cloud: p.cloud(),
        items: ids.len() as i32,
        private_skipped: private as i32,
        already_done: done as i32,
        estimate_usd: estimate,
        per_100k_usd: cost::per_100k(&p.price, &a),
        budget_usd: if p.price.is_free() { None } else { budget },
        batch: batch as i32,
        measured,
        consent,
        samples: ids.iter().take(12).map(|&i| i as i32).collect(),
    })
}

/// Create a run over `ids`.
pub fn create(c: &Connection, kind: &str, p: &Provider, ids: &[i64], batch: usize, budget: Option<f64>) -> rusqlite::Result<i64> {
    let (a, _) = assumptions(c, p, batch)?;
    let estimate = cost::per_item(&p.price, &a) * ids.len() as f64;
    c.execute(
        "INSERT INTO ai_runs (kind, provider, model, prompt_version, batch_size, items, estimate_usd, budget_usd, state, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'ready', ?9)",
        params![
            kind,
            p.name,
            p.model,
            prompt::PROMPT_VERSION,
            batch as i64,
            ids.len() as i64,
            estimate,
            if p.price.is_free() { None } else { budget },
            now_ms()
        ],
    )?;
    let run = c.last_insert_rowid();
    let mut ins = c.prepare("INSERT INTO ai_items (run_id, asset_id) VALUES (?1, ?2)")?;
    for id in ids {
        ins.execute(params![run, id])?;
    }
    Ok(run)
}

/// What a run needs from its surroundings.
pub struct Hooks<'a> {
    /// Progress; false asks the run to stop.
    pub progress: &'a dyn Fn(usize, usize) -> bool,
    /// Measured cost as it is spent.
    pub cost: &'a dyn Fn(f64),
    /// The JPEG and hints for an item.
    pub image: &'a (dyn Fn(i64) -> Option<(Hints, Vec<u8>)> + Sync),
}

struct RunRow {
    kind: String,
    batch: usize,
    budget: Option<f64>,
    spent: f64,
    items: usize,
    note: Option<String>,
}

enum Outcome {
    Answered { reply: Reply, verdicts: Vec<Option<Verdict>> },
    Malformed { reply: Reply, why: String },
    Failed(String),
}

struct Sent {
    ids: Vec<i64>,
    unreadable: Vec<i64>,
    outcome: Outcome,
    throttled: bool,
}

fn categories(c: &Connection) -> rusqlite::Result<(Vec<CategoryDef>, HashMap<String, String>)> {
    let mut st = c.prepare("SELECT key, name, definition FROM categories ORDER BY sort, id")?;
    let rows: Vec<(String, String, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
    let names = rows.iter().map(|(k, n, _)| (k.clone(), n.clone())).collect();
    let defs = rows.into_iter().map(|(key, name, d)| CategoryDef { key, definition: if d.trim().is_empty() { name } else { d } }).collect();
    Ok((defs, names))
}

/// A short pause that grows with each attempt, with jitter so parallel
/// batches do not retry in step.
fn backoff(attempt: u32) -> Duration {
    let base = 500u64 << attempt.min(5);
    let jitter = (now_ms() as u64 ^ (attempt as u64).wrapping_mul(0x9e37_79b9)) % base.max(1);
    Duration::from_millis(base + jitter / 2)
}

fn send(p: &Provider, cats: &[CategoryDef], items: &[Item]) -> (Result<Reply, ChatError>, bool) {
    let mut reasoning = "none";
    let mut throttled = false;
    let mut attempt = 0;
    loop {
        let body =
            prompt::body(&p.model, cats, items, prompt::Options { openrouter: p.cloud(), allow_non_zdr: p.allow_non_zdr, reasoning });
        match p.endpoint.complete(&body) {
            Ok(r) => return (Ok(r), throttled),
            // Models that always reason reject "none".
            Err(ChatError::Status { status: 400, message }) if reasoning == "none" && message.to_lowercase().contains("reason") => {
                reasoning = "minimal";
            }
            Err(e) if e.retryable() && attempt < 4 => {
                throttled |= e.throttled();
                std::thread::sleep(backoff(attempt));
                attempt += 1;
            }
            Err(e) => return (Err(e), throttled),
        }
    }
}

fn run_batch(p: &Provider, cats: &[CategoryDef], ids: &[i64], image: &(dyn Fn(i64) -> Option<(Hints, Vec<u8>)> + Sync)) -> Sent {
    let mut items = Vec::new();
    let mut sent = Vec::new();
    let mut unreadable = Vec::new();
    for &id in ids {
        match image(id) {
            Some((hints, jpeg)) => {
                items.push(Item { hints, jpeg });
                sent.push(id);
            }
            None => unreadable.push(id),
        }
    }
    if items.is_empty() {
        return Sent { ids: sent, unreadable, outcome: Outcome::Failed("no readable items".into()), throttled: false };
    }
    let (res, throttled) = send(p, cats, &items);
    let outcome = match res {
        Ok(reply) => match prompt::parse(&reply.content, items.len(), cats) {
            Ok(verdicts) => Outcome::Answered { reply, verdicts },
            Err(why) => Outcome::Malformed { reply, why },
        },
        Err(e) => Outcome::Failed(e.explain()),
    };
    Sent { ids: sent, unreadable, outcome, throttled }
}

fn load_run(c: &Connection, run: i64) -> rusqlite::Result<RunRow> {
    c.query_row("SELECT kind, batch_size, budget_usd, spent_usd, items, note FROM ai_runs WHERE id = ?1", [run], |r| {
        Ok(RunRow {
            kind: r.get(0)?,
            batch: r.get::<_, i64>(1)?.max(1) as usize,
            budget: r.get(2)?,
            spent: r.get(3)?,
            items: r.get::<_, i64>(4)? as usize,
            note: r.get(5)?,
        })
    })
}

fn set_state(c: &Connection, run: i64, state: &str) -> rusqlite::Result<()> {
    let finished = (state != "running").then(now_ms);
    c.execute("UPDATE ai_runs SET state = ?2, finished_at = ?3 WHERE id = ?1", params![run, state, finished])?;
    Ok(())
}

/// Run (or continue) `run`. Returns the final state.
pub fn execute(db: &Db, run: i64, p: &Provider, hooks: &Hooks) -> anyhow::Result<String> {
    let (mut row, cats, names) = {
        let c = db.write();
        let row = load_run(&c, run)?;
        // Sent but never answered: the app stopped mid-request. They go
        // again; at most one batch per worker can be charged twice.
        c.execute("UPDATE ai_items SET state = 0 WHERE run_id = ?1 AND state = 1", [run])?;
        set_state(&c, run, "running")?;
        let (cats, names) = categories(&c)?;
        (row, cats, names)
    };
    let pending: Vec<i64> = {
        let c = db.read();
        let mut st = c.prepare("SELECT asset_id FROM ai_items WHERE run_id = ?1 AND state = 0 ORDER BY rowid")?;
        st.query_map([run], |r| r.get(0))?.collect::<Result<_, _>>()?
    };
    let (a, _) = assumptions(&db.read(), p, row.batch)?;
    let per_item = cost::per_item(&p.price, &a);
    let reserve = |n: usize| if p.price.is_free() { 0.0 } else { cost::worst_case(&p.price, &a, n, cost::max_tokens(n)) };
    let total = row.items;
    let mut done = total - pending.len();
    let mut conc = 4usize;
    let mut queue: std::collections::VecDeque<Vec<i64>> = pending.chunks(row.batch).map(<[i64]>::to_vec).collect();
    let started = Instant::now();
    let mut state = "done";

    while !queue.is_empty() {
        if !(hooks.progress)(done, total) {
            state = "stopped";
            break;
        }
        // Take as many batches as fit the budget.
        let mut wave = Vec::new();
        let mut reserved = 0.0;
        while wave.len() < conc
            && let Some(b) = queue.front()
        {
            let r = reserve(b.len());
            if row.budget.is_some_and(|cap| row.spent + reserved + r > cap) {
                break;
            }
            reserved += r;
            wave.push(queue.pop_front().unwrap());
        }
        if wave.is_empty() {
            state = "capped";
            break;
        }
        {
            let c = db.write();
            for b in &wave {
                for id in b {
                    c.execute("UPDATE ai_items SET state = 1, at = ?3 WHERE run_id = ?1 AND asset_id = ?2", params![run, id, now_ms()])?;
                }
            }
        }
        let results: Vec<Sent> = std::thread::scope(|s| {
            let handles: Vec<_> = wave.iter().map(|b| s.spawn(|| run_batch(p, &cats, b, hooks.image))).collect();
            handles.into_iter().map(|h| h.join().expect("decision batch panicked")).collect()
        });
        let throttled = results.iter().any(|r| r.throttled);
        let mut fatal: Option<String> = None;
        for mut r in results {
            // A reply that is not the JSON asked for gets one more try, if
            // the budget allows it; then its items count as unsure.
            if let Outcome::Malformed { reply, why } = &r.outcome {
                let paid = reply.cost.unwrap_or(0.0);
                row.spent += paid;
                (hooks.cost)(paid);
                record_spend(db, run, paid)?;
                let fits = row.budget.is_none_or(|cap| row.spent + reserve(r.ids.len()) <= cap);
                let why = why.clone();
                r.outcome = if fits {
                    let again = run_batch(p, &cats, &r.ids, hooks.image);
                    match again.outcome {
                        Outcome::Malformed { reply, .. } => Outcome::Answered { reply, verdicts: vec![None; r.ids.len()] },
                        o => o,
                    }
                } else {
                    tracing::warn!("decision run {run}: malformed reply ({why}), no budget left to retry");
                    Outcome::Answered { reply: Reply::default(), verdicts: vec![None; r.ids.len()] }
                };
            }
            let c = db.write();
            let tx = c.unchecked_transaction()?;
            for id in &r.unreadable {
                tx.execute(
                    "UPDATE ai_items SET state = 3, error = 'The file could not be read.', at = ?3 WHERE run_id = ?1 AND asset_id = ?2",
                    params![run, id, now_ms()],
                )?;
                done += 1;
            }
            match &r.outcome {
                Outcome::Answered { reply, verdicts } => {
                    let paid = reply.cost.unwrap_or(0.0);
                    let n = r.ids.len().max(1) as f64;
                    for (id, v) in r.ids.iter().zip(verdicts) {
                        tx.execute(
                            "UPDATE ai_items SET state = 2, cost = ?3, tokens_in = ?4, tokens_out = ?5, verdict = ?6, error = NULL, at = ?7
                             WHERE run_id = ?1 AND asset_id = ?2",
                            params![
                                run,
                                id,
                                paid / n,
                                (reply.tokens_in as f64 / n).round() as i64,
                                (reply.tokens_out as f64 / n).round() as i64,
                                v.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()),
                                now_ms()
                            ],
                        )?;
                        if row.kind == "decide"
                            && let Some(v) = v
                        {
                            apply(&tx, *id, v, &p.model, &names)?;
                        }
                    }
                    tx.execute("UPDATE ai_runs SET spent_usd = spent_usd + ?2 WHERE id = ?1", params![run, paid])?;
                    row.spent += paid;
                    (hooks.cost)(paid);
                    done += r.ids.len();
                }
                Outcome::Failed(why) => {
                    for id in &r.ids {
                        tx.execute(
                            "UPDATE ai_items SET state = 3, error = ?3, at = ?4 WHERE run_id = ?1 AND asset_id = ?2",
                            params![run, id, why, now_ms()],
                        )?;
                    }
                    done += r.ids.len();
                    fatal.get_or_insert_with(|| why.clone());
                }
                Outcome::Malformed { .. } => unreachable!("retried above"),
            }
            tx.commit()?;
        }
        // Costs running 20% over the estimate get a note, once.
        let answered = done.max(1) as f64;
        if row.note.is_none() && done >= 16 && per_item > 0.0 && row.spent > 1.2 * per_item * answered {
            let over = ((row.spent / (per_item * answered) - 1.0) * 100.0).round();
            let note = format!("Costs run {over}% over the estimate ({} so far).", usd(row.spent));
            db.write().execute("UPDATE ai_runs SET note = ?2 WHERE id = ?1", params![run, note])?;
            row.note = Some(note);
        }
        conc = if throttled { (conc / 2).max(1) } else { (conc + 1).min(8) };
        // A key or credit problem fails every batch alike: stop early.
        if let Some(why) = fatal
            && (why.contains("API key") || why.contains("credit") || why.contains("not available"))
        {
            let c = db.write();
            c.execute("UPDATE ai_runs SET note = ?2 WHERE id = ?1", params![run, why])?;
            state = "failed";
            break;
        }
    }
    (hooks.progress)(done, total);
    let c = db.write();
    c.execute("UPDATE ai_runs SET seconds = seconds + ?2 WHERE id = ?1", params![run, started.elapsed().as_secs_f64()])?;
    set_state(&c, run, state)?;
    learn(&c, run, p)?;
    Ok(state.to_string())
}

fn record_spend(db: &Db, run: i64, usd: f64) -> rusqlite::Result<()> {
    if usd > 0.0 {
        db.write().execute("UPDATE ai_runs SET spent_usd = spent_usd + ?2 WHERE id = ?1", params![run, usd])?;
    }
    Ok(())
}

/// Keep measured token counts per item for the next estimate.
fn learn(c: &Connection, run: i64, p: &Provider) -> rusqlite::Result<()> {
    let (n, input, output): (i64, Option<f64>, Option<f64>) = c.query_row(
        "SELECT count(*), avg(tokens_in), avg(tokens_out) FROM ai_items WHERE run_id = ?1 AND state = 2 AND tokens_in > 0",
        [run],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if n < 8 {
        return Ok(());
    }
    c.execute(
        "INSERT INTO ai_models (provider, model, price, input_per_item, output_per_item, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (provider, model) DO UPDATE SET input_per_item = ?4, output_per_item = ?5, updated_at = ?6",
        params![p.name, p.model, serde_json::to_string(&p.price).unwrap_or_default(), input, output, now_ms()],
    )?;
    Ok(())
}

fn pretty_source(s: &str) -> Option<&'static str> {
    Some(match s {
        "instagram" => "Instagram",
        "tiktok" => "TikTok",
        "snapchat" => "Snapchat",
        "whatsapp" => "WhatsApp",
        "telegram" => "Telegram",
        "reddit" => "Reddit",
        "x" => "X",
        "facebook" => "Facebook",
        "pinterest" => "Pinterest",
        "youtube" => "YouTube",
        _ => return None,
    })
}

/// Turn an answer into a proposal, a caption, a source tag and the
/// Private mark. Owner decisions and certain rules stay as they are.
fn apply(c: &Connection, asset: i64, v: &Verdict, model: &str, names: &HashMap<String, String>) -> rusqlite::Result<()> {
    let (review, conf, dirty): (i64, Option<f64>, bool) =
        c.query_row("SELECT review, category_conf, meta_dirty FROM assets WHERE id = ?1", [asset], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
    if let Some(cat) = &v.category
        && review < 2
        && (review == 0 || conf.unwrap_or(0.0) < v.confidence)
    {
        let name = names.get(cat).cloned().unwrap_or_else(|| cat.clone());
        edits::set(
            c,
            None,
            asset,
            &Change {
                field: "category",
                value: edits::category_value(Some(cat), "model", Some(v.confidence as f32), 1),
                source: "model",
                source_ref: Some(model),
                conf: Some(v.confidence as f32),
                why: Some(format!("{name}, the decision model's answer ({:.0}% sure).", v.confidence * 100.0)),
            },
        )?;
        // A proposal changes nothing in the file until it is accepted.
        if !dirty {
            c.execute("UPDATE assets SET meta_dirty = 0 WHERE id = ?1", [asset])?;
        }
    }
    if let Some(caption) = &v.caption {
        c.execute("UPDATE assets SET caption = ?2 WHERE id = ?1", params![asset, caption])?;
    }
    if v.confidence >= 0.8
        && let Some(src) = v.source.as_deref().and_then(pretty_source)
    {
        crate::tags::add(c, asset, &format!("Source/{src}"), "model", Some(v.confidence as f32))?;
    }
    if v.private {
        crate::privacy::mark(c, asset, "The decision model saw a private document (an ID, a card or similar).", "model")?;
    }
    Ok(())
}

pub fn runs(c: &Connection, limit: i64) -> rusqlite::Result<Vec<RunInfo>> {
    let mut st = c.prepare(
        "SELECT r.id, r.kind, r.provider, r.model, r.state, r.items,
                (SELECT count(*) FROM ai_items i WHERE i.run_id = r.id AND i.state = 2),
                (SELECT count(*) FROM ai_items i WHERE i.run_id = r.id AND i.state = 3),
                (SELECT count(*) FROM ai_items i WHERE i.run_id = r.id AND i.state = 2 AND i.verdict IS NULL),
                (SELECT count(*) FROM ai_items i WHERE i.run_id = r.id AND i.state = 2 AND json_extract(i.verdict, '$.category') IS NOT NULL),
                r.estimate_usd, r.budget_usd, r.spent_usd, r.seconds, r.note, r.created_at
         FROM ai_runs r ORDER BY r.id DESC LIMIT ?1",
    )?;
    st.query_map([limit], |r| {
        Ok(RunInfo {
            id: r.get::<_, i64>(0)? as i32,
            kind: r.get(1)?,
            provider: r.get(2)?,
            model: r.get(3)?,
            state: r.get(4)?,
            items: r.get::<_, i64>(5)? as i32,
            done: r.get::<_, i64>(6)? as i32,
            failed: r.get::<_, i64>(7)? as i32,
            unsure: r.get::<_, i64>(8)? as i32,
            proposed: r.get::<_, i64>(9)? as i32,
            estimate_usd: r.get(10)?,
            budget_usd: r.get(11)?,
            spent_usd: r.get(12)?,
            seconds: r.get(13)?,
            note: r.get(14)?,
            created_at: r.get::<_, i64>(15)? as f64,
        })
    })?
    .collect()
}

// The model check (PROMPT 5.4).

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CheckItem {
    pub id: i32,
    /// The owner's category key, once labelled.
    pub truth: Option<String>,
    /// What Roll proposes now, for a quick confirm.
    pub proposed: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CheckModel {
    pub run: i32,
    pub provider: String,
    pub model: String,
    pub state: String,
    pub batch: i32,
    pub answered: i32,
    pub correct: i32,
    pub unsure: i32,
    pub accuracy: f64,
    pub per_100k_usd: f64,
    pub items_per_sec: f64,
    pub recommended: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CheckReport {
    pub sample: i32,
    pub labeled: i32,
    /// Share of labelled items rules and local models decided, and how
    /// many of those were right.
    pub local_coverage: f64,
    pub local_accuracy: f64,
    pub models: Vec<CheckModel>,
}

/// Pick a stratified sample of about `n` items for the owner to label:
/// every current category (and the undecided) in proportion, at least a
/// few of each. Items the owner already reviewed come labelled.
pub fn check_sample(c: &Connection, n: usize) -> rusqlite::Result<usize> {
    c.execute("DELETE FROM ai_check", [])?;
    let mut st = c.prepare(&format!(
        "SELECT a.id, coalesce(cat.key, ''), a.review FROM assets a LEFT JOIN categories cat ON cat.id = a.category_id
         WHERE {} ORDER BY (a.id * 2654435761) % 1000003",
        live_items("a")
    ))?;
    let rows: Vec<(i64, String, i64)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
    let mut strata: HashMap<String, Vec<(i64, i64)>> = HashMap::new();
    for (id, key, review) in &rows {
        strata.entry(key.clone()).or_default().push((*id, *review));
    }
    let total = rows.len().max(1) as f64;
    let mut ins = c.prepare("INSERT INTO ai_check (asset_id, truth, labeled_at) VALUES (?1, ?2, ?3)")?;
    let mut picked = 0;
    let mut keys: Vec<&String> = strata.keys().collect();
    keys.sort();
    for key in keys {
        let items = &strata[key];
        let want = ((items.len() as f64 / total * n as f64).round() as usize).max(5).min(items.len());
        for (id, review) in items.iter().take(want) {
            let truth = (*review == 3 && !key.is_empty()).then(|| key.clone());
            ins.execute(params![id, truth, truth.as_ref().map(|_| now_ms())])?;
            picked += 1;
        }
    }
    Ok(picked)
}

pub fn check_items(c: &Connection) -> rusqlite::Result<Vec<CheckItem>> {
    let mut st = c.prepare(
        "SELECT k.asset_id, k.truth, cat.key FROM ai_check k JOIN assets a ON a.id = k.asset_id
         LEFT JOIN categories cat ON cat.id = a.category_id ORDER BY k.truth IS NOT NULL, k.rowid",
    )?;
    st.query_map([], |r| Ok(CheckItem { id: r.get::<_, i64>(0)? as i32, truth: r.get(1)?, proposed: r.get(2)? }))?.collect()
}

pub fn check_label(c: &Connection, asset: i64, key: Option<&str>) -> anyhow::Result<()> {
    if let Some(k) = key {
        let ok: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM categories WHERE key = ?1)", [k], |r| r.get(0))?;
        anyhow::ensure!(ok, "Unknown category {k}");
    }
    c.execute("UPDATE ai_check SET truth = ?2, labeled_at = ?3 WHERE asset_id = ?1", params![asset, key, key.map(|_| now_ms())])?;
    Ok(())
}

pub fn labeled(c: &Connection) -> rusqlite::Result<Vec<i64>> {
    let mut st = c.prepare("SELECT asset_id FROM ai_check WHERE truth IS NOT NULL ORDER BY rowid")?;
    st.query_map([], |r| r.get(0))?.collect()
}

pub fn check_report(c: &Connection) -> rusqlite::Result<CheckReport> {
    let (sample, labeled): (i64, i64) = c.query_row("SELECT count(*), count(truth) FROM ai_check", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    // Local stage: rules and file tags, on items the owner labelled here.
    let (decided, right): (i64, i64) = c.query_row(
        "SELECT count(*), coalesce(sum(cat.key = k.truth), 0) FROM ai_check k JOIN assets a ON a.id = k.asset_id
         JOIN categories cat ON cat.id = a.category_id
         WHERE k.truth IS NOT NULL AND a.review IN (1, 2) AND (a.category_source LIKE 'rule%' OR a.category_source = 'file')",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let mut st = c.prepare(
        "SELECT r.id, r.provider, r.model, r.state, r.batch_size, r.spent_usd, r.seconds,
                count(i.asset_id),
                coalesce(sum(json_extract(i.verdict, '$.category') = k.truth), 0),
                coalesce(sum(i.verdict IS NULL), 0)
         FROM ai_runs r JOIN ai_items i ON i.run_id = r.id AND i.state = 2
         JOIN ai_check k ON k.asset_id = i.asset_id AND k.truth IS NOT NULL
         WHERE r.kind = 'check' AND r.id IN (SELECT max(id) FROM ai_runs WHERE kind = 'check' GROUP BY provider, model, batch_size)
         GROUP BY r.id ORDER BY r.id",
    )?;
    let mut models: Vec<CheckModel> = st
        .query_map([], |r| {
            let answered: i64 = r.get(7)?;
            let correct: i64 = r.get(8)?;
            let spent: f64 = r.get(5)?;
            let secs: f64 = r.get(6)?;
            Ok(CheckModel {
                run: r.get::<_, i64>(0)? as i32,
                provider: r.get(1)?,
                model: r.get(2)?,
                state: r.get(3)?,
                batch: r.get::<_, i64>(4)? as i32,
                answered: answered as i32,
                correct: correct as i32,
                unsure: r.get::<_, i64>(9)? as i32,
                accuracy: correct as f64 / answered.max(1) as f64,
                per_100k_usd: spent / answered.max(1) as f64 * 100_000.0,
                items_per_sec: if secs > 0.0 { answered as f64 / secs } else { 0.0 },
                recommended: false,
            })
        })?
        .collect::<Result<_, _>>()?;
    // The cheapest model within 2 points of the best.
    let best = models.iter().map(|m| m.accuracy).fold(0.0, f64::max);
    if let Some(pick) = models
        .iter_mut()
        .filter(|m| m.answered > 0 && m.accuracy >= best - 0.02)
        .min_by(|a, b| a.per_100k_usd.total_cmp(&b.per_100k_usd).then(b.accuracy.total_cmp(&a.accuracy)))
    {
        pick.recommended = true;
    }
    Ok(CheckReport {
        sample: sample as i32,
        labeled: labeled as i32,
        local_coverage: decided as f64 / labeled.max(1) as f64,
        local_accuracy: right as f64 / decided.max(1) as f64,
        models,
    })
}
