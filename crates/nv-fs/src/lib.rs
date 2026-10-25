//! The contract between nv-core (implements), nv-mount and the app (consume).

use serde::Serialize;

pub type NodeId = u64;
pub const ROOT: NodeId = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    File,
    Dir,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Node {
    pub id: NodeId,
    pub parent: NodeId, // root's parent is ROOT
    pub name: String,   // root's name is ""
    pub kind: Kind,
    pub size: u64,  // 0 for dirs
    pub mtime: i64, // unix ns
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Stats {
    pub total_bytes: u64, // capacity of the host drive holding the volume
    pub free_bytes: u64,
    pub used_bytes: u64, // sum of file sizes
    pub files: u64,
}

#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "code", content = "message", rename_all = "snake_case")]
pub enum Error {
    #[error("not found")]
    NotFound,
    #[error("already exists")]
    Exists,
    #[error("directory not empty")]
    NotEmpty,
    #[error("not a directory")]
    NotDir,
    #[error("is a directory")]
    IsDir,
    #[error("invalid name")]
    InvalidName,
    #[error("vault is locked")]
    Locked,
    #[error("corrupt volume: {0}")]
    Corrupt(String),
    #[error("io: {0}")]
    Io(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Checked on create and rename only: names already stored stay readable. Portable on
/// every host: no Windows path syntax, reserved device names or trailing dot/space.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 255 && name != "." && name != ".." && !name.contains(bad_char) && !name.ends_with(['.', ' ']) && !reserved(name)
}

fn bad_char(c: char) -> bool {
    c.is_control() || "/\\:*?\"<>|".contains(c)
}

fn reserved(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").trim_end_matches(' ').to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}

/// `name` as one safe host path component (for names stored before `valid_name` got
/// strict, or coming from the host): unsafe characters become `_`.
pub fn host_name(name: &str) -> String {
    let mut s: String = name.chars().map(|c| if bad_char(c) { '_' } else { c }).collect();
    let keep = s.trim_end_matches(['.', ' ']).len();
    if keep < s.len() || s.is_empty() {
        s.truncate(keep);
        s.push('_');
    }
    if reserved(&s) { s.insert(0, '_') }
    s
}

/// Filesystem view of an unlocked vault. Every method is thread-safe.
pub trait Fs: Send + Sync {
    fn getattr(&self, id: NodeId) -> Result<Node>;
    fn lookup(&self, parent: NodeId, name: &str) -> Result<Node>;
    fn readdir(&self, dir: NodeId) -> Result<Vec<Node>>;
    fn create(&self, parent: NodeId, name: &str, kind: Kind) -> Result<Node>;
    /// Short read only at EOF.
    fn read(&self, id: NodeId, offset: u64, buf: &mut [u8]) -> Result<usize>;
    /// Writing past EOF zero-fills the gap.
    fn write(&self, id: NodeId, offset: u64, data: &[u8]) -> Result<usize>;
    fn set_size(&self, id: NodeId, size: u64) -> Result<()>;
    fn set_mtime(&self, id: NodeId, mtime: i64) -> Result<()>;
    /// POSIX semantics: replaces an existing target file or empty dir; Err(Exists) for a non-empty dir target.
    fn rename(&self, id: NodeId, new_parent: NodeId, new_name: &str) -> Result<()>;
    /// File or empty dir.
    fn remove(&self, id: NodeId) -> Result<()>;
    /// Durability point: everything done so far survives a crash or power loss once this
    /// returns. Backends call it for fsync(2) / FlushFileBuffers and on unmount — not on every
    /// close (an implementation makes its changes durable by itself within a few seconds).
    fn flush(&self) -> Result<()>;
    fn stats(&self) -> Result<Stats>;
}

#[cfg(test)]
mod tests {
    #[test]
    fn names() {
        assert!(super::valid_name("a.txt"));
        for bad in ["", ".", "..", "a/b", "a\0", &"x".repeat(256), "a\\b", "c:x", "a?", "a\n", "x.", "x ", "CON", "nul.txt", "Com1", "lpt9.a.b", "aux .x"] {
            assert!(!super::valid_name(bad), "{bad:?}");
        }
        for ok in ["COM0", "LPT10", "console", "con-x", ".bashrc", "a b.c"] {
            assert!(super::valid_name(ok), "{ok:?}");
        }
        for (raw, h) in [(r"..\..\x.bat", ".._.._x.bat"), ("C:\\x", "C__x"), ("x.. ", "x_"), ("", "_"), ("..", "_"), ("NUL.txt", "_NUL.txt"), ("ok", "ok")] {
            assert_eq!(super::host_name(raw), h, "{raw:?}");
            assert!(super::valid_name(&super::host_name(raw)));
        }
    }
}
