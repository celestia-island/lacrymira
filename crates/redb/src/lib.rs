//! lacrymira-redb: redb KV driver for lacrymira.
//!
//! Runs natively and inside WASI components (redb is a pure-Rust embedded
//! KV store with no network dependency). Implements the `Command::Kv` arm.

use lacrymira_core::{Command, Error, KvOp, Reply, Value};
use redb::ReadableDatabase;

/// A redb-backed KV store implementing the lacrymira command surface.
pub struct RedbStore {
    db: redb::Database,
}

// Table definition: keys and values are msgpack-serialized bytes.
const TABLE: redb::TableDefinition<&[u8], &[u8]> = redb::TableDefinition::new("lacrymira_kv");

impl RedbStore {
    /// Open (or create) a database at the given path.
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self, Error> {
        let db = redb::Database::create(path).map_err(|e| Error::Config(e.to_string()))?;
        // Ensure the table exists
        let txn = db.begin_write().map_err(|e| Error::Config(e.to_string()))?;
        let _ = txn
            .open_table(TABLE)
            .map_err(|e| Error::Config(e.to_string()))?;
        txn.commit().map_err(|e| Error::Config(e.to_string()))?;
        Ok(Self { db })
    }

    /// Open an in-memory database (for tests).
    pub fn open_in_memory() -> Result<Self, Error> {
        // redb doesn't have a true in-memory mode; use a temp file.
        let dir = std::env::temp_dir().join(format!("lacrymora-{}", uuid::Uuid::new_v4()));
        Self::open(&dir)
    }

    /// Execute a `Command::Kv` against this store.
    pub fn execute_kv(&self, namespace: &str, op: &KvOp) -> Result<Reply, Error> {
        match op {
            KvOp::Get { key } => self.get(namespace, key),
            KvOp::Set { key, value } => self.set(namespace, key, value),
            KvOp::Delete { key } => self.delete(namespace, key),
            KvOp::Scan { start, end, limit } => {
                self.scan(namespace, start.as_ref(), end.as_ref(), *limit)
            }
            KvOp::Count { start, end } => self.count(namespace, start.as_ref(), end.as_ref()),
        }
    }

    /// Execute any command (non-Kv commands return an error).
    pub fn execute(&self, cmd: &Command) -> Result<Reply, Error> {
        match cmd {
            Command::Kv { namespace, op } => self.execute_kv(namespace, op),
            _ => Err(Error::Config(format!(
                "lacrymira-redb only supports Command::Kv, got: {:?}",
                std::mem::discriminant(cmd)
            ))),
        }
    }

    fn ns_key(namespace: &str, key: &Value) -> Vec<u8> {
        // Prefix the namespace to the serialized key for namespacing
        let mut buf = namespace.as_bytes().to_vec();
        buf.push(0xFF); // namespace separator (not valid UTF-8 continuation)
        buf.extend_from_slice(&rmp_serde::to_vec(key).unwrap_or_default());
        buf
    }

    fn get(&self, ns: &str, key: &Value) -> Result<Reply, Error> {
        let txn = self.db.begin_read().map_err(map_redb)?;
        let table = txn.open_table(TABLE).map_err(map_redb)?;
        let nk = Self::ns_key(ns, key);
        match table.get(nk.as_slice()).map_err(map_redb)? {
            Some(v) => {
                let value: Value =
                    rmp_serde::from_slice(v.value()).map_err(|e| Error::Decode(e.to_string()))?;
                Ok(Reply::Value(Some(value)))
            }
            None => Ok(Reply::Value(None)),
        }
    }

    fn set(&self, ns: &str, key: &Value, value: &Value) -> Result<Reply, Error> {
        let txn = self.db.begin_write().map_err(map_redb)?;
        {
            let mut table = txn.open_table(TABLE).map_err(map_redb)?;
            let nk = Self::ns_key(ns, key);
            let serialized = rmp_serde::to_vec(value).map_err(|e| Error::Decode(e.to_string()))?;
            table
                .insert(nk.as_slice(), serialized.as_slice())
                .map_err(map_redb)?;
        }
        txn.commit().map_err(map_redb)?;
        Ok(Reply::Unit)
    }

    fn delete(&self, ns: &str, key: &Value) -> Result<Reply, Error> {
        let txn = self.db.begin_write().map_err(map_redb)?;
        let removed;
        {
            let mut table = txn.open_table(TABLE).map_err(map_redb)?;
            let nk = Self::ns_key(ns, key);
            removed = table.remove(nk.as_slice()).map_err(map_redb)?.is_some();
        }
        txn.commit().map_err(map_redb)?;
        Ok(Reply::Affected(if removed { 1 } else { 0 }))
    }

    fn scan(
        &self,
        ns: &str,
        start: Option<&Value>,
        end: Option<&Value>,
        limit: Option<u64>,
    ) -> Result<Reply, Error> {
        let txn = self.db.begin_read().map_err(map_redb)?;
        let table = txn.open_table(TABLE).map_err(map_redb)?;

        let start_key = start.map(|v| Self::ns_key(ns, v));
        let end_key = end.map(|v| Self::ns_key(ns, v));

        let mut entries = Vec::new();
        let max = limit.unwrap_or(u64::MAX);

        // Use redb's range scan
        let range = match (&start_key, &end_key) {
            (Some(s), Some(e)) => table.range(s.as_slice()..e.as_slice()),
            (Some(s), None) => table.range(s.as_slice()..),
            (None, Some(e)) => table.range(..e.as_slice()),
            (None, None) => table.range::<&[u8]>(..),
        };

        for (count, pair) in range.map_err(map_redb)?.enumerate() {
            let (k, v) = pair.map_err(map_redb)?;
            if count as u64 >= max {
                break;
            }
            // Strip namespace prefix from key
            let raw_key: Value = rmp_serde::from_slice(&k.value()[ns.len() + 1..])
                .map_err(|e| Error::Decode(e.to_string()))?;
            let value: Value =
                rmp_serde::from_slice(v.value()).map_err(|e| Error::Decode(e.to_string()))?;
            entries.push((raw_key, value));
        }

        Ok(Reply::Entries(entries))
    }

    fn count(&self, ns: &str, start: Option<&Value>, end: Option<&Value>) -> Result<Reply, Error> {
        let reply = self.scan(ns, start, end, None)?;
        match reply {
            Reply::Entries(entries) => Ok(Reply::Affected(entries.len() as u64)),
            _ => unreachable!(),
        }
    }
}

fn map_redb<E: std::fmt::Display>(e: E) -> Error {
    Error::Db {
        kind: "redb".into(),
        message: e.to_string(),
        sqlstate: None,
    }
}
