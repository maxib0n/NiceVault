//! `{ code, message }` — the error shape of every command (see app/ui/src/lib/errors.ts).

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into() }
    }
    pub fn locked() -> Self {
        Self::new("locked", "The volume is locked.")
    }
    pub fn not_found(what: &str) -> Self {
        Self::new("not_found", format!("{what} not found."))
    }
    pub fn io(detail: impl std::fmt::Display) -> Self {
        Self::new("io", format!("Read or write error: {detail}"))
    }
    /// Data written by a newer NiceVault that this build cannot read or must not overwrite.
    pub fn unsupported_version() -> Self {
        Self::new(
            "unsupported_version",
            "This volume was saved by a newer version of NiceVault: open it with that version. It was not changed.",
        )
    }
    pub fn cancelled() -> Self {
        Self::new("cancelled", "Cancelled.")
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl From<nv_fs::Error> for ApiError {
    fn from(e: nv_fs::Error) -> Self {
        use nv_fs::Error as E;
        match e {
            E::NotFound => Self::new("not_found", "Not found."),
            E::Exists => Self::new("exists", "Something with this name already exists."),
            E::NotEmpty => Self::new("not_empty", "The folder is not empty."),
            E::NotDir => Self::new("not_dir", "Not a folder."),
            E::IsDir => Self::new("is_dir", "It is a folder."),
            E::InvalidName => Self::new("invalid_name", "Invalid name."),
            E::Locked => Self::locked(),
            E::Corrupt(m) => Self::new("corrupt", format!("The volume is damaged ({m}).")),
            E::Io(m) => Self::io(m),
        }
    }
}

impl From<nv_core::SlotError> for ApiError {
    fn from(e: nv_core::SlotError) -> Self {
        use nv_core::SlotError as E;
        match e {
            E::NotFound => Self::not_found("Key"),
            E::NotPassword => Self::new("invalid_slot", "This key is not a password."),
            E::LastSlot => Self::new("last_slot", "It is the only key: it cannot be removed."),
            E::Full => Self::new("slots_full", "There are already 8 keys."),
            E::LabelTooLong => {
                Self::new("invalid_name", format!("The key name is too long (at most {} bytes).", nv_core::MAX_LABEL))
            }
            E::DuressConflict => Self::new(
                "duress_conflict",
                "A self-destruct password must differ from every other password of the volume.",
            ),
            E::WrongPassword => Self::new("wrong_password", "The current password is not correct."),
            E::Fs(e) => e.into(),
        }
    }
}

impl From<nv_core::OpenError> for ApiError {
    fn from(e: nv_core::OpenError) -> Self {
        use nv_core::OpenError as E;
        match e {
            E::WrongPassword => Self::new("wrong_password", "Wrong password."),
            E::NotAVault => Self::new("not_a_vault", "This file is not a NiceVault volume."),
            E::Corrupt(m) => Self::new("corrupt", format!("The volume is damaged ({m}).")),
            E::Io(m) => Self::io(m),
            E::UnsupportedVersion(_) => Self::unsupported_version(),
            E::RolledBack => Self::new("rolled_back", "This volume's header was replaced with an older copy; it was not opened."),
        }
    }
}

impl From<nv_mount::MountError> for ApiError {
    fn from(e: nv_mount::MountError) -> Self {
        use nv_mount::MountError as E;
        match e {
            E::Unavailable(m) => Self::new("mount_unavailable", format!("This computer cannot mount it as a drive ({m}).")),
            E::Mountpoint(m) => Self::new("mount_failed", format!("Unusable mount point: {m}")),
            E::Failed(m) => Self::new("mount_failed", format!("The system refused the mount: {m}")),
            E::Busy(m) => Self::new("busy", format!("Files on the drive are still open: {m}")),
        }
    }
}

impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::NotFound {
            return Self::new("not_found", "Not found.");
        }
        Self::io(e)
    }
}
