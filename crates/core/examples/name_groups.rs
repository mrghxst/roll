//! Names the Discover groups of a library with the decision model.
//!   cargo run -p roll-core --example name_groups -- <data dir> <model>
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let data = std::path::PathBuf::from(args.next().expect("data dir"));
    let model = args.next().expect("model");
    let lib = roll_core::library::Library::open(&data, std::sync::Arc::new(|_| {}))?;
    let mut s = lib.settings();
    s.ai_mode = "local".into();
    s.ai_decision_model = Some(model);
    lib.save_settings(&s)?;
    for c in lib.discover(None)?.iter().take(5) {
        let t = std::time::Instant::now();
        let ids: Vec<i64> = c.samples.iter().map(|i| *i as i64).collect();
        let n = lib.name_cluster(&ids)?;
        println!(
            "{} ({} items): {:?} — {:?} existing {:?} [{:.1}s]",
            c.label,
            c.count,
            n.name,
            n.definition,
            n.existing,
            t.elapsed().as_secs_f32()
        );
    }
    Ok(())
}
