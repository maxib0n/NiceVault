use crate::{LAUNCH_WAIT_MS, MAX_IN, MAX_TO_BROWSER, ipc, read_frame, write_frame};
use serde_json::{Value, json};
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

type Out = Arc<Mutex<io::Stdout>>;

fn send(out: &Out, v: &Value) {
    let mut o = out.lock().unwrap_or_else(|e| e.into_inner());
    let _ = write_frame(&mut *o, v.to_string().as_bytes());
}

fn os() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

struct App {
    stream: ipc::Stream,
    closed: Arc<AtomicBool>,
}

fn connect(out: &Out, caller: &str) -> io::Result<App> {
    let mut s = ipc::connect()?;
    #[cfg(windows)]
    {
        let want = std::env::current_exe().ok();
        if !ipc::app_is(&s, want.as_deref()) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "the pipe is not served by NiceVault"));
        }
    }
    let hello = json!({ "bridge_hello": { "caller": caller, "version": env!("CARGO_PKG_VERSION"), "pid": std::process::id() } });
    write_frame(&mut s, hello.to_string().as_bytes())?;
    let closed = Arc::new(AtomicBool::new(false));
    let (mut r, out2, c2) = (s.try_clone()?, out.clone(), closed.clone());
    std::thread::spawn(move || {
        loop {
            match read_frame(&mut r, MAX_IN) {
                Ok(Some(f)) if f.len() <= MAX_TO_BROWSER => {
                    let mut o = out2.lock().unwrap_or_else(|e| e.into_inner());
                    if write_frame(&mut *o, &f).is_err() {
                        std::process::exit(0);
                    }
                }
                Ok(Some(_)) => eprintln!("nicevault (browser host): dropped an oversized frame from the app"),
                _ => break,
            }
        }
        c2.store(true, Ordering::SeqCst);
        send(&out2, &json!({ "bridge": "app_closed" }));
    });
    Ok(App { stream: s, closed })
}

fn launch() -> Value {
    if ipc::connect().is_ok() {
        return json!({ "bridge": "launched", "ok": true, "already": true });
    }
    let Some(target) = crate::target() else {
        return json!({ "bridge": "launched", "ok": false, "reason": "no_app" });
    };
    if let Err(e) = crate::launch(&target) {
        eprintln!("nicevault (browser host): launch failed: {e}");
        return json!({ "bridge": "launched", "ok": false, "reason": "spawn" });
    }
    if crate::wait_for_app(LAUNCH_WAIT_MS) {
        json!({ "bridge": "launched", "ok": true })
    } else {
        json!({ "bridge": "launched", "ok": false, "reason": "timeout" })
    }
}

pub fn run(args: &[String]) -> i32 {
    let caller = crate::caller_hint(args);
    let out: Out = Arc::new(Mutex::new(io::stdout()));
    let mut input = io::stdin().lock();
    let mut app: Option<App> = None;
    while let Ok(Some(frame)) = read_frame(&mut input, MAX_IN) {
        if let Ok(Value::Object(m)) = serde_json::from_slice::<Value>(&frame)
            && let Some(cmd) = m.get("bridge").and_then(Value::as_str)
        {
            let reply = match cmd {
                "hello" => json!({ "bridge": "state", "app": ipc::connect().is_ok(), "version": env!("CARGO_PKG_VERSION"), "os": os() }),
                "launch" => launch(),
                _ => json!({ "bridge": "unsupported" }),
            };
            send(&out, &reply);
            continue;
        }
        if app.as_ref().is_some_and(|a| a.closed.load(Ordering::SeqCst)) {
            app = None;
        }
        if app.is_none() {
            match connect(&out, &caller) {
                Ok(a) => app = Some(a),
                Err(_) => {
                    send(&out, &json!({ "bridge": "not_running" }));
                    continue;
                }
            }
        }
        let a = app.as_mut().expect("connected");
        #[cfg(windows)]
        if let Some(pid) = a.stream.app_pid() {
            unsafe { windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(pid) };
        }
        if write_frame(&mut a.stream, &frame).is_err() {
            app = None;
            send(&out, &json!({ "bridge": "not_running" }));
        }
    }
    let _ = io::stdout().flush();
    0
}
