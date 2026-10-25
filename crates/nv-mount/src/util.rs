//! Helpers shared by the backends.
#![allow(dead_code)] // each backend uses a different subset

#[cfg(unix)]
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use crate::{MountError, Result};

/// Turn a volume name into something safe as a single path component / volume label.
pub(crate) fn sanitize_name(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    s = s.trim().trim_start_matches('.').trim().to_string();
    if s.chars().count() > 64 {
        s = s.chars().take(64).collect::<String>().trim_end().to_string();
    }
    if s.is_empty() { "Vault".to_string() } else { s }
}

pub(crate) fn ns_to_system_time(ns: i64) -> SystemTime {
    if ns >= 0 {
        UNIX_EPOCH + Duration::from_nanos(ns as u64)
    } else {
        UNIX_EPOCH - Duration::from_nanos(ns.unsigned_abs())
    }
}

pub(crate) fn system_time_to_ns(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_nanos()).unwrap_or(i64::MAX),
        Err(e) => -i64::try_from(e.duration().as_nanos()).unwrap_or(i64::MAX),
    }
}

pub(crate) fn now_ns() -> i64 {
    system_time_to_ns(SystemTime::now())
}

/// A directory we mount on, remembering whether we created it (and so should remove it).
#[cfg(unix)]
#[derive(Debug)]
pub(crate) struct MountDir {
    pub path: PathBuf,
    pub created: bool,
}

#[cfg(unix)]
impl MountDir {
    /// Remove the directory if we created it and it is empty again. Never recursive.
    pub fn cleanup(&self) {
        if self.created {
            let _ = std::fs::remove_dir(&self.path);
        }
    }
}

#[cfg(unix)]
fn is_mountpoint(p: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let (Ok(m), Some(parent)) = (std::fs::symlink_metadata(p), p.parent()) else {
        return false;
    };
    match std::fs::metadata(parent) {
        Ok(pm) => pm.dev() != m.dev() || pm.ino() == m.ino(),
        Err(_) => false,
    }
}

#[cfg(unix)]
fn usable_existing(p: &Path) -> bool {
    match std::fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => {
            !is_mountpoint(p) && std::fs::read_dir(p).map(|mut d| d.next().is_none()).unwrap_or(false)
        }
        _ => false,
    }
}

/// `base` must be a real directory owned by us that nobody else can write (it may be a
/// predictable /tmp path another user created first). Group/other read is tightened to 0700.
#[cfg(unix)]
fn private_base(base: &Path) -> Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let refuse = |why: &str| MountError::Mountpoint(format!("{}: {why}", base.display()));
    let md = std::fs::symlink_metadata(base).map_err(|e| refuse(&e.to_string()))?;
    // SAFETY: geteuid has no preconditions.
    if !md.is_dir() || md.uid() != unsafe { libc::geteuid() } || md.mode() & 0o022 != 0 {
        return Err(refuse("not a private directory of this user"));
    }
    if md.mode() & 0o777 != 0o700 {
        std::fs::set_permissions(base, std::fs::Permissions::from_mode(0o700)).map_err(|e| refuse(&e.to_string()))?;
    }
    Ok(())
}

/// Pick `<base>/<name>` (or `<name> 2`, `<name> 3`, ...), creating `base` (0700) and the leaf.
/// `on_stale` is given a chance to clean up a dead mount left by a crashed process.
#[cfg(unix)]
pub(crate) fn pick_dir_under(base: &Path, name: &str, on_stale: &dyn Fn(&Path)) -> Result<MountDir> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(base)
        .map_err(|e| MountError::Mountpoint(format!("{}: {e}", base.display())))?;
    private_base(base)?;
    let name = sanitize_name(name);
    for i in 1..=99 {
        let leaf = if i == 1 { name.clone() } else { format!("{name} {i}") };
        let p = base.join(&leaf);
        if let Err(e) = std::fs::symlink_metadata(&p)
            && e.raw_os_error() == Some(libc::ENOTCONN)
        {
            on_stale(&p);
        }
        match std::fs::DirBuilder::new().mode(0o700).create(&p) {
            Ok(()) => return Ok(MountDir { path: p, created: true }),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if usable_existing(&p) {
                    return Ok(MountDir { path: p, created: false });
                }
            }
            Err(e) => return Err(MountError::Mountpoint(format!("{}: {e}", p.display()))),
        }
    }
    Err(MountError::Mountpoint(format!("no free mountpoint under {}", base.display())))
}

/// Validate (or create) a caller-supplied mountpoint directory.
#[cfg(unix)]
pub(crate) fn explicit_dir(p: &Path, on_stale: &dyn Fn(&Path)) -> Result<MountDir> {
    if let Err(e) = std::fs::symlink_metadata(p)
        && e.raw_os_error() == Some(libc::ENOTCONN)
    {
        on_stale(p);
    }
    match std::fs::metadata(p) {
        Ok(m) if m.is_dir() => {
            if is_mountpoint(p) {
                Err(MountError::Mountpoint(format!("{} is already a mountpoint", p.display())))
            } else {
                Ok(MountDir { path: p.to_path_buf(), created: false })
            }
        }
        Ok(_) => Err(MountError::Mountpoint(format!("{} is not a directory", p.display()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(p).map_err(|e| MountError::Mountpoint(format!("{}: {e}", p.display())))?;
            Ok(MountDir { path: p.to_path_buf(), created: true })
        }
        Err(e) => Err(MountError::Mountpoint(format!("{}: {e}", p.display()))),
    }
}

/// Find an executable by name in `$PATH` plus the usual system dirs.
#[cfg(unix)]
pub(crate) fn find_exe(name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain(["/usr/bin", "/bin", "/usr/sbin", "/sbin", "/usr/local/bin"].map(PathBuf::from))
        .map(|d| d.join(name))
        .find(|p| std::fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false))
}

#[cfg(unix)]
/// Run a command, returning Err(stderr+stdout) on non-zero exit.
pub(crate) fn run(cmd: &mut std::process::Command) -> std::result::Result<(), String> {
    match cmd.stdin(std::process::Stdio::null()).output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => {
            let mut s = String::from_utf8_lossy(&o.stderr).trim().to_string();
            let out = String::from_utf8_lossy(&o.stdout);
            if !out.trim().is_empty() {
                if !s.is_empty() {
                    s.push_str("; ");
                }
                s.push_str(out.trim());
            }
            if s.is_empty() {
                s = format!("exited with {}", o.status);
            }
            Err(s)
        }
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize() {
        assert_eq!(sanitize_name("My Vault"), "My Vault");
        assert_eq!(sanitize_name("a/b\\c:d"), "a_b_c_d");
        assert_eq!(sanitize_name("  ..hidden "), "hidden");
        assert_eq!(sanitize_name(""), "Vault");
        assert_eq!(sanitize_name(".."), "Vault");
        assert_eq!(sanitize_name(&"x".repeat(100)).len(), 64);
    }

    #[test]
    fn times_roundtrip() {
        for ns in [0i64, 1, 1_700_000_000_123_456_789, -5_000_000_001] {
            assert_eq!(system_time_to_ns(ns_to_system_time(ns)), ns);
        }
    }
}
