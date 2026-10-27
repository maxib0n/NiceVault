use crate::error::{ApiError, ApiResult};
use crate::mobile_paths::{doc_id, doc_mime, parse_doc_id};
use crate::state::AppState;
use nv_fs::{Fs, Kind, Node, NodeId, ROOT};
use serde::Serialize;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const BUSY_MS: u64 = 30_000;

static LAST_USE: AtomicU64 = AtomicU64::new(0);
static OPEN: AtomicUsize = AtomicUsize::new(0);

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn used() {
    LAST_USE.store(now_ms(), Ordering::SeqCst);
}

pub fn busy() -> bool {
    OPEN.load(Ordering::SeqCst) > 0 || now_ms().saturating_sub(LAST_USE.load(Ordering::SeqCst)) < BUSY_MS
}

pub fn opened() {
    OPEN.fetch_add(1, Ordering::SeqCst);
    used();
}
pub fn closed() {
    let _ = OPEN.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1));
    used();
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Root {
    pub root: String,
    pub doc: String,
    pub title: String,
    pub free: u64,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Row {
    pub doc: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
    pub mtime: i64,
    pub dir: bool,
}

fn bad() -> ApiError {
    ApiError::new("not_found", "No such document.")
}

fn split(doc: &str) -> ApiResult<(&str, NodeId)> {
    parse_doc_id(doc).ok_or_else(bad)
}

fn row(vault: &str, n: &Node, title: &str) -> Row {
    let dir = n.kind == Kind::Dir;
    let name = if n.id == ROOT { title.to_owned() } else { n.name.clone() };
    Row {
        doc: doc_id(vault, n.id),
        mime: if dir { "vnd.android.document/directory".into() } else { doc_mime(&n.name) },
        name,
        size: n.size,
        mtime: n.mtime / 1_000_000,
        dir,
    }
}

fn title_of(st: &AppState, vault: &str) -> String {
    st.vault_list_open().into_iter().find(|v| v.id == vault).map(|v| v.name).unwrap_or_default()
}

pub fn roots(st: &AppState) -> Vec<Root> {
    st.vault_list_open()
        .into_iter()
        .map(|v| Root { free: st.vault_stats(&v.id).map_or(0, |s| s.free_bytes), doc: doc_id(&v.id, ROOT), root: v.id, title: v.name })
        .collect()
}

pub fn stat(st: &AppState, doc: &str) -> ApiResult<Row> {
    let (v, id) = split(doc)?;
    let n = st.vault(v)?.getattr(id)?;
    used();
    Ok(row(v, &n, &title_of(st, v)))
}

pub fn children(st: &AppState, doc: &str) -> ApiResult<Vec<Row>> {
    let (v, id) = split(doc)?;
    let mut list = st.vault(v)?.readdir(id)?;
    list.sort_by_key(|n| (n.kind != Kind::Dir, n.name.to_lowercase()));
    used();
    Ok(list.iter().map(|n| row(v, n, "")).collect())
}

pub fn is_child(st: &AppState, parent: &str, doc: &str) -> bool {
    let (Ok((pv, pid)), Ok((v, mut id))) = (split(parent), split(doc)) else { return false };
    let Ok(fs) = st.vault(v) else { return false };
    if pv != v {
        return false;
    }
    for _ in 0..4096 {
        if id == pid {
            return true;
        }
        if id == ROOT {
            return false;
        }
        match fs.getattr(id) {
            Ok(n) => id = n.parent,
            Err(_) => return false,
        }
    }
    false
}

pub fn open(st: &AppState, doc: &str, write: bool, truncate: bool) -> ApiResult<u64> {
    let (v, id) = split(doc)?;
    let fs = st.vault(v)?;
    let n = fs.getattr(id)?;
    if n.kind != Kind::File {
        return Err(nv_fs::Error::IsDir.into());
    }
    if write && truncate && n.size > 0 {
        fs.set_size(id, 0)?;
        fs.flush()?;
        return Ok(0);
    }
    Ok(n.size)
}

pub fn read(st: &AppState, doc: &str, off: u64, buf: &mut [u8]) -> ApiResult<usize> {
    let (v, id) = split(doc)?;
    used();
    Ok(st.vault(v)?.read(id, off, buf)?)
}

pub fn write(st: &AppState, doc: &str, off: u64, data: &[u8]) -> ApiResult<usize> {
    let (v, id) = split(doc)?;
    used();
    Ok(st.vault(v)?.write(id, off, data)?)
}

pub fn size(st: &AppState, doc: &str) -> ApiResult<u64> {
    let (v, id) = split(doc)?;
    Ok(st.vault(v)?.getattr(id)?.size)
}

pub fn flush(st: &AppState, doc: &str) -> ApiResult<()> {
    let (v, id) = split(doc)?;
    let fs = st.vault(v)?;
    let mtime = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos() as i64);
    let _ = fs.set_mtime(id, mtime);
    Ok(fs.flush()?)
}

