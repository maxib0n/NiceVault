pub mod host;

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

pub const HOST_NAME: &str = "app.nicevault.bridge";
pub const FIREFOX_ID: &str = "browser@nicevault.app";
pub const MAX_IN: usize = 4 << 20;
pub const MAX_TO_BROWSER: usize = 1 << 20;
pub const LAUNCH_WAIT_MS: u64 = 8000;

pub fn read_frame<R: Read>(r: &mut R, cap: usize) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    let mut got = 0;
    while got < 4 {
        match r.read(&mut len[got..]) {
            Ok(0) if got == 0 => return Ok(None),
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    let n = u32::from_ne_bytes(len) as usize;
    if n > cap {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut b = vec![0u8; n];
    r.read_exact(&mut b)?;
    Ok(Some(b))
}

pub fn write_frame<W: Write>(w: &mut W, data: &[u8]) -> io::Result<()> {
    let n = u32::try_from(data.len()).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame too large"))?;
    let mut b = Vec::with_capacity(4 + data.len());
    b.extend_from_slice(&n.to_ne_bytes());
    b.extend_from_slice(data);
    w.write_all(&b)?;
    w.flush()
}

pub fn is_host_invocation(args: &[String]) -> bool {
    let a = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    a(1).starts_with("chrome-extension://") || (a(1).ends_with(".json") && a(2) == FIREFOX_ID)
}

pub fn caller_hint(args: &[String]) -> String {
    let a = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    if a(1).starts_with("chrome-extension://") {
        a(1).to_owned()
    } else if !a(2).is_empty() && !a(2).starts_with("--") {
        a(2).to_owned()
    } else {
        String::new()
    }
}

#[cfg(unix)]
pub mod ipc {
    use std::io;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};

    pub type Stream = UnixStream;

    fn uid() -> u32 {
        unsafe { libc::getuid() }
    }

    pub fn endpoint() -> io::Result<PathBuf> {
        Ok(endpoint_dir()?.join(if cfg!(target_os = "macos") { "nicevault-bridge.sock" } else { "bridge.sock" }))
    }

    #[cfg(target_os = "macos")]
    fn endpoint_dir() -> io::Result<PathBuf> {
        let mut buf = vec![0u8; 1024];
        let n = unsafe { libc::confstr(libc::_CS_DARWIN_USER_TEMP_DIR, buf.as_mut_ptr().cast(), buf.len()) };
        if n > 0 && n <= buf.len() {
            buf.truncate(n - 1);
            if let Ok(s) = String::from_utf8(buf) {
                return Ok(PathBuf::from(s));
            }
        }
        std::env::var_os("TMPDIR").map(PathBuf::from).ok_or_else(|| io::Error::other("no per-user temp dir"))
    }

    #[cfg(not(target_os = "macos"))]
    fn endpoint_dir() -> io::Result<PathBuf> {
        match std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
            Some(d) => Ok(PathBuf::from(d).join("nicevault")),
            None => Ok(std::env::temp_dir().join(format!("nicevault-{}", uid()))),
        }
    }

    fn private_dir(dir: &Path) -> io::Result<()> {
        let mut b = std::fs::DirBuilder::new();
        b.recursive(true).mode(0o700);
        b.create(dir)?;
        let m = std::fs::symlink_metadata(dir)?;
        if !m.is_dir() || m.uid() != uid() {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "endpoint folder is not ours"));
        }
        if m.permissions().mode() & 0o077 != 0 {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub fn peer_uid(s: &UnixStream) -> io::Result<u32> {
        use std::os::fd::AsRawFd;
        let mut c = libc::ucred { pid: 0, uid: 0, gid: 0 };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let r = unsafe { libc::getsockopt(s.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, (&mut c as *mut libc::ucred).cast(), &mut len) };
        if r != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(c.uid)
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    pub fn peer_uid(s: &UnixStream) -> io::Result<u32> {
        use std::os::fd::AsRawFd;
        let (mut u, mut g) = (0, 0);
        if unsafe { libc::getpeereid(s.as_raw_fd(), &mut u, &mut g) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(u)
    }

    fn check_peer(s: &UnixStream) -> io::Result<()> {
        if peer_uid(s)? != uid() {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "peer is another user"));
        }
        Ok(())
    }

    pub fn connect() -> io::Result<Stream> {
        connect_at(&endpoint()?)
    }

    pub fn connect_at(p: &Path) -> io::Result<Stream> {
        let s = UnixStream::connect(p)?;
        check_peer(&s)?;
        Ok(s)
    }

    pub struct Listener {
        inner: UnixListener,
        path: PathBuf,
        ino: u64,
    }

    impl Listener {
        pub fn bind(path: &Path) -> io::Result<Listener> {
            if let Some(d) = path.parent() {
                private_dir(d)?;
            }
            if std::fs::symlink_metadata(path).is_ok() {
                if UnixStream::connect(path).is_ok() {
                    return Err(io::ErrorKind::AddrInUse.into());
                }
                std::fs::remove_file(path)?;
            }
            let inner = UnixListener::bind(path)?;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
            let ino = std::fs::metadata(path)?.ino();
            Ok(Listener { inner, path: path.to_owned(), ino })
        }

        pub fn accept(&self) -> io::Result<Stream> {
            loop {
                let (s, _) = self.inner.accept()?;
                if check_peer(&s).is_ok() {
                    return Ok(s);
                }
            }
        }

        pub fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            if std::fs::metadata(&self.path).is_ok_and(|m| m.ino() == self.ino) {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
}

#[cfg(windows)]
pub mod ipc {
    use std::ffi::OsString;
    use std::io::{self, Read, Write};
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::{Path, PathBuf};
    use std::ptr::{null, null_mut};
    use std::sync::Arc;
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
    };
    use windows_sys::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, GetNamedPipeServerProcessId, PIPE_READMODE_BYTE,
        PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };
    use windows_sys::Win32::System::Threading::{
        CreateEventW, GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    struct Owned(HANDLE);
    unsafe impl Send for Owned {}
    unsafe impl Sync for Owned {}
    impl Drop for Owned {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    pub fn user_sid() -> io::Result<String> {
        unsafe {
            let mut tok: HANDLE = null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut tok) == 0 {
                return Err(io::Error::last_os_error());
            }
            let tok = Owned(tok);
            let mut len = 0u32;
            GetTokenInformation(tok.0, TokenUser, null_mut(), 0, &mut len);
            let mut buf = vec![0u8; len as usize];
            if GetTokenInformation(tok.0, TokenUser, buf.as_mut_ptr().cast(), len, &mut len) == 0 {
                return Err(io::Error::last_os_error());
            }
            let user = &*(buf.as_ptr() as *const TOKEN_USER);
            let mut s: *mut u16 = null_mut();
            if ConvertSidToStringSidW(user.User.Sid, &mut s) == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut n = 0;
            while *s.add(n) != 0 {
                n += 1;
            }
            let out = OsString::from_wide(std::slice::from_raw_parts(s, n)).to_string_lossy().into_owned();
            LocalFree(s.cast());
            Ok(out)
        }
    }

    pub fn endpoint() -> io::Result<PathBuf> {
        Ok(PathBuf::from(format!(r"\\.\pipe\nicevault-bridge-{}", user_sid()?)))
    }

    pub fn image_of(pid: u32) -> Option<PathBuf> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return None;
            }
            let h = Owned(h);
            let mut buf = vec![0u16; 32768];
            let mut n = buf.len() as u32;
            if QueryFullProcessImageNameW(h.0, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut n) == 0 {
                return None;
            }
            Some(PathBuf::from(OsString::from_wide(&buf[..n as usize])))
        }
    }

    fn same_file(a: &Path, b: &Path) -> bool {
        let n = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_owned()).to_string_lossy().to_lowercase();
        n(a) == n(b)
    }

    #[derive(Clone)]
    pub struct Stream(Arc<Owned>);

    impl Stream {
        fn io(&self, buf: *mut u8, len: usize, write: bool) -> io::Result<usize> {
            unsafe {
                let ev = CreateEventW(null(), 1, 0, null());
                if ev.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let ev = Owned(ev);
                let mut ov: OVERLAPPED = std::mem::zeroed();
                ov.hEvent = ev.0;
                let len = len.min(u32::MAX as usize) as u32;
                let ok = if write {
                    WriteFile(self.0.0, buf, len, null_mut(), &mut ov)
                } else {
                    ReadFile(self.0.0, buf, len, null_mut(), &mut ov)
                };
                if ok == 0 {
                    let e = io::Error::last_os_error();
                    if e.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
                        return if !write && e.raw_os_error() == Some(109) { Ok(0) } else { Err(e) };
                    }
                }
                let mut n = 0u32;
                if GetOverlappedResult(self.0.0, &ov, &mut n, 1) == 0 {
                    let e = io::Error::last_os_error();
                    return if !write && e.raw_os_error() == Some(109) { Ok(0) } else { Err(e) };
                }
                Ok(n as usize)
            }
        }

        pub fn try_clone(&self) -> io::Result<Stream> {
            Ok(self.clone())
        }

        pub fn app_pid(&self) -> Option<u32> {
            let mut pid = 0u32;
            (unsafe { GetNamedPipeServerProcessId(self.0.0, &mut pid) } != 0).then_some(pid)
        }

        pub fn client_pid(&self) -> Option<u32> {
            let mut pid = 0u32;
            (unsafe { GetNamedPipeClientProcessId(self.0.0, &mut pid) } != 0).then_some(pid)
        }

        pub fn shutdown(&self) {}
    }

    impl Read for Stream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.io(buf.as_mut_ptr(), buf.len(), false)
        }
    }

    impl Write for Stream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.io(buf.as_ptr() as *mut u8, buf.len(), true)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    pub fn connect() -> io::Result<Stream> {
        connect_at(&endpoint()?)
    }

    pub fn connect_at(p: &Path) -> io::Result<Stream> {
        let name: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
        let h = unsafe { CreateFileW(name.as_ptr(), GENERIC_READ | GENERIC_WRITE, 0, null(), OPEN_EXISTING, FILE_FLAG_OVERLAPPED, null_mut()) };
        if h == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Ok(Stream(Arc::new(Owned(h))))
    }

    pub fn app_is(s: &Stream, expected: Option<&Path>) -> bool {
        match (expected, s.app_pid().and_then(image_of)) {
            (None, _) => true,
            (Some(e), Some(img)) => same_file(e, &img),
            (Some(_), None) => false,
        }
    }

    pub struct Listener {
        name: Vec<u16>,
        sa: SecAttr,
        next: std::sync::Mutex<Option<Stream>>,
        pub client_image: Option<PathBuf>,
        path: PathBuf,
    }

    struct SecAttr(PSECURITY_DESCRIPTOR);
    unsafe impl Send for SecAttr {}
    unsafe impl Sync for SecAttr {}
    impl Drop for SecAttr {
        fn drop(&mut self) {
            unsafe { LocalFree(self.0) };
        }
    }

    impl Listener {
        pub fn bind(path: &Path) -> io::Result<Listener> {
            let sid = user_sid()?;
            let sddl = wide(&format!("D:P(A;;GA;;;{sid})"));
            let mut sd: PSECURITY_DESCRIPTOR = null_mut();
            if unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), SDDL_REVISION_1, &mut sd, null_mut()) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let l = Listener {
                name: path.as_os_str().encode_wide().chain(Some(0)).collect(),
                sa: SecAttr(sd),
                next: std::sync::Mutex::new(None),
                client_image: None,
                path: path.to_owned(),
            };
            let first = l.instance(true)?;
            *l.next.lock().unwrap_or_else(|e| e.into_inner()) = Some(first);
            Ok(l)
        }

        fn instance(&self, first: bool) -> io::Result<Stream> {
            let sa = SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: self.sa.0, bInheritHandle: 0 };
            let open = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | if first { FILE_FLAG_FIRST_PIPE_INSTANCE } else { 0 };
            let h = unsafe {
                CreateNamedPipeW(self.name.as_ptr(), open, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS, PIPE_UNLIMITED_INSTANCES, 65536, 65536, 0, &sa)
            };
            if h == INVALID_HANDLE_VALUE {
                let e = io::Error::last_os_error();
                return Err(if first && e.raw_os_error() == Some(5) { io::ErrorKind::AddrInUse.into() } else { e });
            }
            Ok(Stream(Arc::new(Owned(h))))
        }

        pub fn accept(&self) -> io::Result<Stream> {
            loop {
                let s = match self.next.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    Some(s) => s,
                    None => self.instance(false)?,
                };
                let connected = unsafe {
                    let ev = CreateEventW(null(), 1, 0, null());
                    if ev.is_null() {
                        return Err(io::Error::last_os_error());
                    }
                    let ev = Owned(ev);
                    let mut ov: OVERLAPPED = std::mem::zeroed();
                    ov.hEvent = ev.0;
                    if ConnectNamedPipe(s.0.0, &mut ov) == 0 {
                        let e = io::Error::last_os_error().raw_os_error();
                        if e == Some(ERROR_PIPE_CONNECTED as i32) {
                            true
                        } else if e == Some(ERROR_IO_PENDING as i32) {
                            let mut n = 0u32;
                            GetOverlappedResult(s.0.0, &ov, &mut n, 1) != 0
                        } else {
                            false
                        }
                    } else {
                        true
                    }
                };
                *self.next.lock().unwrap_or_else(|e| e.into_inner()) = Some(self.instance(false)?);
                if !connected {
                    continue;
                }
                let ok = match &self.client_image {
                    None => true,
                    Some(want) => s.client_pid().and_then(image_of).is_some_and(|img| same_file(want, &img)),
                };
                if ok {
                    return Ok(s);
                }
            }
        }

        pub fn path(&self) -> &Path {
            &self.path
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Exe(PathBuf),
    MacApp(PathBuf),
}

