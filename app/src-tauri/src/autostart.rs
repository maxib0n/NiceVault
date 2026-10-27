use std::path::{Path, PathBuf};

pub const MINIMIZED_FLAG: &str = "--minimized";
pub const LABEL: &str = "app.nicevault.desktop";
pub const RUN_VALUE: &str = "NiceVault";
#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

pub fn launched_minimized<I: IntoIterator<Item = S>, S: AsRef<std::ffi::OsStr>>(args: I) -> bool {
    args.into_iter().skip(1).any(|a| a.as_ref() == MINIMIZED_FLAG)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Desktop(PathBuf),
    LaunchAgent(PathBuf),
    RunFile(PathBuf),
    #[cfg(windows)]
    Registry,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Autostart {
    pub target: Target,
    pub exe: PathBuf,
}

impl Autostart {
    pub fn detect() -> Option<Autostart> {
        let exe = std::env::var_os("APPIMAGE").map(PathBuf::from).or_else(|| std::env::current_exe().ok())?;
        let target = match std::env::var_os("NICEVAULT_AUTOSTART_DIR") {
            Some(d) => Self::file_target(Path::new(&d)),
            None => Self::os_target()?,
        };
        Some(Autostart { target, exe })
    }

    fn file_target(dir: &Path) -> Target {
        if cfg!(windows) {
            Target::RunFile(dir.join("nicevault.run"))
        } else if cfg!(target_os = "macos") {
            Target::LaunchAgent(dir.join(format!("{LABEL}.plist")))
        } else {
            Target::Desktop(dir.join("nicevault.desktop"))
        }
    }

    #[cfg(windows)]
    fn os_target() -> Option<Target> {
        Some(Target::Registry)
    }

    #[cfg(target_os = "macos")]
    fn os_target() -> Option<Target> {
        Some(Target::LaunchAgent(dirs::home_dir()?.join("Library/LaunchAgents").join(format!("{LABEL}.plist"))))
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    fn os_target() -> Option<Target> {
        Some(Target::Desktop(dirs::config_dir()?.join("autostart/nicevault.desktop")))
    }

    pub fn exists(&self) -> bool {
        match &self.target {
            Target::Desktop(p) | Target::LaunchAgent(p) | Target::RunFile(p) => p.is_file(),
            #[cfg(windows)]
            Target::Registry => registry::get(RUN_KEY, RUN_VALUE).is_some(),
        }
    }

    pub fn apply(&self, enabled: bool, minimized: bool) -> std::io::Result<()> {
        let file = |p: &Path, body: String| -> std::io::Result<()> {
            if enabled {
                write_entry(p, body.as_bytes())
            } else {
                match std::fs::remove_file(p) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                    _ => Ok(()),
                }
            }
        };
        match &self.target {
            Target::Desktop(p) => file(p, desktop_entry(&self.exe, minimized)),
            Target::LaunchAgent(p) => file(p, launch_agent(&self.exe, minimized)),
            Target::RunFile(p) => file(p, run_command(&self.exe, minimized)),
            #[cfg(windows)]
            Target::Registry => {
                if enabled {
                    registry::set(RUN_KEY, RUN_VALUE, &run_command(&self.exe, minimized))
                } else {
                    registry::delete(RUN_KEY, RUN_VALUE)
                }
            }
        }
    }
}

fn write_entry(p: &Path, data: &[u8]) -> std::io::Result<()> {
    let dir = p.parent().ok_or_else(|| std::io::Error::other("no parent"))?;
    std::fs::create_dir_all(dir)?;
    let name = p.file_name().ok_or_else(|| std::io::Error::other("no file name"))?.to_string_lossy();
    let tmp = dir.join(format!(".{name}.{}.tmp", crate::state::random_id()));
    let r = std::fs::write(&tmp, data).and_then(|()| std::fs::rename(&tmp, p));
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r
}

fn exec_arg(s: &str) -> String {
    let mut q = String::from("\"");
    for c in s.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                q.push('\\');
                q.push(c);
            }
            '%' => q.push_str("%%"),
            '\n' | '\r' => q.push(' '),
            c => q.push(c),
        }
    }
    q.push('"');
    q.replace('\\', "\\\\")
}

