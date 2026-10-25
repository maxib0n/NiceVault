//! Malleability: an attacker WITHOUT the password edits bytes of the file.
//! The property under test: an edit is either harmless (same content comes back) or noticed
//! (open / read fails). Never a different content returned as if nothing happened.

use crate::common::*;
use nv_core::{Appearance, LockedEmblem, OpenError, Vault};
use nv_fs::Fs;

fn flipped(src: &std::path::Path, dir: &std::path::Path, off: usize, mask: u8) -> std::path::PathBuf {
    let p = dir.join(format!("flip-{off}.nvault"));
    let mut raw = std::fs::read(src).unwrap();
    raw[off] ^= mask;
    std::fs::write(&p, raw).unwrap();
    p
}

/// One bit flip at every 4 KiB of the file (both header copies, every database page, start,
/// middle and end of each page) and at every byte of the first header copy's used part.
#[test]
fn byte_flip_anywhere_is_harmless_or_detected() {
    let t = tempfile::tempdir().unwrap();
    let (p, _, _) = sample(t.path(), "v.nvault");
    let want = open_dump(&p, "pw").unwrap();
    let len = std::fs::metadata(&p).unwrap().len() as usize;
    let used = postcard::to_allocvec(&read_copy(&p, 0)).unwrap().len();
    let mut offs: Vec<usize> = (0..used).collect();
    for page in (DB_OFFSET..len).step_by(PAGE) {
        offs.extend([page, page + 1, page + 15, page + 16, page + PAGE / 2, page + PAGE - 49, page + PAGE - 1]);
    }
    offs.extend((0..len).step_by(4096));
    offs.retain(|&o| o < len);
    let mut silent_changes = vec![];
    for (n, off) in offs.into_iter().enumerate() {
        let q = flipped(&p, t.path(), off, 1 << (n % 8));
        match open_dump(&q, "pw") {
            Ok(got) if got != want => silent_changes.push(off),
            _ => {}
        }
        std::fs::remove_file(&q).unwrap();
        let _ = std::fs::remove_file(t.path().join(format!("flip-{off}.nvault-journal")));
    }
    assert!(silent_changes.is_empty(), "content changed without an error after flips at {silent_changes:?}");
}

/// SQLCipher's per-page HMAC covers the page number: swapping two pages of one volume fails.
#[test]
fn page_swap_within_a_volume_is_detected() {
    let t = tempfile::tempdir().unwrap();
    let (p, _, _) = sample(t.path(), "v.nvault");
    let raw = std::fs::read(&p).unwrap();
    let pages = (raw.len() - DB_OFFSET) / PAGE;
    assert!(pages >= 4, "{pages} pages");
    let want = open_dump(&p, "pw").unwrap();
    for (i, j) in [(1, 2), (2, 3), (0, 1), (1, pages - 1)] {
        let mut m = raw.clone();
        let (a, b) = (DB_OFFSET + i * PAGE, DB_OFFSET + j * PAGE);
        let pa = raw[a..a + PAGE].to_vec();
        m[a..a + PAGE].copy_from_slice(&raw[b..b + PAGE]);
        m[b..b + PAGE].copy_from_slice(&pa);
        let q = t.path().join("swap.nvault");
        std::fs::write(&q, m).unwrap();
        let r = open_dump(&q, "pw");
        assert!(r.as_ref().is_err() || r.as_ref().unwrap() == &want, "swap {i}<->{j} went unnoticed with different content");
        std::fs::remove_file(&q).unwrap();
    }
}