pub fn target() -> Option<Target> {
    let exe = std::env::current_exe().ok()?;
    if cfg!(target_os = "macos")
        && let Some(bundle) = exe.parent().and_then(Path::parent).and_then(Path::parent).filter(|b| b.extension().is_some_and(|e| e == "app"))
    {
        return Some(Target::MacApp(bundle.to_owned()));
    }
    let image = std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_file());
    Some(Target::Exe(image.unwrap_or(exe)))
}

pub fn launch(t: &Target) -> io::Result<()> {
    let mut c = match t {
        Target::MacApp(app) => {
            let mut c = std::process::Command::new("/usr/bin/open");
            c.arg("-g").arg("-j").arg("-a").arg(app).arg("--args").arg("--background");
            c
        }
        Target::Exe(exe) => {
            let mut c = std::process::Command::new(exe);
            c.arg("--background");
            c
        }
    };
    c.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    detach(&mut c);
    let child = match c.spawn() {
        Ok(ch) => ch,
        #[cfg(windows)]
        Err(e) if e.raw_os_error() == Some(5) => {
            let Target::Exe(exe) = t else { return Err(e) };
            std::process::Command::new("explorer.exe").arg(exe).spawn()?
        }
        Err(e) => return Err(e),
    };
    let mut child = child;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(unix)]
