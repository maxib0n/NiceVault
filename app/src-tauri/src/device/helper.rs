use super::commands::{EXIT_CHANGED, EXIT_REFUSED};
use super::{Disk, HostCtx, Plan};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub plan: Plan,
}

pub fn plan_hash(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn progress_path(plan_path: &Path) -> std::path::PathBuf {
    plan_path.with_file_name("progress.jsonl")
}

pub fn load(plan_path: &Path, hash: Option<&str>) -> Option<Request> {
    let bytes = std::fs::read(plan_path).ok()?;
    if hash? != plan_hash(&bytes) {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

pub fn recheck(plan: &Plan, now: Option<&Disk>, ctx: &HostCtx) -> Result<(), i32> {
    let Some(d) = now else { return Err(EXIT_CHANGED) };
    if !d.same_as(&plan.disk) || d.bus != plan.disk.bus || d.number != plan.disk.number || d.is_system {
        return Err(EXIT_CHANGED);
    }
    if super::refusal(d, ctx).is_some() {
        return Err(EXIT_REFUSED);
    }
    Ok(())
}

fn line(path: &Path, v: serde_json::Value) {
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{v}");
    }
}

pub fn run(plan_path: &Path, hash: Option<&str>) -> i32 {
    let progress = progress_path(plan_path);
    let Some(req) = load(plan_path, hash) else {
        line(&progress, serde_json::json!({ "exit": 2, "reason": "plan" }));
        return 2;
    };
    line(&progress, serde_json::json!({ "step": "check", "helper": std::process::id() }));
    run_os(&req, &progress)
}

#[cfg(windows)]
fn run_os(req: &Request, progress: &Path) -> i32 {
    use std::io::BufRead;
    use std::os::windows::process::CommandExt;
    let plan = &req.plan;
    let disks = super::enumerate::os_disks();
    let now = disks.iter().find(|d| d.number == plan.disk.number);
    if let Err(code) = recheck(plan, now, &HostCtx::detect(vec![])) {
        line(progress, serde_json::json!({ "exit": code, "reason": "recheck" }));
        return code;
    }
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let ps = format!(r"{root}\System32\WindowsPowerShell\v1.0\powershell.exe");
    let child = std::process::Command::new(ps)
        .args(super::commands::windows_argv(plan))
        .creation_flags(0x0800_0000)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            line(progress, serde_json::json!({ "exit": 3, "error": e.to_string() }));
            return 3;
        }
    };
    if let Some(out) = child.stdout.take() {
        for l in std::io::BufReader::new(out).lines().map_while(Result::ok) {
            match l.strip_prefix("NV-STEP ") {
                Some(s) => line(progress, serde_json::json!({ "step": s.trim() })),
                None => line(progress, serde_json::json!({ "out": l })),
            }
        }
    }
    let o = child.wait_with_output();
    let (code, err) = match o {
        Ok(o) => (o.status.code().unwrap_or(1), String::from_utf8_lossy(&o.stderr).into_owned()),
        Err(e) => (1, e.to_string()),
    };
    line(progress, serde_json::json!({ "exit": code, "stderr": err }));
    code
}

#[cfg(not(windows))]
fn run_os(_req: &Request, progress: &Path) -> i32 {
    line(progress, serde_json::json!({ "exit": 2, "error": "the device helper only runs on Windows" }));
    2
}
