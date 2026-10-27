use crate::header::Header;
use rusqlite::{Connection, OptionalExtension, params};

const APPEARANCE: &str = "nv.appearance";
const COMMIT_SEQ: &str = "nv.commit_seq";

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

pub(crate) fn record_appearance(conn: &Connection, schema: &str, h: &Header) -> rusqlite::Result<()> {
    conn.execute(&format!("INSERT OR REPLACE INTO {schema}.meta(k, v) VALUES (?, ?)"), params![APPEARANCE, appearance_record(h)]).map(|_| ())
}

pub(crate) fn appearance_replayed(conn: &Connection, h: &Header) -> rusqlite::Result<bool> {
    Ok(recorded(conn)?.is_some_and(|(at, rec)| h.body.seq <= at && rec[8..] != appearance_record(h)[8..]))
}

pub(crate) fn appearance_current(conn: &Connection, h: &Header) -> rusqlite::Result<bool> {
    Ok(recorded(conn)?.is_some_and(|(_, rec)| rec == appearance_record(h)))
}

pub(crate) fn bump_commit(conn: &Connection) -> rusqlite::Result<()> {
    conn.prepare_cached("INSERT INTO meta(k, v) VALUES (?, 1) ON CONFLICT(k) DO UPDATE SET v = CAST(v AS INTEGER) + 1")?
        .execute([COMMIT_SEQ])
        .map(|_| ())
}

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
        assert!(!appearance_replayed(&c, &header(4, Some("other"))).unwrap());
        record_appearance(&c, "main", &h).unwrap();
        assert!(appearance_current(&c, &h).unwrap());
        assert!(!appearance_replayed(&c, &h).unwrap());
        assert!(!appearance_replayed(&c, &header(3, Some("prussia"))).unwrap());
        assert!(appearance_replayed(&c, &header(4, Some("bottiglia"))).unwrap());
        assert!(appearance_replayed(&c, &header(4, None)).unwrap());
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
