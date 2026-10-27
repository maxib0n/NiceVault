use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use nv_core::{ResizeError, ResizeInfo, ResizeProgress};
use serde_json::json;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const PROGRESS_EVERY: Duration = Duration::from_millis(100);

impl From<ResizeError> for ApiError {
    fn from(e: ResizeError) -> Self {
        match e {
            ResizeError::NotFixed => ApiError::new("not_fixed", "This volume grows by itself: there is no size to change."),
            ResizeError::OutOfRange { min, max } => ApiError::new("out_of_range", format!("The size must be between {min} and {max} bytes.")),
            ResizeError::TooSmall { min } => {
                ApiError::new("too_small", format!("The data in the volume needs at least {min} bytes."))
            }
            ResizeError::NoSpace { need, free } => {
                ApiError::new("no_space", format!("Not enough free space on the drive: {need} bytes needed, {free} free."))
            }
            ResizeError::Fs(e) => e.into(),
        }
    }
}

impl AppState {
    pub fn vault_resize_info(&self, id: &str) -> ApiResult<ResizeInfo> {
        Ok(self.vault(id)?.resize_info()?)
    }

    pub fn vault_resize(&self, id: &str, size: u64) -> ApiResult<ResizeInfo> {
        let v = self.vault(id)?;
        let last = Mutex::new(None::<Instant>);
        let report = |p: ResizeProgress| {
            let mut l = last.lock().unwrap_or_else(|e| e.into_inner());
            if l.is_some_and(|t| t.elapsed() < PROGRESS_EVERY) {
                return;
            }
            *l = Some(Instant::now());
            let _ = self.vault(id);
            self.emit("vault-resize", json!({ "vault": id, "phase": p.phase, "done": p.done, "total": p.total }));
        };
        v.resize(size, &report)?;
        Ok(v.resize_info()?)
    }
}

pub mod cmd {
    use crate::error::{ApiError, ApiResult};
    use crate::state::AppState;
    use nv_core::ResizeInfo;
    use std::sync::Arc;
    use tauri::State;

    type St<'a> = State<'a, Arc<AppState>>;

    async fn blocking<T: Send + 'static>(st: &St<'_>, f: impl FnOnce(&AppState) -> ApiResult<T> + Send + 'static) -> ApiResult<T> {
        let s = st.inner().clone();
        tauri::async_runtime::spawn_blocking(move || f(&s)).await.map_err(|e| ApiError::new("error", format!("Internal error: {e}")))?
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn vault_resize_info(st: St<'_>, vault: String) -> ApiResult<ResizeInfo> {
        blocking(&st, move |s| s.vault_resize_info(&vault)).await
    }

    #[tauri::command(rename_all = "snake_case")]
    pub async fn vault_resize(st: St<'_>, vault: String, size: u64) -> ApiResult<ResizeInfo> {
        blocking(&st, move |s| s.vault_resize(&vault, size)).await
    }
}
