//! Prints how closed the eyes of every face are, and with `--draw <dir>`
//! saves each picture with the mesh's eye points drawn on it.
//! `cargo run --release -p roll-ai --example eyes -- <models/faces> <models/eyes> [--draw dir] <images…>`

fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let faces = roll_ai::faces::Faces::load(args.remove(0).as_ref(), true)?;
    let eyes = roll_ai::eyes::Eyes::load(args.remove(0).as_ref(), true)?;
    let draw = args.iter().position(|a| a == "--draw").map(|i| {
        args.remove(i);
        std::path::PathBuf::from(args.remove(i))
    });
    for path in args {
        let Ok(img) = image::open(&path) else { continue };
        let mut img = img.resize(1024, 1024, image::imageops::FilterType::Triangle).to_rgb8();
        let name = std::path::Path::new(&path).file_name().unwrap().to_string_lossy().into_owned();
        let found = faces.detect(&img)?;
        let mut line = format!("{name}:");
        let mut dots = Vec::new();
        for f in found.iter().filter(|f| f.rect[2] >= 24.0) {
            let s = eyes.state(&img, f)?;
            line += &format!(
                " [{:.0}px blink {:.2} {:.2} p {:.2}{}]",
                f.rect[2],
                s.blink[0],
                s.blink[1],
                s.presence,
                if s.closed() { " CLOSED" } else { "" }
            );
            if let Some(dir) = &draw {
                let n = dots.len() / 10;
                roll_ai::eyes::face_crop(&img, f).save(dir.join(format!("face-{}-{n}.png", name.trim_end_matches(".jpg"))))?;
                let (pts, _) = eyes.landmarks(&img, f)?;
                dots.extend([33, 133, 159, 145, 263, 362, 386, 374, 1, 152].map(|i| pts[i]));
            }
        }
        println!("{line}");
        if let Some(dir) = &draw {
            let (w, h) = img.dimensions();
            for [x, y] in dots {
                for d in -2i32..=2 {
                    for (px, py) in [(x as i32 + d, y as i32), (x as i32, y as i32 + d)] {
                        if px >= 0 && py >= 0 && (px as u32) < w && (py as u32) < h {
                            img.put_pixel(px as u32, py as u32, image::Rgb([255, 0, 0]));
                        }
                    }
                }
            }
            img.save(dir.join(&name))?;
        }
    }
    Ok(())
}