pub fn desktop_entry(exe: &Path, minimized: bool) -> String {
    let mut exec = exec_arg(&exe.to_string_lossy());
    if minimized {
        exec.push(' ');
        exec.push_str(MINIMIZED_FLAG);
    }
    format!(
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName=NiceVault\nComment=Encrypted volumes\nExec={exec}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
    )
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn app_bundle(exe: &Path) -> Option<&Path> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    let ok = macos.file_name()? == "MacOS" && contents.file_name()? == "Contents" && app.extension()? == "app";
    ok.then_some(app)
}

pub fn launch_agent(exe: &Path, minimized: bool) -> String {
    let mut args: Vec<String> = match app_bundle(exe) {
        Some(app) => vec!["/usr/bin/open".into(), "-a".into(), app.to_string_lossy().into_owned()],
        None => vec![exe.to_string_lossy().into_owned()],
    };
    if minimized {
        if args.len() > 1 {
            args.push("--args".into());
        }
        args.push(MINIMIZED_FLAG.into());
    }
    let args: String = args.iter().map(|a| format!("\t\t<string>{}</string>\n", xml(a))).collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n\
<dict>\n\
\t<key>Label</key>\n\
\t<string>{LABEL}</string>\n\
\t<key>ProgramArguments</key>\n\
\t<array>\n{args}\t</array>\n\
\t<key>RunAtLoad</key>\n\
\t<true/>\n\
\t<key>ProcessType</key>\n\
\t<string>Interactive</string>\n\
\t<key>LimitLoadToSessionType</key>\n\
\t<string>Aqua</string>\n\
</dict>\n\
</plist>\n"
    )
}

pub fn run_command(exe: &Path, minimized: bool) -> String {
    let mut s = format!("\"{}\"", exe.to_string_lossy());
    if minimized {
        s.push(' ');
        s.push_str(MINIMIZED_FLAG);
    }
    s
}

#[cfg(windows)]
mod registry {
    use std::ffi::c_void;

    type Hkey = isize;
    const HKEY_CURRENT_USER: Hkey = 0x8000_0001u32 as i32 as isize;
    const REG_SZ: u32 = 1;
    const RRF_RT_REG_SZ: u32 = 0x2;
    const ERROR_SUCCESS: i32 = 0;
    const ERROR_FILE_NOT_FOUND: i32 = 2;

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegSetKeyValueW(key: Hkey, sub: *const u16, name: *const u16, ty: u32, data: *const c_void, len: u32) -> i32;
        fn RegDeleteKeyValueW(key: Hkey, sub: *const u16, name: *const u16) -> i32;
        fn RegGetValueW(key: Hkey, sub: *const u16, name: *const u16, flags: u32, ty: *mut u32, data: *mut c_void, len: *mut u32) -> i32;
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn err(code: i32) -> std::io::Error {
        std::io::Error::from_raw_os_error(code)
    }

