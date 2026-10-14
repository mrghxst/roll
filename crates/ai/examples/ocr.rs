//! Reads the text in images: cargo run --release -p roll-ai --example ocr -- <pack dir> <image>...
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = std::path::PathBuf::from(args.next().unwrap());
    let r = roll_ai::ocr::Reader::load(&dir, std::env::var("OCR_CPU").is_err())?;
    for f in args {
        let img = image::open(&f)?;
        let t = std::time::Instant::now();
        let read = r.read(&img)?;
        println!("== {f} ({} lines, area {:.2}, {} ms)", read.lines.len(), read.area, t.elapsed().as_millis());
        for l in &read.lines {
            println!("   {:.2} {}", l.conf, l.text);
        }
    }
    Ok(())
}
