use nv_core::{FileKind, KdfPreset, Vault, io_counters};
use nv_fs::{Fs, Kind, ROOT};
use secrecy::SecretString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

fn arg(name: &str) -> Option<String> {
    let mut it = std::env::args();
    while let Some(a) = it.next() {
        if a == name {
            return it.next();
        }
    }
    None
}

fn fill(seed: u64, buf: &mut [u8]) {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    for w in buf.chunks_mut(8) {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let b = x.to_le_bytes();
        w.copy_from_slice(&b[..w.len()]);
    }
}

fn write_bytes_proc() -> u64 {
    std::fs::read_to_string("/proc/self/io")
        .ok()
        .and_then(|s| s.lines().find_map(|l| l.strip_prefix("write_bytes: ").and_then(|v| v.trim().parse().ok())))
        .unwrap_or(0)
}

fn ensure_src(src: &Path, count: usize, size: usize) -> PathBuf {
    let d = src.join(format!("{count}x{size}"));
    let done = d.join(".complete");
    if done.exists() {
        return d;
    }
    std::fs::create_dir_all(&d).unwrap();
    let mut buf = vec![0u8; size.min(1 << 20)];
    for i in 0..count {
        let mut f = std::fs::File::create(d.join(format!("f{i:05}.bin"))).unwrap();
        let mut left = size;
        let mut part = 0u64;
        while left > 0 {
            let n = left.min(buf.len());
            fill((i as u64) << 20 | part, &mut buf[..n]);
            f.write_all(&buf[..n]).unwrap();
            left -= n;
            part += 1;
        }
    }
    std::fs::write(&done, b"").unwrap();
    d
}

