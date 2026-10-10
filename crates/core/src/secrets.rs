//! API keys in the OS keychain (Windows Credential Manager, macOS Keychain,
//! Secret Service). They are read only here, in Rust; the UI can store,
//! check and delete them but never read them back.

const SERVICE: &str = "Roll";

/// Known secret names. Anything else is refused.
pub const NAMES: &[&str] = &["openrouter", "immich"];

fn entry(name: &str) -> anyhow::Result<keyring::Entry> {
    if !NAMES.contains(&name) {
        anyhow::bail!("Unknown secret {name}");
    }
    Ok(keyring::Entry::new(SERVICE, name)?)
}

pub fn set(name: &str, value: &str) -> anyhow::Result<()> {
    let value = value.trim();
    if value.is_empty() {
        return delete(name);
    }
    Ok(entry(name)?.set_password(value)?)
}

pub fn get(name: &str) -> Option<String> {
    entry(name).ok()?.get_password().ok()
}

pub fn has(name: &str) -> bool {
    get(name).is_some()
}

pub fn delete(name: &str) -> anyhow::Result<()> {
    match entry(name)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}
