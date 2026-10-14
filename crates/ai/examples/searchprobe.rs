//! How text-to-image similarity spreads for a few searches on the CC0 set.
use roll_ai::siglip::{ImageEncoder, TextEncoder};
use roll_ai::vectors::dot;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let dir = std::path::PathBuf::from(&args[1]);
    let set = std::path::PathBuf::from(&args[2]);
    let img = ImageEncoder::load(&dir, true)?;
    let txt = TextEncoder::load(&dir)?;
    let m: serde_json::Value = serde_json::from_slice(&std::fs::read(set.join("manifest.json"))?)?;
    let items: Vec<(String, String, Vec<f32>)> = m["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|it| {
            let f = it["file"].as_str().unwrap().to_string();
            let title = it["source"]["title"].as_str().unwrap_or("").chars().take(40).collect();
            let e = img.embed(&image::open(set.join(&f)).unwrap()).unwrap();
            (f, title, e)
        })
        .collect();
    for q in ["dog", "cat", "beach", "mountains", "food", "receipt", "chat", "nebula", "city at night", "flower", "hund am strand", "spam"]
    {
        let t = txt.embed(q)?;
        let mut s: Vec<(f32, &str, &str)> = items.iter().map(|(f, ti, e)| (dot(&t, e), f.as_str(), ti.as_str())).collect();
        s.sort_by(|a, b| b.0.total_cmp(&a.0));
        println!("== {q}: p50 {:.3} p90 {:.3}", s[s.len() / 2].0, s[s.len() / 10].0);
        for (sc, f, ti) in s.iter().take(8) {
            println!("   {sc:.3} {f} {ti}");
        }
    }
    Ok(())
}
