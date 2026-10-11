//! Build Roll's `places.tsv` from GeoNames files, as the app does when the
//! owner installs place names.
//!
//! `cargo run -p roll-geo --example build_places -- <geonames dir> <places.tsv>`

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().expect("folder with the GeoNames files"));
    let out = PathBuf::from(args.next().expect("output file"));
    let open = |n: &str| -> anyhow::Result<BufReader<File>> { Ok(BufReader::new(File::open(dir.join(n))?)) };
    let g = roll_geo::gazetteer::build(open("cities500.txt")?, open("admin1CodesASCII.txt")?, open("countryInfo.txt")?)?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    g.save(&out)?;
    println!("{} places written to {}", g.len(), out.display());
    Ok(())
}