fn detach(c: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        c.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
}

#[cfg(windows)]
fn detach(c: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    c.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB);
}

pub fn wait_for_app(max_ms: u64) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed().as_millis() < u128::from(max_ms) {
        if ipc::connect().is_ok() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let mut b = Vec::new();
        write_frame(&mut b, br#"{"a":1}"#).unwrap();
        write_frame(&mut b, b"").unwrap();
        let mut r = &b[..];
        assert_eq!(read_frame(&mut r, 100).unwrap().unwrap(), br#"{"a":1}"#);
        assert_eq!(read_frame(&mut r, 100).unwrap().unwrap(), b"");
        assert!(read_frame(&mut r, 100).unwrap().is_none());
    }

    #[test]
    fn frames_too_large_or_cut_are_errors() {
        let mut b = Vec::new();
        write_frame(&mut b, &[7u8; 50]).unwrap();
        assert!(read_frame(&mut &b[..], 10).is_err());
        assert!(read_frame(&mut &b[..20], 100).is_err());
        assert!(read_frame(&mut &b[..2], 100).is_err());
    }

    #[test]
    fn caller_hints() {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(caller_hint(&v(&["bridge", "chrome-extension://abc/", "--parent-window=0"])), "chrome-extension://abc/");
        assert_eq!(caller_hint(&v(&["bridge", "/home/u/.mozilla/native-messaging-hosts/app.nicevault.bridge.json", "browser@nicevault.app"])), "browser@nicevault.app");
        assert_eq!(caller_hint(&v(&["bridge"])), "");
    }

    #[test]
    fn host_invocations() {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(is_host_invocation(&v(&["nicevault", "chrome-extension://abc/"])));
        assert!(is_host_invocation(&v(&["nicevault.exe", "chrome-extension://abc/", "--parent-window=0"])));
        assert!(is_host_invocation(&v(&["nicevault", "/home/u/.mozilla/native-messaging-hosts/app.nicevault.bridge.json", "browser@nicevault.app"])));
        assert!(!is_host_invocation(&v(&["nicevault"])));
        assert!(!is_host_invocation(&v(&["nicevault", "--background"])));
        assert!(!is_host_invocation(&v(&["nicevault", "/media/stick/Personale.nvault"])));
        assert!(!is_host_invocation(&v(&["nicevault", "/tmp/other.json", "someone@else"])));
    }

    #[cfg(unix)]
    #[test]
    fn listener_is_private_and_not_stolen() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("nv").join("bridge.sock");
        let l = ipc::Listener::bind(&p).unwrap();
        assert_eq!(std::fs::metadata(p.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
        let t = std::thread::spawn(move || {
            let mut s = l.accept().unwrap();
            let f = read_frame(&mut s, 100).unwrap().unwrap();
            write_frame(&mut s, &f).unwrap();
            l
        });
        let mut c = ipc::connect_at(&p).unwrap();
        write_frame(&mut c, b"ping").unwrap();
        assert_eq!(read_frame(&mut c, 100).unwrap().unwrap(), b"ping");
        let l = t.join().unwrap();
        assert_eq!(ipc::Listener::bind(&p).err().map(|e| e.kind()), Some(io::ErrorKind::AddrInUse));
        drop(l);
        assert!(!p.exists());
        let _stale = std::os::unix::net::UnixListener::bind(&p).unwrap();
        drop(_stale);
        let l2 = ipc::Listener::bind(&p).unwrap();
        assert!(l2.path().exists());
    }
}
