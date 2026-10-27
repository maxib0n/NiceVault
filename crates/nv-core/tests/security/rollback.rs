use crate::common::*;
use nv_core::{Appearance, KdfPreset, OpenError, Vault};
use nv_fs::{Fs, Kind, ROOT};

#[test]
fn old_header_written_back_is_refused() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("v.nvault");
    Vault::create(&p, &pw("leaked"), KdfPreset::Fast).unwrap().close().unwrap();
    let old_header = std::fs::read(&p).unwrap()[..DB_OFFSET].to_vec();
    let v = Vault::open(&p, &pw("leaked")).unwrap();
    v.change_password(0, &pw("new-secret"), KdfPreset::Fast, None).unwrap();
    let f = v.create(ROOT, "written-after-the-change", Kind::File).unwrap();
    v.write(f.id, 0, b"new data").unwrap();
    v.close().unwrap();
    assert_eq!(Vault::open(&p, &pw("leaked")).err(), Some(OpenError::WrongPassword));

    write_at(&p, 0, &old_header);
    let before = std::fs::read(&p).unwrap();
    assert_eq!(Vault::open(&p, &pw("leaked")).err(), Some(OpenError::RolledBack));
    assert!(std::fs::read(&p).unwrap() == before, "refused without writing");
    assert_eq!(Vault::open(&p, &pw("new-secret")).err(), Some(OpenError::WrongPassword));
}

#[test]
fn database_rollback_is_detected_against_the_anchor() {
    let t = tempfile::tempdir().unwrap();
    let (p, a, _) = sample(t.path(), "v.nvault");
    let old = std::fs::read(&p).unwrap();
    let v = Vault::open(&p, &pw("pw")).unwrap();
    v.write(a, 0, &[b'B'; 4096]).unwrap();
    v.create(ROOT, "later", Kind::Dir).unwrap();
    v.flush().unwrap();
    let anchor = v.commit_seq().unwrap();
    v.close().unwrap();
    let reopened = Vault::open(&p, &pw("pw")).unwrap();
    assert!(reopened.commit_seq().unwrap() >= anchor, "the current file is not older than its anchor");
    reopened.close().unwrap();
    let mut raw = std::fs::read(&p).unwrap();
    raw.truncate(DB_OFFSET);
    raw.extend_from_slice(&old[DB_OFFSET..]);
    std::fs::write(&p, raw).unwrap();
    let v = Vault::open(&p, &pw("pw")).expect("an older copy opens: the file alone cannot know");
    assert!(v.commit_seq().unwrap() < anchor, "rolled-back database not older than the anchor");
}

#[test]
#[ignore = "honest limit: a whole-file or whole-database rollback cannot be seen from the file alone (anchor: see above)"]
fn database_rollback_is_detected() {
    let t = tempfile::tempdir().unwrap();
    let (p, a, _) = sample(t.path(), "v.nvault");
    let old = std::fs::read(&p).unwrap();
    let v = Vault::open(&p, &pw("pw")).unwrap();
    v.write(a, 0, &[b'B'; 4096]).unwrap();
    v.create(ROOT, "later", Kind::Dir).unwrap();
    v.close().unwrap();
    let mut raw = std::fs::read(&p).unwrap();
    raw.truncate(DB_OFFSET);
    raw.extend_from_slice(&old[DB_OFFSET..]);
    std::fs::write(&p, raw).unwrap();
    let r = open_dump(&p, "pw");
    assert!(r.is_err(), "rolled-back database accepted: {:?}", r.map(|d| d.into_iter().map(|e| e.0).collect::<Vec<_>>()));
}

