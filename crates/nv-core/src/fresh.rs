//! Freshness records kept in the encrypted, authenticated `meta` table.
//! Both are additive `meta` keys: builds before them ignore them, and a volume they never wrote
//! simply has no record (nothing is refused for that).
//!
//! - `nv.appearance` ties the plaintext appearance section to the header it was written with
//!   The appearance has its own MAC, but that MAC binds only the volume id, so an older
//!   authentic appearance (or none) spliced into the current header passed as authentic.
//! - `nv.commit_seq` counts durability points. The database alone cannot tell that it was
//!   replaced by an older copy of itself; a counter remembered OUTSIDE the file can
//!   ([`crate::Vault::commit_seq`], the app's per-volume anchor).

use crate::header::Header;
use rusqlite::{Connection, OptionalExtension, params};

const APPEARANCE: &str = "nv.appearance";
const COMMIT_SEQ: &str = "nv.commit_seq";

/// `seq (u64 LE) || BLAKE3(postcard(Option<Appearance>))` of `h`.
fn appearance_record(h: &Header) -> Vec<u8> {
    let a = postcard::to_allocvec(&h.appearance.as_ref().map(|a| &a.appearance)).unwrap_or_default();
    let mut v = h.body.seq.to_le_bytes().to_vec();
    v.extend_from_slice(blake3::hash(&a).as_bytes());
    v
}

fn recorded(conn: &Connection) -> rusqlite::Result<Option<(u64, Vec<u8>)>> {
    let v: Option<Vec<u8>> = conn.query_row("SELECT v FROM meta WHERE k = ?", [APPEARANCE], |r| r.get(0)).optional()?;
    Ok(v.filter(|v| v.len() == 40).map(|v| (u64::from_le_bytes(v[..8].try_into().unwrap()), v)))
}

/// After a header write (or at open, when the record is missing or stale): the database learns
/// which appearance goes with header `h`. `schema` = "main", or an attached copy being built.
pub(crate) fn record_appearance(conn: &Connection, schema: &str, h: &Header) -> rusqlite::Result<()> {
    conn.execute(&format!("INSERT OR REPLACE INTO {schema}.meta(k, v) VALUES (?, ?)"), params![APPEARANCE, appearance_record(h)]).map(|_| ())
}

/// Does `h` carry an appearance other than the one recorded for its header? Only a header this
/// build recorded can be judged: when `h` is newer than the record, some other build (an older
/// one) wrote the header since, and its appearance is taken as is. A splice cannot fake that:
/// `seq` is covered by the header MAC.
pub(crate) fn appearance_replayed(conn: &Connection, h: &Header) -> rusqlite::Result<bool> {
    Ok(recorded(conn)?.is_some_and(|(at, rec)| h.body.seq <= at && rec[8..] != appearance_record(h)[8..]))
}

/// Is the record exactly `h`'s (nothing to write)?
pub(crate) fn appearance_current(conn: &Connection, h: &Header) -> rusqlite::Result<bool> {
    Ok(recorded(conn)?.is_some_and(|(_, rec)| rec == appearance_record(h)))
}

/// Inside the transaction about to be committed: one more durability point.
pub(crate) fn bump_commit(conn: &Connection) -> rusqlite::Result<()> {
    conn.prepare_cached("INSERT INTO meta(k, v) VALUES (?, 1) ON CONFLICT(k) DO UPDATE SET v = CAST(v AS INTEGER) + 1")?
        .execute([COMMIT_SEQ])
        .map(|_| ())
}

/// Durability points recorded so far (0 = none since the counter exists).
pub(crate) fn commit_seq(conn: &Connection) -> rusqlite::Result<u64> {
    let v: Option<i64> = conn.prepare_cached("SELECT CAST(v AS INTEGER) FROM meta WHERE k = ?")?.query_row([COMMIT_SEQ], |r| r.get(0)).optional()?;
    Ok(v.unwrap_or(0).max(0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Appearance;
    use crate::header::{HeaderBody, MAGIC, SignedAppearance, VERSION};

    fn header(seq: u64, accent: Option<&str>) -> Header {
        Header {
            body: HeaderBody { magic: MAGIC, version: VERSION, volume_id: [7; 16], seq, slots: vec![] },
            header_mac: [0; 32],
            appearance: accent.map(|a| SignedAppearance { appearance: Appearance { accent: Some(a.into()), emblem: None }, mac: [0; 32] }),
            layout: None,
            fixed: None,
            journal: None,
        }
    }

    #[test]
    fn appearance_record_rules() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE meta (k TEXT PRIMARY KEY, v BLOB);").unwrap();
        let h = header(4, Some("prussia"));
        // no record (a volume from before it, or only ever written by older builds): never flagged
        assert!(!appearance_replayed(&c, &header(4, Some("other"))).unwrap());
        record_appearance(&c, "main", &h).unwrap();
        assert!(appearance_current(&c, &h).unwrap());
        assert!(!appearance_replayed(&c, &h).unwrap());
        // the other copy of the same pair (seq - 1) carries the same appearance
        assert!(!appearance_replayed(&c, &header(3, Some("prussia"))).unwrap());
        // spliced: another appearance, or none, under a header this build recorded
        assert!(appearance_replayed(&c, &header(4, Some("bottiglia"))).unwrap());
        assert!(appearance_replayed(&c, &header(4, None)).unwrap());
        // a newer header: written since by a build that does not keep the record; taken as is
        assert!(!appearance_replayed(&c, &header(6, Some("bottiglia"))).unwrap());
        assert!(!appearance_current(&c, &header(6, Some("bottiglia"))).unwrap());
    }

    #[test]
    fn commit_counter_counts() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE meta (k TEXT PRIMARY KEY, v BLOB);").unwrap();
        assert_eq!(commit_seq(&c).unwrap(), 0);
        for n in 1..=3 {
            bump_commit(&c).unwrap();
            assert_eq!(commit_seq(&c).unwrap(), n);
        }
    }
}
