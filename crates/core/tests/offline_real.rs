//! Offline map areas against the real Protomaps build (needs the internet;
//! downloads about 10 MB): central Lisbon from zoom 0 to 15, then served.
//!
//! ```text
//! ROLL_REAL_MAPS=1 cargo test --release -p roll-core --test offline_real -- --ignored --nocapture
//! ```

use std::time::Instant;

use roll_core::offline::{self, Request};

#[test]
#[ignore = "downloads map tiles from build.protomaps.com"]
fn copies_an_area_and_serves_it() {
    if std::env::var_os("ROLL_REAL_MAPS").is_none() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let bbox = std::env::var("ROLL_REAL_MAPS_BBOX")
        .ok()
        .and_then(|s| serde_json::from_str::<[f64; 4]>(&s).ok())
        .unwrap_or([-9.25, 38.68, -9.08, 38.78]);
    let req = Request { name: "Lisbon".into(), bbox };
    let est = offline::estimate(bbox);
    let t = Instant::now();
    offline::ensure_fonts(dir).unwrap();
    let bytes = offline::download(dir, "lisbon", &req, &|_, _| true).unwrap();
    let areas = offline::list(dir, &[], &Default::default());
    let a = &areas[0];
    println!(
        "{} tiles (estimate {}), {:.1} MB of tiles, {:.1} MB on disk, {:.0} bytes a tile, build {}, {:.1} s",
        a.tiles,
        est.tiles,
        bytes as f64 / 1e6,
        a.bytes / 1e6,
        a.bytes / f64::from(a.tiles.max(1)),
        a.build,
        t.elapsed().as_secs_f32()
    );
    assert_eq!(a.state, "ready");
    assert!(f64::from(a.tiles) >= f64::from(est.tiles) * 0.8, "most tiles of the area exist");
    let tiles = offline::Tiles::load(dir);
    let world = tiles.get(0, 0, 0).expect("the world tile");
    // A vector tile starts with a layer (field 3, length-delimited).
    assert_eq!(world[0], 0x1a);
    let centre = [(bbox[0] + bbox[2]) / 2.0, (bbox[1] + bbox[3]) / 2.0];
    let ((x, _), (y, _)) = roll_geo::pmtiles::tile_range([centre[0], centre[1], centre[0], centre[1]], 13);
    let city = tiles.get(13, x, y).expect("a tile in the middle of the area");
    assert!(city.len() > 1000, "{} bytes", city.len());
    // Every building is at zoom 15: much more than its zoom 13 parent has.
    let ((x15, _), (y15, _)) = roll_geo::pmtiles::tile_range([centre[0], centre[1], centre[0], centre[1]], 15);
    let street = tiles.get(15, x15, y15).expect("a zoom 15 tile");
    let enlarged = roll_geo::mvt::overzoom(&city, 2, x15 & 3, y15 & 3).unwrap();
    let buildings = |t: &[u8]| roll_geo::mvt::layers(t).unwrap().into_iter().find(|(n, _)| n == "buildings").map_or(0, |(_, n)| n);
    let (real, coarse) = (buildings(&street), buildings(&enlarged));
    println!("zoom 15 tile: {real} buildings, {} bytes; zoom 13 enlarged: {coarse} buildings, {} bytes", street.len(), enlarged.len());
    assert!(real > 10 * coarse.max(1), "{real} against {coarse}");
    let c = rusqlite::Connection::open(dir.join("lisbon.db")).unwrap();
    let mut q = c.prepare("SELECT z, count(*), sum(length(data)) FROM tiles GROUP BY z HAVING z >= 12").unwrap();
    for row in q.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))).unwrap() {
        let (z, n, b) = row.unwrap();
        println!("  zoom {z}: {n} tiles, {:.0} KB a tile", b as f64 / n as f64 / 1e3);
    }
    assert!(offline::glyphs(dir, "Noto Sans Regular", "0-255").is_some_and(|g| g.len() > 10_000));
    assert!(offline::glyphs(dir, "Noto Sans Regular", "../x").is_none());
}