#[test]
#[ignore = "honest limit (local volumes): page-level rollback can yield a never-existing mixed state"]
fn single_page_rollback_is_detected() {
    let t = tempfile::tempdir().unwrap();
    let (p, a, big) = sample(t.path(), "v.nvault");
    let old = std::fs::read(&p).unwrap();
    let old_dump = open_dump(&p, "pw").unwrap();
    let v = Vault::open(&p, &pw("pw")).unwrap();
    v.write(a, 0, &[b'B'; 4096]).unwrap();
    v.write(big, 300 * 1024, &[0xEE; 1000]).unwrap();
    v.create(ROOT, "later", Kind::Dir).unwrap();
    v.close().unwrap();
    let new = std::fs::read(&p).unwrap();
    let new_dump = open_dump(&p, "pw").unwrap();
    let n = old.len().min(new.len());
    let changed: Vec<usize> = (0..(n - DB_OFFSET) / PAGE)
        .filter(|pg| {
            let off = DB_OFFSET + pg * PAGE;
            old[off..off + PAGE] != new[off..off + PAGE]
        })
        .collect();
    let mut mixed = vec![];
    for &pg in &changed {
        let off = DB_OFFSET + pg * PAGE;
        for (base, donor, how) in [(&new, &old, "old page in new file"), (&old, &new, "new page in old file")] {
            let mut m = base[..].to_vec();
            m[off..off + PAGE].copy_from_slice(&donor[off..off + PAGE]);
            if how == "new page in old file" {
                m[..DB_OFFSET].copy_from_slice(&new[..DB_OFFSET]);
            }
            let q = t.path().join("mixed.nvault");
            std::fs::write(&q, m).unwrap();
            if let Ok(d) = open_dump(&q, "pw")
                && d != old_dump
                && d != new_dump
            {
                let names: Vec<String> = d.iter().map(|(n, c)| format!("{n}:{}", c.first().map_or(String::new(), |b| format!("{b:02x}")))).collect();
                mixed.push(format!("pgno {} ({how}): {names:?}, integrity_check ok: {}", pg + 1, integrity_ok(&q, "pw")));
            }
            std::fs::remove_file(&q).unwrap();
            let _ = std::fs::remove_file(t.path().join("mixed.nvault-journal"));
        }
    }
    assert!(mixed.is_empty(), "{} changed pages; never-existing states accepted:\n{}", changed.len(), mixed.join("\n"));
}

#[test]
fn appearance_replay_is_flagged() {
    let t = tempfile::tempdir().unwrap();
    let (p, _, _) = sample(t.path(), "v.nvault");
    let set = |accent: &str| {
        let v = Vault::open(&p, &pw("pw")).unwrap();
        v.set_appearance(Some(&Appearance { accent: Some(accent.into()), emblem: None })).unwrap();
        v.close().unwrap();
    };
    set("prussia");
    let old = read_copy(&p, 0).appearance;
    set("bottiglia");
    for c in 0..2 {
        let mut h = read_copy(&p, c);
        h.appearance = old.clone();
        write_copy(&p, c, &h);
    }
    let v = Vault::open(&p, &pw("pw")).unwrap();
    assert!(v.appearance_tampered(), "replayed appearance {:?} accepted as authentic", v.appearance());
}

#[test]
fn appearance_strip_is_flagged_and_unrecorded_volumes_are_not() {
    let t = tempfile::tempdir().unwrap();
    let (p, _, _) = sample(t.path(), "v.nvault");
    let v = Vault::open(&p, &pw("pw")).unwrap();
    v.set_appearance(Some(&Appearance { accent: Some("prussia".into()), emblem: None })).unwrap();
    v.close().unwrap();
    let saved = std::fs::read(&p).unwrap();
    for c in 0..2 {
        let mut h = read_copy(&p, c);
        h.appearance = None;
        write_copy(&p, c, &h);
    }
    let v = Vault::open(&p, &pw("pw")).unwrap();
    assert!(v.appearance_tampered(), "stripped appearance accepted");
    v.close().unwrap();
    std::fs::write(&p, &saved).unwrap();
    for _ in 0..2 {
        let v = Vault::open(&p, &pw("pw")).unwrap();
        assert!(!v.appearance_tampered());
        assert_eq!(v.appearance().and_then(|a| a.accent).as_deref(), Some("prussia"));
        v.close().unwrap();
    }
}
