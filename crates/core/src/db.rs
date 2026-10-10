//! SQLite access: one writer and a few readers over the same WAL database,
//! with numbered migrations.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::{Mutex, MutexGuard};
use rusqlite::{Connection, OpenFlags};

use crate::schema::MIGRATIONS;

pub type DbResult<T> = rusqlite::Result<T>;

pub struct Db {
    path: PathBuf,
    writer: Mutex<Connection>,
    readers: Vec<Mutex<Connection>>,
    next: AtomicUsize,
}

fn configure(c: &Connection) -> DbResult<()> {
    c.busy_timeout(std::time::Duration::from_secs(10))?;
    c.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA temp_store = MEMORY;
         PRAGMA cache_size = -32000;
         PRAGMA mmap_size = 268435456;",
    )
}

impl Db {
    /// Open the library database.
    pub fn open(path: &Path) -> DbResult<Self> {
        Self::open_with(path, MIGRATIONS)
    }

    pub fn open_with(path: &Path, migrations: &[&str]) -> DbResult<Self> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let writer = Connection::open(path)?;
        configure(&writer)?;
        migrate(&writer, migrations)?;
        let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 6);
        let mut readers = Vec::with_capacity(n);
        for _ in 0..n {
            let r = Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI,
            )?;
            r.busy_timeout(std::time::Duration::from_secs(10))?;
            r.execute_batch("PRAGMA temp_store = MEMORY; PRAGMA cache_size = -16000; PRAGMA mmap_size = 268435456;")?;
            readers.push(Mutex::new(r));
        }
        Ok(Self { path: path.to_path_buf(), writer: Mutex::new(writer), readers, next: AtomicUsize::new(0) })
    }

    /// An in-memory database for tests (readers share the writer).
    pub fn memory() -> DbResult<Self> {
        Self::memory_with(MIGRATIONS)
    }

    pub fn memory_with(migrations: &[&str]) -> DbResult<Self> {
        let writer = Connection::open_in_memory()?;
        writer.execute_batch("PRAGMA foreign_keys = ON;")?;
        migrate(&writer, migrations)?;
        Ok(Self { path: PathBuf::new(), writer: Mutex::new(writer), readers: Vec::new(), next: AtomicUsize::new(0) })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write(&self) -> MutexGuard<'_, Connection> {
        self.writer.lock()
    }

    /// A read connection. Falls back to the writer for in-memory databases.
    pub fn read(&self) -> MutexGuard<'_, Connection> {
        if self.readers.is_empty() {
            return self.writer.lock();
        }
        let start = self.next.fetch_add(1, Ordering::Relaxed);
        for i in 0..self.readers.len() {
            if let Some(g) = self.readers[(start + i) % self.readers.len()].try_lock() {
                return g;
            }
        }
        self.readers[start % self.readers.len()].lock()
    }

    /// Run `f` in a write transaction.
    pub fn tx<T>(&self, f: impl FnOnce(&rusqlite::Transaction) -> DbResult<T>) -> DbResult<T> {
        let mut c = self.write();
        let tx = c.transaction()?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }
}

fn migrate(c: &Connection, migrations: &[&str]) -> DbResult<()> {
    let version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in migrations.iter().enumerate() {
        let v = i as i64 + 1;
        if v > version {
            c.execute_batch(&format!("BEGIN; {sql}; PRAGMA user_version = {v}; COMMIT;"))?;
        }
    }
    Ok(())
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_file_db_and_reads() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("library.db")).unwrap();
        let v: i64 = db.read().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v as usize, MIGRATIONS.len());
        let n: i64 = db.read().query_row("SELECT count(*) FROM categories", [], |r| r.get(0)).unwrap();
        assert!(n >= 8);
        // Reopen: migrations are idempotent.
        drop(db);
        Db::open(&dir.path().join("library.db")).unwrap();
    }
}
