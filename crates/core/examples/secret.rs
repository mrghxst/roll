//! Stores an API key in the OS keychain the way Roll does, read from
//! standard input so it never lands in a file or the shell history.
//!
//!   cargo run -p roll-core --example secret -- openrouter < key.txt
//!   cargo run -p roll-core --example secret -- openrouter --check

use std::io::Read;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [name, rest @ ..] = args.as_slice() else {
        anyhow::bail!("usage: secret <{}> [--check]", roll_core::secrets::NAMES.join("|"));
    };
    if rest.iter().any(|a| a == "--check") {
        println!("{name}: {}", if roll_core::secrets::has(name) { "stored" } else { "not stored" });
        return Ok(());
    }
    let mut value = String::new();
    std::io::stdin().read_to_string(&mut value)?;
    roll_core::secrets::set(name, &value)?;
    println!("{name}: stored ({} characters)", value.trim().len());
    Ok(())
}
