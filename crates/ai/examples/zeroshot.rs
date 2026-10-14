//! Zero-shot sorting of the CC0 set with SigLIP 2: accuracy and speed.
//!   cargo run --release -p roll-ai --example zeroshot -- fixtures-out/packs/models/siglip2-b16 fixtures-out/cc0 [cpu]
use std::time::Instant;

use roll_ai::siglip::{ImageEncoder, TextEncoder};
use roll_ai::vectors::dot;

const CATS: &[(&str, &[&str])] = &[
    (
        "photos",
        &[
            "a photo",
            "a photo of people",
            "a photo of a dog",
            "a photo of a cat",
            "a photo of food on a plate",
            "a landscape photo",
            "a photo of a city street",
            "a photo of the beach",
            "a photo of mountains",
            "a close-up photo of a flower",
            "an old photograph",
        ],
    ),
    (
        "screenshots",
        &[
            "a screenshot of a phone screen",
            "a screenshot of an app",
            "a screenshot of a chat conversation",
            "a screenshot of a website",
            "a screenshot of settings",
        ],
    ),
    ("memes", &["a meme with caption text", "an internet meme", "a funny image with white text on top and bottom"]),
    ("social_media", &["a social media post", "an instagram post with likes and comments", "a saved post from a social app"]),
    (
        "wallpapers",
        &[
            "a wallpaper",
            "an abstract background",
            "a desktop wallpaper",
            "a space nebula",
            "a seamless texture pattern",
            "a gradient background",
        ],
    ),
    ("documents", &["a photo of a document", "a scanned paper document", "a receipt", "a page of printed text"]),
    (
        "junk",
        &[
            "an accidental photo of the floor",
            "a completely black photo",
            "an extremely blurry photo with nothing in it",
            "a photo taken inside a pocket",
            "a photo of a finger covering the lens",
        ],
    ),
];

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let dir = std::path::PathBuf::from(&args[1]);
    let set = std::path::PathBuf::from(&args[2]);
    let gpu = args.get(3).map(|s| s != "cpu").unwrap_or(true);
    let t = Instant::now();
    let img = ImageEncoder::load(&dir, gpu)?;
    let txt = TextEncoder::load(&dir)?;
    println!("loaded in {:.1}s, gpu {}", t.elapsed().as_secs_f32(), img.on_gpu());
    let mode = std::env::var("ZS_MODE").unwrap_or_else(|_| "max".into());
    let protos: Vec<(&str, Vec<Vec<f32>>)> = CATS.iter().map(|(k, ps)| (*k, ps.iter().map(|p| txt.embed(p).unwrap()).collect())).collect();
    let score = |e: &[f32], ps: &[Vec<f32>]| -> f32 {
        if mode == "max" {
            ps.iter().map(|p| dot(e, p)).fold(f32::MIN, f32::max)
        } else {
            ps.iter().map(|p| dot(e, p)).sum::<f32>() / ps.len() as f32
        }
    };
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(set.join("manifest.json"))?)?;
    let items = manifest["items"].as_array().unwrap();
    let (mut right, mut n) = (0, 0);
    let mut per: std::collections::BTreeMap<&str, (u32, u32)> = Default::default();
    let mut margins = Vec::new();
    let t = Instant::now();
    for it in items {
        let truth = it["category"].as_str().unwrap();
        let im = image::open(set.join(it["file"].as_str().unwrap()))?;
        let e = img.embed(&im)?;
        let mut s: Vec<(&str, f32)> = protos.iter().map(|(k, p)| (*k, score(&e, p))).collect();
        s.sort_by(|a, b| b.1.total_cmp(&a.1));
        let ok = s[0].0 == truth;
        margins.push((s[0].1 - s[1].1, ok));
        n += 1;
        right += ok as u32;
        let p = per.entry(truth).or_default();
        p.1 += 1;
        p.0 += ok as u32;
        if !ok {
            println!("  {truth} -> {} ({:.3} vs {:.3})  {}", s[0].0, s[0].1, s[1].1, it["file"]);
        }
    }
    let el = t.elapsed().as_secs_f32();
    for (k, (r, t)) in &per {
        println!("{k:<14} {r} of {t}");
    }
    println!("accuracy {:.1}% ({right} of {n}), {:.1} items/s", right as f32 * 100.0 / n as f32, n as f32 / el);
    // Auto-accept curve by margin.
    margins.sort_by(|a, b| b.0.total_cmp(&a.0));
    for frac in [0.3, 0.4, 0.5, 0.6, 0.7, 0.8] {
        let k = (margins.len() as f32 * frac) as usize;
        let ok = margins[..k].iter().filter(|m| m.1).count();
        println!("top {:.0}% by margin: precision {:.1}% (margin >= {:.4})", frac * 100.0, ok as f32 * 100.0 / k as f32, margins[k - 1].0);
    }
    Ok(())
}