/// Two volumes with the SAME password have independent master keys: a page from one never
/// authenticates in the other (same page number, same password, still an HMAC failure).
#[test]
fn cross_volume_page_splice_is_detected() {
    let t = tempfile::tempdir().unwrap();
    let (a, _, _) = sample(t.path(), "a.nvault");
    let (b, _, _) = sample(t.path(), "b.nvault");
    let (ra, rb) = (std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
    let want = open_dump(&a, "pw").unwrap();
    let pages = (ra.len().min(rb.len()) - DB_OFFSET) / PAGE;
    for i in 0..pages {
        let mut m = ra.clone();
        let off = DB_OFFSET + i * PAGE;
        m[off..off + PAGE].copy_from_slice(&rb[off..off + PAGE]);
        let q = t.path().join("splice.nvault");
        std::fs::write(&q, m).unwrap();
        let r = open_dump(&q, "pw");
        assert!(r.is_err() || r.unwrap() == want, "page {i} from another volume accepted");
        std::fs::remove_file(&q).unwrap();
    }
    // Whole database of B under A's header: refused (key and volume_id do not match).
    let mut m = ra.clone();
    m.truncate(DB_OFFSET);
    m.extend_from_slice(&rb[DB_OFFSET..]);
    let q = t.path().join("swapdb.nvault");
    std::fs::write(&q, m).unwrap();
    assert!(Vault::open(&q, &pw("pw")).is_err());
}

/// Truncating the database (dropping trailing pages) must not yield shorter / zeroed files.
#[test]
fn truncation_is_detected() {
    let t = tempfile::tempdir().unwrap();
    let (p, _, _) = sample(t.path(), "v.nvault");
    let raw = std::fs::read(&p).unwrap();
    let want = open_dump(&p, "pw").unwrap();
    let pages = (raw.len() - DB_OFFSET) / PAGE;
    for cut in 1..pages {
        let q = t.path().join("trunc.nvault");
        std::fs::write(&q, &raw[..raw.len() - cut * PAGE]).unwrap();
        let r = open_dump(&q, "pw");
        assert!(r.is_err() || r.unwrap() == want, "truncated by {cut} pages: different content, no error");
        std::fs::remove_file(&q).unwrap();
        let _ = std::fs::remove_file(t.path().join("trunc.nvault-journal"));
    }
    // Inside the header area: not a volume / corrupt, never a crash.
    for keep in [0, 7, 8, 100, COPY_LEN, DB_OFFSET - 1, DB_OFFSET] {
        let q = t.path().join("trunc.nvault");
        std::fs::write(&q, &raw[..keep]).unwrap();
        assert!(Vault::open(&q, &pw("pw")).is_err(), "kept {keep} bytes");
        let _ = nv_core::peek(&q);
        std::fs::remove_file(&q).unwrap();
    }
}

/// Header MAC strip / forge: the checksum is unkeyed, so anyone can make a copy "valid";
/// the keyed MAC must still refuse it after unlock.
#[test]
fn header_mac_strip_is_refused() {
    let t = tempfile::tempdir().unwrap();
    let (p, _, _) = sample(t.path(), "v.nvault");
    for mac in [[0u8; 32], [0xff; 32]] {
        let q = t.path().join("mac.nvault");
        std::fs::copy(&p, &q).unwrap();
        for c in 0..2 {
            let mut h = read_copy(&q, c);
            h.header_mac = mac;
            write_copy(&q, c, &h);
        }
        assert!(matches!(Vault::open(&q, &pw("pw")), Err(OpenError::Corrupt(_))));
        std::fs::remove_file(&q).unwrap();
    }
    // Edit any MAC-covered field (seq, volume_id, slot order) and fix the checksum: refused.
    let edits: [fn(&mut Header); 3] = [
        |h| h.body.seq += 10,
        |h| h.body.volume_id[0] ^= 1,
        |h| h.body.slots.push(None),
    ];
    for e in edits {
        let q = t.path().join("edit.nvault");
        std::fs::copy(&p, &q).unwrap();
        for c in 0..2 {
            let mut h = read_copy(&q, c);
            e(&mut h);
            write_copy(&q, c, &h);
        }
        assert!(Vault::open(&q, &pw("pw")).is_err());
        std::fs::remove_file(&q).unwrap();
    }
}

/// A forged copy with a valid checksum and a higher seq (no key needed) wins over the
/// intact copy; its MAC then fails and the volume refuses to open with the right password,
/// although the other copy is intact and authentic. Desired: fall back to the other copy.
#[test]
fn forged_newest_copy_falls_back_to_the_authentic_one() {
    let t = tempfile::tempdir().unwrap();
    let (p, _, _) = sample(t.path(), "v.nvault");
    let mut h = read_copy(&p, 1);
    h.body.seq = u64::MAX;
    write_copy(&p, 1, &h);
    assert!(open_dump(&p, "pw").is_ok(), "the authentic copy A should still open");
}

/// A forged appearance (no key) is detected at unlock and flagged.
#[test]
fn forged_appearance_is_flagged_at_unlock() {
    let t = tempfile::tempdir().unwrap();
    let (p, _, _) = sample(t.path(), "v.nvault");
    let v = Vault::open(&p, &pw("pw")).unwrap();
    v.set_appearance(Some(&Appearance { accent: Some("prussia".into()), emblem: Some(LockedEmblem::Icon("key".into())) })).unwrap();
    v.close().unwrap();
    for c in 0..2 {
        let mut h = read_copy(&p, c);
        h.appearance.as_mut().unwrap().appearance.accent = Some("velluto".into());
        write_copy(&p, c, &h);
    }
    // Before unlock it is shown as is (display-only, documented)...
    assert_eq!(nv_core::peek(&p).unwrap().appearance.unwrap().accent.as_deref(), Some("velluto"));
    // ...and caught at unlock.
    let v = Vault::open(&p, &pw("pw")).unwrap();
    assert!(v.appearance_tampered());
    assert!(v.appearance().is_none());
}

/// Focused: SQLCipher notices the HMAC failure, zeroes the page and puts the pager in a
/// permanent error state, but the read in progress still returns the zeroed bytes as data
/// when the page is the last overflow page of a chunk (next-page pointer 0 = "end of chain").
/// Only the NEXT statement fails (as "out of memory"). Desired: the read itself fails.
#[test]
fn tampered_tail_page_never_reads_as_zeros() {
    let t = tempfile::tempdir().unwrap();
    let (p, _, big) = sample(t.path(), "v.nvault");
    let len = std::fs::metadata(&p).unwrap().len() as usize;
    let want = pat(7, 600 * 1024);
    let mut silent = vec![];
    for page in 0..(len - DB_OFFSET) / PAGE {
        let q = flipped(&p, t.path(), DB_OFFSET + page * PAGE + 100, 1);
        if let Ok(v) = Vault::open(&q, &pw("pw")) {
            // Half the pages with a group-commit transaction open (a write happened first).
            if page % 2 == 1 {
                let _ = v.create(nv_fs::ROOT, "w", nv_fs::Kind::File);
            }
            if let Ok(got) = read_all(&v, big)
                && got != want
            {
                let zeros = got.iter().zip(&want).filter(|(g, w)| g != w).count();
                silent.push((page + 1, zeros)); // SQLite page number
            }
        }
        std::fs::remove_file(&q).unwrap();
        let _ = std::fs::remove_file(t.path().join(format!("flip-{}.nvault-journal", DB_OFFSET + page * PAGE + 100)));
    }
    assert!(silent.is_empty(), "(pgno, bytes wrong) read back without an error: {silent:?}");
}
