//! Decision runs against a stand-in OpenAI-compatible server that charges
//! like OpenRouter (`usage.cost` from token counts). Checks the M5
//! acceptance points that need no real provider: the estimate against the
//! measured cost on 1,000 items, the budget cap, and resuming without
//! sending an answered item again. No network, no money.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

use base64::Engine;
use parking_lot::Mutex;
use roll_ai::chat::Endpoint;
use roll_ai::cost::Price;
use roll_ai::decide::Hints;
use roll_core::db::Db;
use roll_core::decide::{self, Hooks, Provider};
use rusqlite::params;
use serde_json::{Value, json};

const PRICE: Price = Price { prompt: 0.05e-6, completion: 0.15e-6, image: 0.0, request: 0.0 };
const CHEAP: Price = Price { prompt: 0.01e-6, completion: 0.03e-6, image: 0.0, request: 0.0 };

#[derive(Default)]
struct Server {
    /// How often each item id was sent.
    seen: HashMap<i64, usize>,
    requests: usize,
    charged: f64,
    /// Answer the next N requests with 429.
    throttle: usize,
    /// Answer requests containing this id with prose once.
    garble: Option<i64>,
    bodies_with_paths: usize,
    zdr_missing: usize,
}

fn read_request(r: &mut BufReader<TcpStream>) -> Option<(String, Vec<u8>)> {
    let mut line = String::new();
    if r.read_line(&mut line).ok()? == 0 {
        return None;
    }
    let path = line.split_whitespace().nth(1)?.to_string();
    let mut len = 0;
    loop {
        let mut h = String::new();
        r.read_line(&mut h).ok()?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':')
            && k.eq_ignore_ascii_case("content-length")
        {
            len = v.trim().parse().ok()?;
        }
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body).ok()?;
    Some((path, body))
}

fn respond(w: &mut TcpStream, status: u16, body: &str) {
    let _ = write!(w, "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}", body.len());
}

/// The truth for an item: its id decides the category.
fn truth(id: i64) -> &'static str {
    ["photos", "screenshots", "memes", "documents"][(id % 4) as usize]
}

fn handle(state: &Mutex<Server>, body: &[u8]) -> (u16, String) {
    let req: Value = serde_json::from_slice(body).unwrap();
    let text = String::from_utf8_lossy(body);
    let mut s = state.lock();
    s.requests += 1;
    if text.contains("IMG_") || text.contains("/lib") {
        s.bodies_with_paths += 1;
    }
    if req["provider"]["zdr"] != true {
        s.zdr_missing += 1;
    }
    if s.throttle > 0 {
        s.throttle -= 1;
        return (429, r#"{"error":{"message":"Rate limited"}}"#.into());
    }
    // The stand-in reads item ids from the "JPEG" bytes.
    let ids: Vec<i64> = req["messages"][1]["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["image_url"]["url"].as_str())
        .map(|u| {
            let b = base64::engine::general_purpose::STANDARD.decode(u.trim_start_matches("data:image/jpeg;base64,")).unwrap();
            i64::from_le_bytes(b[..8].try_into().unwrap())
        })
        .collect();
    // Image tokens vary between 256 and 330 per item; 400 prompt tokens.
    let input: u64 = 400 + ids.iter().map(|id| 256 + (id * 37 % 75) as u64 + 30).sum::<u64>();
    let output: u64 = 10 + 33 * ids.len() as u64;
    let price = if req["model"] == "cheap/vl" { CHEAP } else { PRICE };
    let cost = input as f64 * price.prompt + output as f64 * price.completion;
    s.charged += cost;
    let garbled = s.garble.is_some_and(|g| ids.contains(&g));
    if garbled {
        s.garble = None;
    } else {
        for id in &ids {
            *s.seen.entry(*id).or_default() += 1;
        }
    }
    let content = if garbled {
        "Here are the categories you asked for.".to_string()
    } else {
        let items: Vec<Value> = ids
            .iter()
            .enumerate()
            .map(|(n, id)| {
                let cat = if id % 10 == 9 {
                    "unsure"
                } else if req["model"] == "cheap/vl" && id % 3 == 0 {
                    "photos"
                } else {
                    truth(*id)
                };
                json!({"i": n + 1, "category": cat, "confidence": 0.9,
                       "source": if truth(*id) == "memes" { json!("reddit") } else { Value::Null },
                       "caption": format!("item {id}"), "has_text": false, "private": id % 50 == 7})
            })
            .collect();
        json!({ "items": items }).to_string()
    };
    let reply = json!({
        "id": format!("gen-{}", s.requests),
        "model": req["model"],
        "choices": [{"message": {"role": "assistant", "content": content}}],
        "usage": {"prompt_tokens": input, "completion_tokens": output, "cost": cost}
    });
    (200, reply.to_string())
}

/// Starts the stand-in on a free port.
fn serve() -> (String, Arc<Mutex<Server>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/api/v1", listener.local_addr().unwrap());
    let state = Arc::new(Mutex::new(Server::default()));
    let st = state.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let st = st.clone();
            std::thread::spawn(move || {
                let mut w = stream.try_clone().unwrap();
                let mut r = BufReader::new(stream);
                while let Some((path, body)) = read_request(&mut r) {
                    let (status, text) = if path.ends_with("/chat/completions") { handle(&st, &body) } else { (404, "{}".into()) };
                    respond(&mut w, status, &text);
                }
            });
        }
    });
    (base, state)
}

