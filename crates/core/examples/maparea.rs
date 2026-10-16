//! Downloads an offline map area into a folder, for development and the
//! mock backend's `?maps=1` (it reads `fixtures-out/maps`).
//!
//!   cargo run --release -p roll-core --example maparea -- fixtures-out/maps garda "Lake Garda" 10.5 45.5 11.0 46.0

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dir, id, name, w, s, e, n] = args.as_slice() else {
        anyhow::bail!("usage: maparea <dir> <id> <name> <west> <south> <east> <north>");
    };
    let bbox = [w.parse()?, s.parse()?, e.parse()?, n.parse()?];
    let dir = std::path::Path::new(dir);
    roll_core::offline::ensure_fonts(dir)?;
    let req = roll_core::offline::Request { name: name.clone(), bbox };
    let bytes = roll_core::offline::download(dir, id, &req, &|done, total| {
        if done == total || done % 50 == 0 {
            println!("{done} of {total}");
        }
        true
    })?;
    println!("{name}: {:.1} MB", bytes as f64 / 1e6);
    Ok(())
}
