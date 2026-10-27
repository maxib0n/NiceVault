#[cfg(unix)]
#[test]
fn children_inherit_no_core_dumps() {
    nicevault_app::hardening::no_dumps();
    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg("ulimit -c; ulimit -Hc; ulimit -c unlimited 2>/dev/null && echo raised || echo refused")
        .output()
        .expect("sh");
    let s = String::from_utf8_lossy(&out.stdout);
    assert_eq!(s.split_whitespace().collect::<Vec<_>>(), ["0", "0", "refused"], "child limits: {s}");
}

#[cfg(target_os = "linux")]
#[test]
fn process_is_not_dumpable() {
    nicevault_app::hardening::no_dumps();
    assert_eq!(unsafe { libc::prctl(libc::PR_GET_DUMPABLE) }, 0);
}