fn library(n: i64) -> (Db, Vec<i64>) {
    let db = Db::memory().unwrap();
    let mut ids = Vec::new();
    {
        let c = db.write();
        c.execute("INSERT INTO roots (id, path, added_at) VALUES (1, '/lib', 0)", []).unwrap();
        for i in 0..n {
            c.execute(
                "INSERT INTO assets (root_id, rel_path, dir, name, ext, size, mtime, quick_hash, kind, format, added_at, width, height, taken_utc)
                 VALUES (1, ?1, 'Dump', ?2, 'jpg', 1, 0, 'q', 0, 'jpeg', 0, 1080, 1350, ?3)",
                params![format!("Dump/IMG_{i}.JPG"), format!("IMG_{i}.JPG"), i],
            )
            .unwrap();
            ids.push(c.last_insert_rowid());
        }
    }
    (db, ids)
}

fn provider(base: &str) -> Provider {
    let mut endpoint = Endpoint::new(base, Some("sk-test".into()));
    endpoint.openrouter = true;
    Provider {
        endpoint,
        name: "openrouter".into(),
        label: "OpenRouter".into(),
        model: "g/gemma".into(),
        price: PRICE,
        upstream: Some("DeepInfra".into()),
        allow_non_zdr: false,
    }
}

fn image(id: i64) -> Option<(Hints, Vec<u8>)> {
    Some((Hints { width: Some(1080), height: Some(1350), ..Default::default() }, id.to_le_bytes().to_vec()))
}

fn spent(db: &Db, run: i64) -> f64 {
    db.read().query_row("SELECT spent_usd FROM ai_runs WHERE id = ?1", [run], |r| r.get(0)).unwrap()
}

