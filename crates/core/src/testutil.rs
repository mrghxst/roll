use rusqlite::{params, Connection};

/// Insert a minimal asset row in root 1 (created if needed).
pub fn insert_asset(c: &Connection, rel: &str) -> i64 {
    c.execute("INSERT OR IGNORE INTO roots (id, path, added_at) VALUES (1, '/lib', 0)", []).unwrap();
    let name = rel.rsplit('/').next().unwrap();
    let dir = rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    c.execute(
        "INSERT INTO assets (root_id, rel_path, dir, name, ext, size, mtime, quick_hash, kind, format, added_at)
         VALUES (1, ?1, ?2, ?3, 'jpg', 1, 0, 'q', 0, 'jpeg', 0)",
        params![rel, dir, name],
    )
    .unwrap();
    c.last_insert_rowid()
}