fn free_name(fs: &dyn Fs, dir: NodeId, name: &str) -> ApiResult<String> {
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    for k in 1..10_000 {
        let n = if k == 1 { name.to_owned() } else { format!("{stem} ({k}){ext}") };
        match fs.lookup(dir, &n) {
            Err(nv_fs::Error::NotFound) => return Ok(n),
            Err(e) => return Err(e.into()),
            Ok(_) => {}
        }
    }
    Err(nv_fs::Error::Exists.into())
}

pub fn create(st: &AppState, parent: &str, name: &str, dir: bool) -> ApiResult<String> {
    let (v, pid) = split(parent)?;
    let fs = st.vault(v)?;
    let name = name.trim();
    if !nv_fs::valid_name(name) {
        return Err(nv_fs::Error::InvalidName.into());
    }
    let name = free_name(fs.as_ref(), pid, name)?;
    let n = fs.create(pid, &name, if dir { Kind::Dir } else { Kind::File })?;
    fs.flush()?;
    used();
    Ok(doc_id(v, n.id))
}

pub fn delete(st: &AppState, doc: &str) -> ApiResult<()> {
    let (v, id) = split(doc)?;
    used();
    st.fs_delete(v, &[id])
}

pub fn rename(st: &AppState, doc: &str, name: &str) -> ApiResult<String> {
    let (v, id) = split(doc)?;
    let parent = st.vault(v)?.getattr(id)?.parent;
    st.fs_rename(v, id, parent, name)?;
    used();
    Ok(doc.to_owned())
}

pub fn move_to(st: &AppState, doc: &str, target: &str) -> ApiResult<String> {
    let (v, id) = split(doc)?;
    let (tv, tid) = split(target)?;
    if tv != v {
        return Err(ApiError::new("unsupported", "Files move between volumes by copy."));
    }
    let name = st.vault(v)?.getattr(id)?.name;
    st.fs_rename(v, id, tid, &name)?;
    used();
    Ok(doc.to_owned())
}

#[cfg(target_os = "android")]
pub mod jni_glue {
    use super::*;
    use jni::JNIEnv;
    use jni::objects::{JByteArray, JClass, JString};
    use jni::sys::{jboolean, jint, jlong, jstring};
    use std::sync::Arc;

    fn state() -> Option<Arc<AppState>> {
        crate::mobile::app_state()
    }

    fn s(env: &mut JNIEnv, j: &JString) -> String {
        env.get_string(j).map(|v| v.into()).unwrap_or_default()
    }

    fn out(env: &mut JNIEnv, v: String) -> jstring {
        env.new_string(v).map(|j| j.into_raw()).unwrap_or(std::ptr::null_mut())
    }

    fn json<T: Serialize>(r: ApiResult<T>) -> String {
        match r {
            Ok(v) => serde_json::json!({ "ok": v }).to_string(),
            Err(e) => serde_json::json!({ "error": e.code, "message": e.message }).to_string(),
        }
    }

