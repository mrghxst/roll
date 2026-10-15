//! Finds faces in pictures and prints how alike they are.
//! `cargo run --release -p roll-ai --example faces -- <models/faces dir> <images…>`
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("the faces pack folder");
    let faces = roll_ai::faces::Faces::load(dir.as_ref(), true)?;
    let mut all: Vec<(String, Vec<f32>)> = Vec::new();
    for path in args {
        let img = image::open(&path)?;
        let img = img.resize(1024, 1024, image::imageops::FilterType::Triangle).to_rgb8();
        let t = Instant::now();
        let found = faces.detect(&img)?;
        let det = t.elapsed();
        let name = std::path::Path::new(&path).file_name().unwrap().to_string_lossy().into_owned();
        println!("{name}: {} faces in {det:.0?} ({}×{})", found.len(), img.width(), img.height());
        for (i, f) in found.iter().enumerate() {
            let sharp = roll_ai::faces::sharpness(&img, f);
            let t = Instant::now();
            let v = (f.rect[2] >= roll_ai::faces::MIN_RECOGNIZE).then(|| faces.embed(&img, f)).transpose()?;
            println!(
                "  {i}: {:.0},{:.0} {:.0}×{:.0} score {:.2} sharp {sharp:.0} {}",
                f.rect[0],
                f.rect[1],
                f.rect[2],
                f.rect[3],
                f.score,
                if v.is_some() { format!("vector in {:.0?}", t.elapsed()) } else { "too small".into() }
            );
            if let Some(v) = v {
                all.push((format!("{name}#{i}"), v));
            }
        }
    }
    println!("pairs at 0.363 or more:");
    for i in 0..all.len() {
        for j in i + 1..all.len() {
            let s = roll_ai::faces::cosine(&all[i].1, &all[j].1);
            if s >= 0.363 {
                println!("  {} ~ {} {s:.2}", all[i].0, all[j].0);
            }
        }
    }
    Ok(())
}