fn main() {
    let vol_dir = PathBuf::from(arg("--vol-dir").expect("--vol-dir"));
    let src_dir = PathBuf::from(arg("--src-dir").unwrap_or_else(|| vol_dir.join("src").display().to_string()));
    let mode = arg("--mode").unwrap_or_else(|| "api".into());
    let files = arg("--files").expect("--files COUNTxBYTES");
    let (count, size) = files.split_once('x').map(|(c, s)| (c.parse::<usize>().unwrap(), s.parse::<usize>().unwrap())).unwrap();
    let label = arg("--label").unwrap_or_default();
    let single = std::env::args().any(|a| a == "--single");
    let rewrite = std::env::args().any(|a| a == "--rewrite");
    let fixed: Option<u64> = arg("--fixed").map(|v| v.parse().expect("--fixed BYTES"));
    let side = std::env::args().any(|a| a == "--side");
    let label = if single { format!("{label}v2") } else { label };
    let label = match (fixed, side) {
        (Some(_), true) => format!("{label}fxside"),
        (Some(_), false) => format!("{label}fx"),
        (None, _) => label,
    };
    let logical = (count * size) as u64;
    std::fs::create_dir_all(&vol_dir).unwrap();
    let vol = vol_dir.join(format!("bench-{mode}.nvault"));
    let _ = std::fs::remove_file(&vol);

    let src = if mode == "api" { None } else { Some(ensure_src(&src_dir, count, size)) };
    if let Some(s) = &src {
        let _ = Command::new("cat").arg("--").args(std::fs::read_dir(s).unwrap().map(|e| e.unwrap().path())).stdout(std::process::Stdio::null()).status();
    }

    let pw = SecretString::from("bench".to_owned());
    let mut c0 = io_counters();
    let w0 = write_bytes_proc();
    let mut t0 = Instant::now();
    let t_copy;
    match mode.as_str() {
        "api" => {
            let v = match fixed {
                Some(cap) => {
                    let v = Vault::create_fixed_as(&vol, &pw, KdfPreset::Fast, cap, !side).unwrap();
                    (c0, t0) = (io_counters(), Instant::now());
                    v
                }
                None => Vault::create(&vol, &pw, KdfPreset::Fast).unwrap(),
            };
            if single {
                v.upgrade_format().unwrap();
                (c0, t0) = (io_counters(), Instant::now());
            }
            let d = v.create(ROOT, "in", Kind::Dir).unwrap();
            let mut buf = vec![0u8; 1 << 20];
            for i in 0..count {
                let f = v.create(d.id, &format!("f{i:05}.bin"), Kind::File).unwrap();
                let mut off = 0usize;
                let mut part = 0u64;
                while off < size {
                    let n = (size - off).min(buf.len());
                    fill((i as u64) << 20 | part, &mut buf[..n]);
                    v.write(f.id, off as u64, &buf[..n]).unwrap();
                    off += n;
                    part += 1;
                }
            }
            v.flush().unwrap();
            t_copy = t0.elapsed().as_secs_f64();
            if rewrite {
                let r0 = io_counters();
                let t = Instant::now();
                for i in 0..count {
                    let f = v.lookup(d.id, &format!("f{i:05}.bin")).unwrap();
                    let mut off = 0usize;
                    while off < size {
                        let n = (size - off).min(buf.len());
                        fill((i as u64) << 20 | 0xffff, &mut buf[..n]);
                        v.write(f.id, off as u64, &buf[..n]).unwrap();
                        off += n;
                    }
                }
                v.flush().unwrap();
                let el = t.elapsed().as_secs_f64();
                let c = io_counters().since(&r0);
                println!(
                    "{label:<10} rewr  {files:>13}  {:7.1} MB/s ({el:6.1}s)  amp {:.2} (db {:.2} jrnl {:.2})  syncs/GB {:6.1}",
                    logical as f64 / 1e6 / el,
                    c.total_write_bytes() as f64 / logical as f64,
                    c.write_bytes[FileKind::Db as usize] as f64 / logical as f64,
                    c.write_bytes[FileKind::Journal as usize] as f64 / logical as f64,
                    c.total_syncs() as f64 / (logical as f64 / 1e9),
                );
            }
            v.close().unwrap();
        }
        "fuse" => {
            let v = Arc::new(Vault::create(&vol, &pw, KdfPreset::Fast).unwrap());
            if single {
                v.upgrade_format().unwrap();
                (c0, t0) = (io_counters(), Instant::now());
            }
            let mnt = vol_dir.join("mnt");
            std::fs::create_dir_all(&mnt).unwrap();
            let opts = nv_mount::MountOpts { volume_name: "Bench".into(), mountpoint: Some(mnt.clone()), read_only: false };
            let h = nv_mount::mount(v.clone() as Arc<dyn Fs>, opts).expect("mount");
            let st = Command::new("cp").arg("-r").arg(src.as_ref().unwrap()).arg(h.mountpoint().join("in")).status().unwrap();
            assert!(st.success(), "cp failed");
            t_copy = t0.elapsed().as_secs_f64();
            h.unmount().unwrap();
            Arc::try_unwrap(v).ok().expect("vault still shared").close().unwrap();
        }
        "plain" => {
            let dst = vol_dir.join("plain");
            let _ = std::fs::remove_dir_all(&dst);
            let st = Command::new("cp").arg("-r").arg("--reflink=never").arg(src.as_ref().unwrap()).arg(&dst).status().unwrap();
            assert!(st.success());
            t_copy = t0.elapsed().as_secs_f64();
            Command::new("sync").arg("-f").arg(&dst).status().unwrap();
        }
        m => panic!("unknown mode {m}"),
    }
    let el = t0.elapsed().as_secs_f64();
    let c = io_counters().since(&c0);
    let phys = write_bytes_proc() - w0;
    let vol_len = std::fs::metadata(&vol).map(|m| m.len()).unwrap_or(0);
    let gb = logical as f64 / 1e9;
    let mb = logical as f64 / 1e6;
    let (db, wal, jr) = (c.write_bytes[FileKind::Db as usize], c.write_bytes[FileKind::Wal as usize], c.write_bytes[FileKind::Journal as usize]);
    println!(
        "{label:<10} {mode:<5} {files:>13}  {:7.1} MB/s (copy {:6.1}s, durable {:6.1}s)  amp {:.2} (db {:.2} wal {:.2} jrnl {:.2}; proc {:.2})  syncs/GB {:6.1} (db {} wal {} jrnl {})  vol {:.2}x",
        mb / el,
        t_copy,
        el,
        c.total_write_bytes() as f64 / logical as f64,
        db as f64 / logical as f64,
        wal as f64 / logical as f64,
        jr as f64 / logical as f64,
        phys as f64 / logical as f64,
        c.total_syncs() as f64 / gb,
        c.syncs[0],
        c.syncs[1],
        c.syncs[2],
        vol_len as f64 / logical as f64,
    );
    let _ = std::fs::remove_file(&vol);
    let _ = std::fs::remove_dir_all(vol_dir.join("plain"));
}