    fn with<T: Serialize>(f: impl FnOnce(&AppState) -> ApiResult<T>) -> String {
        json(state().ok_or_else(ApiError::locked).and_then(|st| f(&st)))
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_app_nicevault_desktop_NvDocs_roots(mut env: JNIEnv, _: JClass) -> jstring {
        let r = with(|st| Ok(roots(st)));
        out(&mut env, r)
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_app_nicevault_desktop_NvDocs_stat(mut env: JNIEnv, _: JClass, doc: JString) -> jstring {
        let d = s(&mut env, &doc);
        let r = with(|st| stat(st, &d));
        out(&mut env, r)
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_app_nicevault_desktop_NvDocs_children(mut env: JNIEnv, _: JClass, doc: JString) -> jstring {
        let d = s(&mut env, &doc);
        let r = with(|st| children(st, &d));
        out(&mut env, r)
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_app_nicevault_desktop_NvDocs_isChild(mut env: JNIEnv, _: JClass, parent: JString, doc: JString) -> jboolean {
        let (p, d) = (s(&mut env, &parent), s(&mut env, &doc));
        state().is_some_and(|st| is_child(&st, &p, &d)) as jboolean
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_app_nicevault_desktop_NvDocs_open(mut env: JNIEnv, _: JClass, doc: JString, write: jboolean, truncate: jboolean) -> jlong {
        let d = s(&mut env, &doc);
        match state().ok_or_else(ApiError::locked).and_then(|st| open(&st, &d, write != 0, truncate != 0)) {
            Ok(n) => {
                opened();
                n as jlong
            }
            Err(_) => -1,
        }
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_app_nicevault_desktop_NvDocs_release(mut env: JNIEnv, _: JClass, doc: JString, write: jboolean) -> jboolean {
        let d = s(&mut env, &doc);
        closed();
        let ok = write == 0 || state().is_some_and(|st| flush(&st, &d).is_ok());
        ok as jboolean
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_app_nicevault_desktop_NvDocs_read(mut env: JNIEnv, _: JClass, doc: JString, off: jlong, buf: JByteArray, len: jint) -> jint {
        let d = s(&mut env, &doc);
        let Some(st) = state() else { return -1 };
        let mut tmp = vec![0u8; len.max(0) as usize];
        match read(&st, &d, off.max(0) as u64, &mut tmp) {
            Ok(n) => {
                let signed: &[i8] = unsafe { std::slice::from_raw_parts(tmp.as_ptr().cast(), n) };
                if env.set_byte_array_region(&buf, 0, signed).is_err() {
                    return -1;
                }
                tmp.fill(0);
                n as jint
            }
            Err(_) => -1,
        }
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_app_nicevault_desktop_NvDocs_write(mut env: JNIEnv, _: JClass, doc: JString, off: jlong, buf: JByteArray, len: jint) -> jint {
        let d = s(&mut env, &doc);
        let Some(st) = state() else { return -1 };
        let mut tmp = vec![0i8; len.max(0) as usize];
        if env.get_byte_array_region(&buf, 0, &mut tmp).is_err() {
            return -1;
        }
        let data: &[u8] = unsafe { std::slice::from_raw_parts(tmp.as_ptr().cast(), tmp.len()) };
        let r = write(&st, &d, off.max(0) as u64, data);
        tmp.fill(0);
        r.map_or(-1, |n| n as jint)
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_app_nicevault_desktop_NvDocs_size(mut env: JNIEnv, _: JClass, doc: JString) -> jlong {
        let d = s(&mut env, &doc);
        state().ok_or_else(ApiError::locked).and_then(|st| size(&st, &d)).map_or(-1, |n| n as jlong)
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_app_nicevault_desktop_NvDocs_flush(mut env: JNIEnv, _: JClass, doc: JString) -> jboolean {
        let d = s(&mut env, &doc);
        state().is_some_and(|st| flush(&st, &d).is_ok()) as jboolean
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_app_nicevault_desktop_NvDocs_create(mut env: JNIEnv, _: JClass, parent: JString, name: JString, dir: jboolean) -> jstring {
        let (p, n) = (s(&mut env, &parent), s(&mut env, &name));
        let r = with(|st| create(st, &p, &n, dir != 0));
        out(&mut env, r)
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_app_nicevault_desktop_NvDocs_delete(mut env: JNIEnv, _: JClass, doc: JString) -> jstring {
        let d = s(&mut env, &doc);
        let r = with(|st| delete(st, &d));
        out(&mut env, r)
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_app_nicevault_desktop_NvDocs_rename(mut env: JNIEnv, _: JClass, doc: JString, name: JString) -> jstring {
        let (d, n) = (s(&mut env, &doc), s(&mut env, &name));
        let r = with(|st| rename(st, &d, &n));
        out(&mut env, r)
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_app_nicevault_desktop_NvDocs_move(mut env: JNIEnv, _: JClass, doc: JString, target: JString) -> jstring {
        let (d, t) = (s(&mut env, &doc), s(&mut env, &target));
        let r = with(|st| move_to(st, &d, &t));
        out(&mut env, r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Locations, Store};
    use secrecy::SecretString;

    fn app(dir: &std::path::Path) -> std::sync::Arc<AppState> {
        let loc = Locations { exe_dir: Some(dir.to_path_buf()), removable: Some(vec![]), config_dir: None };
        AppState::new(Store::new(loc), Box::new(crate::NoEvents))
    }

    #[test]
    fn browse_read_write_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let st = app(tmp.path());
        let v = st.vault_create(tmp.path().join("a.nvault").to_str().unwrap(), SecretString::from("x".to_owned()), Some("fast")).unwrap();
        let r = roots(&st);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].root, v.id);
        assert_eq!(r[0].title, v.name);
        let root = r[0].doc.clone();
        assert_eq!(stat(&st, &root).unwrap().name, v.name, "the root shows the volume's name");
        assert!(stat(&st, &root).unwrap().dir);

        let dir = create(&st, &root, "Foto", true).unwrap();
        let f = create(&st, &dir, "a.txt", false).unwrap();
        assert_eq!(create(&st, &dir, "a.txt", false).map(|d| stat(&st, &d).unwrap().name).unwrap(), "a (2).txt");
        assert!(is_child(&st, &root, &f));
        assert!(is_child(&st, &dir, &f));
        assert!(!is_child(&st, &f, &dir));

        assert_eq!(open(&st, &f, true, true).unwrap(), 0);
        assert_eq!(write(&st, &f, 0, b"hello world").unwrap(), 11);
        flush(&st, &f).unwrap();
        closed();
        let mut buf = [0u8; 64];
        let n = read(&st, &f, 6, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"world");
        assert_eq!(size(&st, &f).unwrap(), 11);
        let row = stat(&st, &f).unwrap();
        assert_eq!((row.mime.as_str(), row.size, row.dir), ("text/plain", 11, false));
        assert_eq!(open(&st, &f, true, false).unwrap(), 11);
        assert_eq!(open(&st, &f, true, true).unwrap(), 0);
        assert!(open(&st, &dir, false, false).is_err(), "a folder is not opened");

        let kids = children(&st, &root).unwrap();
        assert_eq!(kids.iter().map(|k| k.name.as_str()).collect::<Vec<_>>(), ["Foto"]);
        assert_eq!(kids[0].mime, "vnd.android.document/directory");
        rename(&st, &f, "b.md").unwrap();
        assert_eq!(stat(&st, &f).unwrap().mime, "text/markdown");
        move_to(&st, &f, &root).unwrap();
        assert!(!is_child(&st, &dir, &f));
        delete(&st, &f).unwrap();
        assert!(stat(&st, &f).is_err());
        assert!(busy(), "just used");

        st.vault_lock(&v.id, "user").unwrap();
        assert!(roots(&st).is_empty());
        assert_eq!(stat(&st, &dir).unwrap_err().code, "locked");
        assert!(read(&st, &dir, 0, &mut buf).is_err());
        assert!(stat(&st, "nonsense").is_err());
    }
}
