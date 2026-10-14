//! Downloads packs with Roll's pack manager into a folder, for development
//! and the model tests (they look in `fixtures-out/packs` or `ROLL_PACKS`).
//!
//!   cargo run -p roll-core --example packs -- fixtures-out/packs image-model text-reader places

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use roll_core::packs::{Catalog, PackState, Packs};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().unwrap_or_else(|| "fixtures-out/packs".into()));
    let ids: Vec<String> = args.collect();
    let packs = Packs::new(root.clone(), Catalog::bundled(), Arc::new(|| {}));
    for id in &ids {
        packs.download(id, None)?;
    }
    loop {
        std::thread::sleep(Duration::from_secs(5));
        let mut busy = false;
        for id in &ids {
            let s = packs.status(id).unwrap();
            println!("{id}: {:?} {} of {}{}", s.state, s.done, s.size, s.error.map(|e| format!(" ({e})")).unwrap_or_default());
            busy |= matches!(s.state, PackState::Downloading | PackState::Installing);
        }
        if !busy {
            break;
        }
    }
    let failed: Vec<_> = ids.iter().filter(|id| packs.installed(id).is_none()).collect();
    anyhow::ensure!(failed.is_empty(), "not installed: {failed:?}");
    println!("installed in {}", root.display());
    Ok(())
}