    pub fn set(key: &str, name: &str, value: &str) -> std::io::Result<()> {
        let (k, n, v) = (wide(key), wide(name), wide(value));
        let r = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr(), REG_SZ, v.as_ptr().cast(), (v.len() * 2) as u32) };
        if r == ERROR_SUCCESS { Ok(()) } else { Err(err(r)) }
    }

    pub fn delete(key: &str, name: &str) -> std::io::Result<()> {
        let (k, n) = (wide(key), wide(name));
        let r = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr()) };
        if r == ERROR_SUCCESS || r == ERROR_FILE_NOT_FOUND { Ok(()) } else { Err(err(r)) }
    }

    pub fn get(key: &str, name: &str) -> Option<String> {
        let (k, n) = (wide(key), wide(name));
        let mut len = 0u32;
        unsafe {
            if RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), std::ptr::null_mut(), &mut len) != ERROR_SUCCESS {
                return None;
            }
            let mut buf = vec![0u16; (len as usize).div_ceil(2)];
            if RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut len) != ERROR_SUCCESS {
                return None;
            }
            let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            Some(String::from_utf16_lossy(&buf[..end]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_parsing_skips_the_program() {
        assert!(launched_minimized(["nicevault", "--minimized"]));
        assert!(launched_minimized(["nicevault", "-x", "--minimized"]));
        assert!(!launched_minimized(["--minimized"]));
        assert!(!launched_minimized(["nicevault", "--minimised", "minimized"]));
    }

    #[test]
    fn desktop_entry_quotes_the_path() {
        let e = desktop_entry(Path::new("/opt/Nice Vault/nicevault"), true);
        assert!(e.starts_with("[Desktop Entry]\n"));
        assert!(e.contains("\nExec=\"/opt/Nice Vault/nicevault\" --minimized\n"), "{e}");
        assert!(e.contains("\nType=Application\n") && e.contains("\nName=NiceVault\n"));
        let e = desktop_entry(Path::new("/home/a$b/100%/x\"y\\z"), false);
        assert!(e.contains("\nExec=\"/home/a\\\\$b/100%%/x\\\\\"y\\\\\\\\z\"\n"), "{e}");
    }

    #[test]
    fn launch_agent_starts_the_bundle_or_the_binary() {
        let p = launch_agent(Path::new("/Applications/Nice & Co.app/Contents/MacOS/nicevault"), true);
        assert!(p.contains("<string>app.nicevault.desktop</string>"));
        assert!(p.contains(
            "<string>/usr/bin/open</string>\n\t\t<string>-a</string>\n\t\t<string>/Applications/Nice &amp; Co.app</string>\n\t\t<string>--args</string>\n\t\t<string>--minimized</string>\n"
        ), "{p}");
        assert!(p.contains("<key>RunAtLoad</key>\n\t<true/>"));
        let p = launch_agent(Path::new("/Users/me/bin/nicevault"), false);
        assert!(p.contains("<array>\n\t\t<string>/Users/me/bin/nicevault</string>\n\t</array>"), "{p}");
        assert!(!p.contains("--minimized"));
    }

    #[test]
    fn run_value_is_the_quoted_path_and_the_flag() {
        assert_eq!(run_command(Path::new(r"C:\Program Files\NiceVault\NiceVault.exe"), true), r#""C:\Program Files\NiceVault\NiceVault.exe" --minimized"#);
        assert_eq!(run_command(Path::new(r"C:\NV\nv.exe"), false), r#""C:\NV\nv.exe""#);
    }

    #[test]
    fn apply_writes_rewrites_and_removes_each_kind() {
        let t = tempfile::tempdir().unwrap();
        for (target, p) in [
            (Target::Desktop as fn(PathBuf) -> Target, t.path().join("autostart/nicevault.desktop")),
            (Target::LaunchAgent, t.path().join("LaunchAgents/app.nicevault.desktop.plist")),
            (Target::RunFile, t.path().join("run/nicevault.run")),
        ] {
            let a = Autostart { target: target(p.clone()), exe: PathBuf::from("/opt/nv/nicevault") };
            let p = &p;
            assert!(!a.exists());
            a.apply(false, false).unwrap();
            a.apply(true, false).unwrap();
            assert!(a.exists());
            assert!(!std::fs::read_to_string(p).unwrap().contains(MINIMIZED_FLAG));
            a.apply(true, true).unwrap();
            assert!(std::fs::read_to_string(p).unwrap().contains(MINIMIZED_FLAG));
            a.apply(false, true).unwrap();
            assert!(!a.exists());
            assert_eq!(std::fs::read_dir(p.parent().unwrap()).unwrap().count(), 0);
        }
    }
}
