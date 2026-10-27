pub fn no_dumps() {
    #[cfg(unix)]
    unsafe {
        let zero = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        libc::setrlimit(libc::RLIMIT_CORE, &zero);
        #[cfg(target_os = "linux")]
        libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
    }
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn SetErrorMode(mode: u32) -> u32;
        }
        #[link(name = "wer")]
        unsafe extern "system" {
            fn WerSetFlags(flags: u32) -> i32;
        }
        unsafe {
            SetErrorMode(0x0001 | 0x0002);
            WerSetFlags(0x0001 | 0x0020);
        }
    }
}
