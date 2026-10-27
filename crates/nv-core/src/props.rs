use crate::{Vault, dberr};
use nv_fs::{NodeId, Result};
use rusqlite::{Connection, OptionalExtension, params};

pub const FILES_SCHEMA_KEY: &str = "nv.files_schema";
pub const FILES_SCHEMA_VERSION: u32 = 1;

const MIGRATIONS: [&str; 1] = ["
CREATE TABLE IF NOT EXISTS node_meta (
  node INTEGER PRIMARY KEY REFERENCES nodes(id) ON DELETE CASCADE,
  created INTEGER,
  imported INTEGER,
  origin TEXT,
  sha256 BLOB,
  sha256_size INTEGER,
  sha256_mtime INTEGER
);
CREATE TRIGGER IF NOT EXISTS node_meta_created AFTER INSERT ON nodes
BEGIN
  INSERT OR IGNORE INTO node_meta(node, created) VALUES (NEW.id, NEW.mtime);
END;
-- a brand-new volume (only its root so far): the root was made just now
INSERT OR IGNORE INTO node_meta(node, created) SELECT id, mtime FROM nodes WHERE id = 1 AND (SELECT COUNT(*) FROM nodes) = 1;
"];

pub(crate) fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    use rusqlite::types::Value;
    let raw: Option<Value> = conn.query_row("SELECT v FROM meta WHERE k = ?", [FILES_SCHEMA_KEY], |r| r.get(0)).optional()?;
    let parse = |b: &[u8]| std::str::from_utf8(b).ok().and_then(|s| s.trim().parse::<u32>().ok());
    let have: u32 = match raw {
        None => 0,
        Some(v) => match match &v {
            Value::Blob(b) => parse(b),
            Value::Text(s) => parse(s.as_bytes()),
            Value::Integer(n) => u32::try_from(*n).ok(),
            _ => None,
        } {
            Some(n) => n,
            None => return Ok(()),
        },
    };
    if have >= FILES_SCHEMA_VERSION {
        return Ok(());
    }
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let r = (|| {
        for m in &MIGRATIONS[have as usize..] {
            conn.execute_batch(m)?;
        }
        conn.execute("INSERT OR REPLACE INTO meta(k, v) VALUES (?, ?)", params![FILES_SCHEMA_KEY, FILES_SCHEMA_VERSION.to_string().into_bytes()])?;
        Ok(())
    })();
    match r {
        Ok(()) => conn.execute_batch("COMMIT"),
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NodeMeta {
    pub created: Option<i64>,
    pub imported: Option<i64>,
    pub origin: Option<String>,
    pub sha256: Option<[u8; 32]>,
}

fn missing_table(e: &rusqlite::Error) -> bool {
    e.to_string().contains("no such table")
}

impl Vault {
    pub fn node_meta(&self, id: NodeId) -> Result<NodeMeta> {
        let n = nv_fs::Fs::getattr(self, id)?;
        self.with(|i| {
            let row = i
                .conn
                .query_row(
                    "SELECT created, imported, origin, sha256, sha256_size, sha256_mtime FROM node_meta WHERE node = ?",
                    [id as i64],
                    |r| {
                        Ok((
                            r.get::<_, Option<i64>>(0)?,
                            r.get::<_, Option<i64>>(1)?,
                            r.get::<_, Option<String>>(2)?,
                            r.get::<_, Option<Vec<u8>>>(3)?,
                            r.get::<_, Option<i64>>(4)?,
                            r.get::<_, Option<i64>>(5)?,
                        ))
                    },
                )
                .optional();
            let row = match row {
                Ok(r) => r,
                Err(e) if missing_table(&e) => None,
                Err(e) => return Err(dberr(e)),
            };
            let Some((created, imported, origin, sha, size, mtime)) = row else { return Ok(NodeMeta::default()) };
            let sha256 = sha
                .and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok())
                .filter(|_| size == Some(n.size as i64) && mtime == Some(n.mtime));
            Ok(NodeMeta { created, imported, origin, sha256 })
        })
    }

    pub fn set_import_meta(&self, id: NodeId, created: Option<i64>, imported: i64, origin: &str) -> Result<()> {
        self.with(|i| {
            i.op(|c| {
                c.execute(
                    "INSERT INTO node_meta(node, created, imported, origin) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(node) DO UPDATE SET created = COALESCE(?2, created), imported = ?3, origin = ?4",
                    params![id as i64, created, imported, origin],
                )
                .map_err(dberr)?;
                Ok(())
            })
        })
    }

    pub fn set_sha256(&self, id: NodeId, hash: &[u8; 32], size: u64, mtime: i64) -> Result<()> {
        self.with(|i| {
            i.op(|c| {
                c.execute(
                    "INSERT INTO node_meta(node, sha256, sha256_size, sha256_mtime) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(node) DO UPDATE SET sha256 = ?2, sha256_size = ?3, sha256_mtime = ?4",
                    params![id as i64, &hash[..], size as i64, mtime],
                )
                .map_err(dberr)?;
                Ok(())
            })?;
            i.durable()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KdfPreset;
    use nv_fs::{Fs, Kind, ROOT};
    use secrecy::SecretString;

    fn pw() -> SecretString {
        SecretString::from("pw".to_owned())
    }

    #[test]
    fn creation_import_and_hash() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("v.nvault");
        let v = Vault::create(&p, &pw(), KdfPreset::Fast).unwrap();
        assert!(v.node_meta(ROOT).unwrap().created.is_some(), "a new volume's root was made now");
        let f = v.create(ROOT, "a.txt", Kind::File).unwrap();
        let m = v.node_meta(f.id).unwrap();
        assert_eq!(m.created, Some(f.mtime), "the trigger records every new node");
        assert_eq!((m.imported, m.origin.as_deref(), m.sha256), (None, None, None));

        v.set_import_meta(f.id, Some(42), 100, "/home/u/a.txt").unwrap();
        v.set_import_meta(f.id, None, 101, "/home/u/a.txt").unwrap();
        v.write(f.id, 0, b"hello").unwrap();
        v.flush().unwrap();
        let n = v.getattr(f.id).unwrap();
        v.set_sha256(f.id, &[7; 32], n.size, n.mtime).unwrap();
        let m = v.node_meta(f.id).unwrap();
        assert_eq!((m.created, m.imported, m.origin.as_deref(), m.sha256), (Some(42), Some(101), Some("/home/u/a.txt"), Some([7; 32])));

        v.write(f.id, 5, b"!").unwrap();
        v.flush().unwrap();
        assert_eq!(v.node_meta(f.id).unwrap().sha256, None);

        v.remove(f.id).unwrap();
        v.flush().unwrap();
        let g = v.create(ROOT, "b.txt", Kind::File).unwrap();
        let m = v.node_meta(g.id).unwrap();
        assert_eq!((m.imported, m.origin), (None, None));
        v.close().unwrap();

        let v = Vault::open(&p, &pw()).unwrap();
        assert_eq!(v.meta_get(FILES_SCHEMA_KEY).unwrap().as_deref(), Some(&b"1"[..]));
        assert!(v.node_meta(g.id).unwrap().created.is_some(), "persisted");
    }

    #[test]
    fn volume_made_before_the_table_migrates_and_reads_unknown() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("old.nvault");
        let v = Vault::create(&p, &pw(), KdfPreset::Fast).unwrap();
        let f = v.create(ROOT, "old.txt", Kind::File).unwrap();
        v.with(|i| {
            i.conn
                .execute_batch(&format!("DROP TRIGGER node_meta_created; DROP TABLE node_meta; DELETE FROM meta WHERE k = '{FILES_SCHEMA_KEY}';"))
                .map_err(dberr)
        })
        .unwrap();
        assert_eq!(v.node_meta(f.id).unwrap(), NodeMeta::default(), "no table: unknown, not an error");
        v.close().unwrap();

        let v = Vault::open(&p, &pw()).unwrap();
        assert_eq!(v.node_meta(f.id).unwrap(), NodeMeta::default(), "made before: unknown");
        assert_eq!(v.node_meta(ROOT).unwrap().created, None, "an old root is not dated by the migration");
        let g = v.create(ROOT, "new.txt", Kind::File).unwrap();
        assert!(v.node_meta(g.id).unwrap().created.is_some());
    }

    #[test]
    fn newer_schema_is_left_alone() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("newer.nvault");
        let v = Vault::create(&p, &pw(), KdfPreset::Fast).unwrap();
        v.with(|i| {
            i.conn
                .execute_batch(&format!("DROP TRIGGER node_meta_created; DROP TABLE node_meta; UPDATE meta SET v = CAST('9' AS BLOB) WHERE k = '{FILES_SCHEMA_KEY}';"))
                .map_err(dberr)
        })
        .unwrap();
        v.close().unwrap();
        let v = Vault::open(&p, &pw()).unwrap();
        assert_eq!(v.meta_get(FILES_SCHEMA_KEY).unwrap().as_deref(), Some(&b"9"[..]));
        let f = v.create(ROOT, "x", Kind::File).unwrap();
        assert_eq!(v.node_meta(f.id).unwrap(), NodeMeta::default(), "no table created by this build");
        assert!(v.set_import_meta(f.id, None, 1, "/x").is_err());
    }
}
