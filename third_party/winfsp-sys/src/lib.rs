#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(clippy::useless_transmute)]
#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::missing_safety_doc)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

#[cfg(feature = "docsrs")]
mod bindings;

#[cfg(feature = "docsrs")]
pub use bindings::*;

#[allow(non_camel_case_types)]
pub type FILE_ACCESS_RIGHTS = u32;
#[allow(non_camel_case_types)]
pub type FILE_FLAGS_AND_ATTRIBUTES = u32;

/// NiceVault patch: MinGW runtime binding of `winfsp-x64.dll` (see build.rs, `write_gnu_thunks`).
#[cfg(all(windows, target_env = "gnu"))]
mod gnu_bind {
    use core::ffi::c_void;

    include!(concat!(env!("OUT_DIR"), "/gnu_thunks.rs"));

    unsafe extern "C" {
        static mut __nvwfsp_slots: [usize; NAMES.len()];
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    }

    /// Point every WinFsp entry point at `module`'s export (from `LoadLibraryW` of
    /// winfsp-x64.dll). Returns the exports the DLL lacks; calling one of those returns
    /// STATUS_PROCEDURE_NOT_FOUND.
    ///
    /// # Safety
    /// `module` must be a loaded winfsp DLL that stays loaded for the process lifetime, and no
    /// WinFsp function may run concurrently with this call.
    pub unsafe fn nv_bind_module(module: *mut c_void) -> Vec<&'static str> {
        let mut missing = Vec::new();
        for (i, name) in NAMES.iter().enumerate() {
            // SAFETY: NUL-terminated name; slot index in bounds.
            let p = unsafe { GetProcAddress(module, name.as_ptr()) };
            if p.is_null() {
                missing.push(&name[..name.len() - 1]);
            } else {
                unsafe { core::ptr::addr_of_mut!(__nvwfsp_slots[i]).write_volatile(p as usize) };
            }
        }
        missing
    }
}

#[cfg(all(windows, target_env = "gnu"))]
pub use gnu_bind::nv_bind_module;
