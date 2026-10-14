//! Prepares a data folder with a library folder, for trying the app on a
//! test library: cargo run -p roll-core --example addroot -- <data dir> <folder>
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let data = std::path::PathBuf::from(args.next().expect("data dir"));
    let root = std::path::PathBuf::from(args.next().expect("folder"));
    let lib = roll_core::library::Library::open(&data, std::sync::Arc::new(|_| {}))?;
    lib.add_root(&root)?;
    let mut s = lib.settings();
    s.first_run_done = true;
    s.guide_done = true;
    lib.save_settings(&s)?;
    println!("{} ready with {}", data.display(), root.display());
    Ok(())
}
