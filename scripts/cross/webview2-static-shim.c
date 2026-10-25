/*
 * MSVC runtime pieces needed to link Microsoft's WebView2LoaderStatic.lib (MSVC objects)
 * into a MinGW (x86_64-pc-windows-gnu) executable, so NiceVault.exe does not need
 * WebView2Loader.dll next to it. Compiled by scripts/cross/zig-mingw-link.sh with zig cc.
 *
 * The loader objects reference: C++ operator new/delete (MSVC mangling), std::nothrow,
 * MSVC "magic statics" (_Init_thread_*), the CFG dispatch pointer and _purecall.
 * Everything else (memcpy, wcs*, _ultow_s, __security_cookie, _tls_index, Win32 imports)
 * comes from the MinGW CRT and import libraries.
 */
#include <windows.h>
#include <limits.h>
#include <stdlib.h>

/* ---- operator new / delete ------------------------------------------------------------ */
/* void* operator new(size_t, const std::nothrow_t&) */
void *nv_new_nothrow(size_t n, const void *tag) __asm__("??2@YAPEAX_KAEBUnothrow_t@std@@@Z");
void *nv_new_nothrow(size_t n, const void *tag) { (void)tag; return malloc(n ? n : 1); }
/* void operator delete(void*, size_t) */
void nv_delete_sized(void *p, size_t n) __asm__("??3@YAXPEAX_K@Z");
void nv_delete_sized(void *p, size_t n) { (void)n; free(p); }
/* void* operator new[](size_t)  (MSVC throws on failure; we abort instead) */
void *nv_new_array(size_t n) __asm__("??_U@YAPEAX_K@Z");
void *nv_new_array(size_t n) { void *p = malloc(n ? n : 1); if (!p) abort(); return p; }
/* void operator delete[](void*) */
void nv_delete_array(void *p) __asm__("??_V@YAXPEAX@Z");
void nv_delete_array(void *p) { free(p); }
/* const std::nothrow_t std::nothrow */
const char nv_std_nothrow __asm__("?nothrow@std@@3Unothrow_t@1@B") = 0;

/* ---- thread-safe local statics (vcruntime thread_safe_statics.cpp semantics) ----------- */
#define NV_UNINITIALIZED 0
#define NV_BEING_INITIALIZED (-1)
int _Init_global_epoch = INT_MIN;
/* Read by the MSVC objects through _tls_index + SECREL, so it must be native TLS. */
__thread int _Init_thread_epoch = INT_MIN;
static SRWLOCK nv_init_lock = SRWLOCK_INIT;
static CONDITION_VARIABLE nv_init_cv = CONDITION_VARIABLE_INIT;

void _Init_thread_header(int *once) {
    AcquireSRWLockExclusive(&nv_init_lock);
    if (*once == NV_UNINITIALIZED) {
        *once = NV_BEING_INITIALIZED;
    } else {
        while (*once == NV_BEING_INITIALIZED) {
            SleepConditionVariableSRW(&nv_init_cv, &nv_init_lock, 100, 0);
            if (*once == NV_UNINITIALIZED) {
                *once = NV_BEING_INITIALIZED;
                ReleaseSRWLockExclusive(&nv_init_lock);
                return;
            }
        }
        _Init_thread_epoch = _Init_global_epoch;
    }
    ReleaseSRWLockExclusive(&nv_init_lock);
}

void _Init_thread_footer(int *once) {
    AcquireSRWLockExclusive(&nv_init_lock);
    ++_Init_global_epoch;
    *once = _Init_global_epoch;
    _Init_thread_epoch = _Init_global_epoch;
    ReleaseSRWLockExclusive(&nv_init_lock);
    WakeAllConditionVariable(&nv_init_cv);
}

void _Init_thread_abort(int *once) {
    AcquireSRWLockExclusive(&nv_init_lock);
    *once = NV_UNINITIALIZED;
    ReleaseSRWLockExclusive(&nv_init_lock);
    WakeAllConditionVariable(&nv_init_cv);
}

/* ---- Control Flow Guard dispatch (no-op: jump to the target in rax) ------------------- */
__asm__(".text\n"
        ".globl nv_guard_dispatch_icall_nop\n"
        "nv_guard_dispatch_icall_nop:\n"
        "    jmp *%rax\n");
extern void nv_guard_dispatch_icall_nop(void);
__attribute__((weak)) void *__guard_dispatch_icall_fptr = (void *)&nv_guard_dispatch_icall_nop;

/* ---- pure virtual call ----------------------------------------------------------------- */
__attribute__((weak)) int _purecall(void) { abort(); }

/* ---- /GS stack cookie (the MinGW CRT zig links does not provide it) -------------------- */
__attribute__((weak)) UINT_PTR __security_cookie = 0x00002B992DDFA232ULL;

__attribute__((weak)) void __security_check_cookie(UINT_PTR cookie) {
    if (cookie != __security_cookie) __fastfail(2 /* FAST_FAIL_STACK_COOKIE_CHECK_FAILURE */);
}

/* Randomise the cookie before main, i.e. before any loader function has used it. */
__attribute__((constructor)) static void nv_security_init_cookie(void) {
    LARGE_INTEGER pc;
    FILETIME ft;
    QueryPerformanceCounter(&pc);
    GetSystemTimeAsFileTime(&ft);
    UINT_PTR c = (UINT_PTR)pc.QuadPart ^ ((UINT_PTR)ft.dwHighDateTime << 32 | ft.dwLowDateTime)
               ^ (UINT_PTR)GetCurrentProcessId() << 16 ^ (UINT_PTR)GetCurrentThreadId()
               ^ (UINT_PTR)&c;
    c &= 0x0000FFFFFFFFFFFFULL;
    if (c == 0 || c == 0x00002B992DDFA232ULL) c = 0x00002B992DDFA233ULL;
    __security_cookie = c;
}
