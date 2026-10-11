//! Place names install from the files the place-names pack downloads
//! (`cities500.zip` plus two text files), and pack downloads stay off until
//! the owner turns downloads on.
//!
//! Uses the mini gazetteer in `fixtures-out/geonames`; skipped without it.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use roll_core::library::Library;
use roll_core::packs::PackState;

#[test]
fn places_from_the_pack_files() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures-out/geonames");
    let Ok(cities) = std::fs::read(src.join("cities500.txt")) else {
        eprintln!("fixtures-out not found; skipping");
        return;
    };
    let data = tempfile::tempdir().unwrap();
    let lib = Library::open(data.path(), Arc::new(|_| {})).unwrap();

    // Off by default: nothing is fetched.
    let err = lib.download_pack("places").unwrap_err().to_string();
    assert!(err.contains("Downloads are off"), "{err}");
    assert!(lib.packs().iter().all(|p| p.state == PackState::Available));

    let dir = tempfile::tempdir().unwrap();
    let mut zip = zip::ZipWriter::new(std::fs::File::create(dir.path().join("cities500.zip")).unwrap());
    zip.start_file("cities500.txt", zip::write::SimpleFileOptions::default()).unwrap();
    zip.write_all(&cities).unwrap();
    zip.finish().unwrap();
    for f in ["admin1CodesASCII.txt", "countryInfo.txt"] {
        std::fs::copy(src.join(f), dir.path().join(f)).unwrap();
    }
    let info = lib.install_places(dir.path()).unwrap();
    assert!(info.installed && info.count > 10, "{info:?}");
    assert!(data.path().join("geodata/places.tsv").exists());
    assert!(lib.place_search("Zurich").iter().any(|h| h.name.contains("Zürich") || h.name.contains("Zurich")));

    // Files that are not GeoNames are refused.
    let bad = tempfile::tempdir().unwrap();
    for f in ["cities500.txt", "admin1CodesASCII.txt", "countryInfo.txt"] {
        std::fs::write(bad.path().join(f), "nothing here\n").unwrap();
    }
    assert!(lib.install_places(bad.path()).is_err());
    assert_eq!(lib.places_info().count, info.count);
}