#[test]
fn estimate_holds_on_a_thousand_items_and_answers_become_proposals() {
    let (base, server) = serve();
    server.lock().throttle = 2;
    server.lock().garble = Some(5);
    let (db, ids) = library(1000);
    let p = provider(&base);
    let plan = decide::plan(&db.read(), &p, 8, Some(5.0), false).unwrap();
    assert_eq!(plan.items, 1000);
    assert!(
        plan.consent.starts_with("1,000 thumbnails (512 px, no metadata) will be sent to DeepInfra through OpenRouter."),
        "{}",
        plan.consent
    );
    assert!(plan.per_100k_usd < 3.0, "default settings stay under $3 per 100k: {}", plan.per_100k_usd);
    let run = decide::create(&db.write(), "decide", &p, &ids, 8, Some(5.0)).unwrap();
    let state = decide::execute(&db, run, &p, &Hooks { progress: &|_, _| true, cost: &|_| (), image: &image }).unwrap();
    assert_eq!(state, "done");

    let s = server.lock();
    let measured = spent(&db, run);
    assert!((measured - s.charged).abs() < 1e-9, "every charge is counted");
    let ratio = plan.estimate_usd / measured;
    assert!((0.75..=1.25).contains(&ratio), "estimate {} vs measured {measured}", plan.estimate_usd);
    assert!(s.seen.values().all(|&n| n == 1), "no item answered twice");
    assert_eq!(s.seen.len(), 1000);
    assert_eq!((s.bodies_with_paths, s.zdr_missing), (0, 0), "no names or paths, ZDR on every request");
    drop(s);

    let c = db.read();
    let (proposed, model_src): (i64, i64) = c
        .query_row("SELECT count(*), sum(category_source = 'model') FROM assets WHERE review = 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!(proposed, 900, "unsure items stay undecided");
    assert_eq!(model_src, 900);
    let right: i64 = c
        .query_row(
            "SELECT count(*) FROM assets a JOIN categories k ON k.id = a.category_id WHERE a.review = 1 AND
             k.key = CASE a.id % 4 WHEN 0 THEN 'photos' WHEN 1 THEN 'screenshots' WHEN 2 THEN 'memes' ELSE 'documents' END",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(right, 900);
    let caption: String = c.query_row("SELECT caption FROM assets WHERE id = ?1", [ids[3]], |r| r.get(0)).unwrap();
    assert_eq!(caption, format!("item {}", ids[3]));
    let private: i64 = c.query_row("SELECT count(*) FROM assets WHERE private = 1", [], |r| r.get(0)).unwrap();
    assert_eq!(private, 20);
    let reddit: i64 = c
        .query_row("SELECT count(*) FROM asset_tags at JOIN tags t ON t.id = at.tag_id WHERE t.path = 'Source/Reddit'", [], |r| r.get(0))
        .unwrap();
    assert!(reddit > 200);
    // Measured token counts feed the next estimate.
    let input: f64 = c.query_row("SELECT input_per_item FROM ai_models WHERE model = 'g/gemma'", [], |r| r.get(0)).unwrap();
    assert!((300.0..400.0).contains(&input), "{input}");
    drop(c);
    // Nothing is left to plan for this model and prompt.
    assert_eq!(decide::plan(&db.read(), &p, 8, Some(5.0), false).unwrap().items, 0);
}

#[test]
fn the_cap_stops_the_run_exactly() {
    let (base, server) = serve();
    let (db, ids) = library(1000);
    let p = provider(&base);
    let cap = 0.005;
    let run = decide::create(&db.write(), "decide", &p, &ids, 8, Some(cap)).unwrap();
    let state = decide::execute(&db, run, &p, &Hooks { progress: &|_, _| true, cost: &|_| (), image: &image }).unwrap();
    assert_eq!(state, "capped");
    let measured = spent(&db, run);
    assert!(measured <= cap, "{measured} over the cap");
    // It used most of the budget: within one batch's worst case.
    let worst = roll_ai::cost::worst_case(&PRICE, &Default::default(), 8, roll_ai::cost::max_tokens(8));
    assert!(measured > cap - 8.0 * worst, "{measured} stopped early");
    let pending: i64 = db.read().query_row("SELECT count(*) FROM ai_items WHERE run_id = ?1 AND state = 0", [run], |r| r.get(0)).unwrap();
    assert!(pending > 0);
    assert!((server.lock().charged - measured).abs() < 1e-9);
}

#[test]
fn a_run_resumes_without_paying_twice() {
    let (base, server) = serve();
    let (db, ids) = library(200);
    let p = provider(&base);
    let run = decide::create(&db.write(), "decide", &p, &ids, 8, Some(5.0)).unwrap();
    // Stop after the first wave, as when the owner presses Stop.
    let calls = Mutex::new(0);
    let stop = |_: usize, _: usize| {
        let mut n = calls.lock();
        *n += 1;
        *n < 2
    };
    let state = decide::execute(&db, run, &p, &Hooks { progress: &stop, cost: &|_| (), image: &image }).unwrap();
    assert_eq!(state, "stopped");
    let first = server.lock().seen.len();
    assert!(first > 0 && first < 200);

    // A crash with one batch in flight: its items are still marked sent.
    {
        let c = db.write();
        c.execute(
            "UPDATE ai_items SET state = 1 WHERE run_id = ?1 AND asset_id IN (SELECT asset_id FROM ai_items WHERE run_id = ?1 AND state = 0 LIMIT 8)",
            [run],
        )
        .unwrap();
        c.execute("UPDATE ai_runs SET state = 'running' WHERE id = ?1", [run]).unwrap();
    }
    let state = decide::execute(&db, run, &p, &Hooks { progress: &|_, _| true, cost: &|_| (), image: &image }).unwrap();
    assert_eq!(state, "done");
    let s = server.lock();
    assert_eq!(s.seen.len(), 200);
    assert!(s.seen.values().all(|&n| n == 1), "answered items are never sent again");
    assert!((spent(&db, run) - s.charged).abs() < 1e-9);
    let info = &decide::runs(&db.read(), 5).unwrap()[0];
    assert_eq!((info.done, info.items, info.state.as_str()), (200, 200, "done"));
}

#[test]
fn the_model_check_recommends_the_cheapest_good_model() {
    let (base, _server) = serve();
    let (db, _) = library(400);
    {
        let c = db.write();
        // Some items the owner already sorted: they come labelled.
        c.execute(
            "UPDATE assets SET review = 3, category_id = (SELECT id FROM categories WHERE key = 'photos') WHERE id % 4 = 0 AND id < 100",
            [],
        )
        .unwrap();
    }
    let n = decide::check_sample(&db.write(), 150).unwrap();
    assert!((100..=200).contains(&n), "{n}");
    let items = decide::check_items(&db.read()).unwrap();
    let pre = items.iter().filter(|i| i.truth.is_some()).count();
    assert!(pre > 0, "reviewed items come labelled");
    for it in items.iter().filter(|i| i.truth.is_none()) {
        decide::check_label(&db.write(), it.id as i64, Some(truth(it.id as i64))).unwrap();
    }
    assert!(decide::check_label(&db.write(), items[0].id as i64, Some("nonsense")).is_err());
    let ids = decide::labeled(&db.read()).unwrap();
    let good = provider(&base);
    let mut cheap = provider(&base);
    cheap.model = "cheap/vl".into();
    cheap.price = CHEAP;
    for p in [&good, &cheap] {
        let run = decide::create(&db.write(), "check", p, &ids, 8, Some(1.0)).unwrap();
        decide::execute(&db, run, p, &Hooks { progress: &|_, _| true, cost: &|_| (), image: &image }).unwrap();
    }
    // A check run proposes nothing.
    let proposed: i64 = db.read().query_row("SELECT count(*) FROM assets WHERE category_source = 'model'", [], |r| r.get(0)).unwrap();
    assert_eq!(proposed, 0);
    let r = decide::check_report(&db.read()).unwrap();
    assert_eq!(r.labeled as usize, n);
    assert_eq!(r.models.len(), 2);
    let g = r.models.iter().find(|m| m.model == "g/gemma").unwrap();
    let c = r.models.iter().find(|m| m.model == "cheap/vl").unwrap();
    assert!(g.accuracy > c.accuracy + 0.02, "{} vs {}", g.accuracy, c.accuracy);
    assert!(c.per_100k_usd < g.per_100k_usd);
    assert!(g.recommended && !c.recommended, "the cheaper model is more than 2 points worse");
}

#[test]
fn model_lists_put_the_highest_quality_first() {
    use roll_ai::openrouter::Offer;
    let offer = |id: &str, per_100k: f64, zdr: bool| Offer {
        id: id.into(),
        name: id.into(),
        provider: Some("DeepInfra".into()),
        price: PRICE,
        zdr,
        structured: true,
        context_length: None,
        per_100k,
    };
    let offers = vec![
        offer("inclusionai/ling-3.0-flash-vl", 1.0, true),
        offer("google/gemma-3-12b-it", 2.2, true),
        offer("google/gemini-2.5-flash", 30.0, true),
        offer("google/gemini-2.5-flash-lite", 4.8, false),
    ];
    let mut providers = std::collections::HashMap::new();
    providers.insert(
        "deepinfra".to_string(),
        roll_ai::openrouter::ProviderInfo {
            name: "DeepInfra".into(),
            headquarters: Some("US".into()),
            privacy_policy_url: Some("https://deepinfra.com/privacy".into()),
        },
    );
    let ranked = decide::rank_offers(offers.clone(), &providers, &Default::default());
    let ids: Vec<&str> = ranked.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["google/gemini-2.5-flash", "google/gemini-2.5-flash-lite", "google/gemma-3-12b-it", "inclusionai/ling-3.0-flash-vl"]);
    assert!(ranked[0].best && !ranked[1].best);
    assert_eq!((ranked[0].quality.as_str(), ranked[3].quality.as_str()), ("Best", "Not rated"));
    assert_eq!(ranked[0].country.as_deref(), Some("US"));
    assert!(ranked[1].data.starts_with("May keep requests"));
    assert!(ranked[0].data.starts_with("Keeps no data"));
    // A model checked on the owner's items outranks any tier.
    let mut measured = std::collections::HashMap::new();
    measured.insert("google/gemma-3-12b-it".to_string(), 0.97);
    assert_eq!(decide::rank_offers(offers, &providers, &measured)[0].id, "google/gemma-3-12b-it");

    // Local: installed and suggested, best that fits an 11 GB card first.
    let installed = vec!["qwen3-vl:4b-instruct".to_string(), "qwen3-vl:8b".to_string(), "llava:7b".to_string()];
    let local = decide::rank_local(&installed, &Default::default(), Some(11.0), &Default::default(), true);
    assert_eq!(local[0].name, "qwen3-vl:8b-instruct", "the instruct tag, not the installed thinking one");
    assert!(local[0].best && !local[0].installed && local[0].fits == Some(true));
    assert_eq!(local[0].download_gb, Some(6.1));
    let big = local.iter().position(|m| m.name == "gemma4:26b").unwrap();
    let small = local.iter().position(|m| m.name == "qwen3-vl:4b-instruct").unwrap();
    let thinking = local.iter().find(|m| m.name == "qwen3-vl:8b").unwrap();
    assert!(thinking.installed && thinking.basis.as_deref().unwrap().starts_with("Thinking variant"));
    assert_eq!(thinking.quality, "Good");
    assert!(big > small, "models that do not fit go last");
    assert_eq!(local.iter().find(|m| m.name == "llava:7b").unwrap().quality, "Not rated");
}
