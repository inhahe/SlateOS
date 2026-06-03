//! Dynamic linking stubs.
//!
//! Implements `dlopen`, `dlsym`, `dlclose`, `dlerror` stubs.
//!
//! Our OS doesn't have a dynamic linker yet — all programs are
//! statically linked.  These stubs allow programs that optionally
//! probe for dynamic libraries to compile and run (they'll just
//! get "not supported" errors).
//!
//! ## Limitations
//!
//! - All `dlopen` calls return NULL.
//! - `dlerror` always returns a descriptive error message.

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Resolve symbols lazily.
pub const RTLD_LAZY: i32 = 1;
/// Resolve symbols immediately.
pub const RTLD_NOW: i32 = 2;
/// Make symbols available globally.
pub const RTLD_GLOBAL: i32 = 0x100;
/// Make symbols available only locally.
pub const RTLD_LOCAL: i32 = 0;
/// Return handle for the main program.
pub const RTLD_DEFAULT: *mut u8 = core::ptr::null_mut();

// ---------------------------------------------------------------------------
// Error state
// ---------------------------------------------------------------------------

/// Static error message for dlerror.
static mut DL_ERROR: *const u8 = core::ptr::null();

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// Open a shared library.
///
/// Stub: always returns NULL (dynamic linking not supported).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dlopen(_filename: *const u8, _flags: i32) -> *mut u8 {
    // SAFETY: Single-threaded access.
    unsafe {
        core::ptr::addr_of_mut!(DL_ERROR)
            .write(c"dynamic linking not supported".as_ptr().cast::<u8>());
    }
    core::ptr::null_mut()
}

/// Look up a symbol in a shared library.
///
/// Stub: always returns NULL.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dlsym(_handle: *mut u8, _symbol: *const u8) -> *mut u8 {
    unsafe {
        core::ptr::addr_of_mut!(DL_ERROR)
            .write(c"dynamic linking not supported".as_ptr().cast::<u8>());
    }
    core::ptr::null_mut()
}

/// Close a shared library handle.
///
/// Stub: always returns -1.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dlclose(_handle: *mut u8) -> i32 {
    unsafe {
        core::ptr::addr_of_mut!(DL_ERROR)
            .write(c"dynamic linking not supported".as_ptr().cast::<u8>());
    }
    -1
}

/// Return a human-readable error message from the last dl* call.
///
/// Returns the error string and clears the error state.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dlerror() -> *const u8 {
    let err = unsafe { *core::ptr::addr_of!(DL_ERROR) };
    if err.is_null() {
        return core::ptr::null();
    }
    // Clear error state.
    unsafe {
        core::ptr::addr_of_mut!(DL_ERROR).write(core::ptr::null());
    }
    err
}

/// Information about a dynamically loaded symbol.
#[repr(C)]
pub struct DlInfo {
    /// Pathname of the shared object.
    pub dli_fname: *const u8,
    /// Address at which the shared object is loaded.
    pub dli_fbase: *mut core::ffi::c_void,
    /// Name of the nearest symbol.
    pub dli_sname: *const u8,
    /// Exact value of the nearest symbol.
    pub dli_saddr: *mut core::ffi::c_void,
}

/// Get information about a dynamically loaded symbol.
///
/// Stub: returns 0 (failure) since we don't support dynamic linking.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dladdr(_addr: *const core::ffi::c_void, _info: *mut DlInfo) -> i32 {
    0 // 0 = failure for dladdr (unlike most POSIX functions).
}

// ---------------------------------------------------------------------------
// dl_iterate_phdr — iterate over loaded shared objects
// ---------------------------------------------------------------------------

/// Iterate over program headers of loaded shared objects.
///
/// Stub: calls `callback` once with NULL info (for the main executable),
/// then returns 0.  Since we don't support dynamic linking, there are
/// no shared objects to iterate over.
///
/// Some libraries (libgcc, libunwind) call this to find exception
/// handling tables.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dl_iterate_phdr(
    _callback: Option<extern "C" fn(*mut u8, usize, *mut u8) -> i32>,
    _data: *mut u8,
) -> i32 {
    // No shared objects to iterate — return immediately.
    0
}

// ---------------------------------------------------------------------------
// __tls_get_addr — thread-local storage access
// ---------------------------------------------------------------------------

/// TLS access for the GNU TLS model.
///
/// Stub: returns NULL.  We don't support the GD or LD TLS models
/// (which require dynamic linking).  Programs using `__thread` or
/// `thread_local` with the initial-exec model don't call this
/// function — they use `%fs`-relative addressing directly.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __tls_get_addr(_ti: *mut u8) -> *mut u8 {
    core::ptr::null_mut()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Constants match Linux/glibc --

    #[test]
    fn test_rtld_constants() {
        assert_eq!(RTLD_LAZY, 1);
        assert_eq!(RTLD_NOW, 2);
        assert_eq!(RTLD_GLOBAL, 0x100);
        assert_eq!(RTLD_LOCAL, 0);
        assert!(RTLD_DEFAULT.is_null());
    }

    // -- dlopen always returns NULL --

    #[test]
    fn test_dlopen_returns_null() {
        let handle = dlopen(b"libfoo.so\0".as_ptr(), RTLD_LAZY);
        assert!(handle.is_null());
    }

    #[test]
    fn test_dlopen_null_filename() {
        // dlopen(NULL, ...) would normally return main program handle
        let handle = dlopen(core::ptr::null(), RTLD_NOW);
        assert!(handle.is_null());
    }

    // -- dlsym always returns NULL --

    #[test]
    fn test_dlsym_returns_null() {
        let sym = dlsym(core::ptr::null_mut(), b"some_function\0".as_ptr());
        assert!(sym.is_null());
    }

    // -- dlclose always returns -1 --

    #[test]
    fn test_dlclose_returns_error() {
        assert_eq!(dlclose(core::ptr::null_mut()), -1);
    }

    // -- dlerror returns error after dl* call then clears --

    #[test]
    fn test_dlerror_after_dlopen() {
        // Clear any prior error.
        let _ = dlerror();

        // Trigger an error.
        let _ = dlopen(b"libfoo.so\0".as_ptr(), RTLD_LAZY);

        let err = dlerror();
        assert!(!err.is_null());
        // Should contain "not supported"
        let first_byte = unsafe { *err };
        assert_eq!(first_byte, b'd'); // "dynamic linking not supported"
    }

    #[test]
    fn test_dlerror_clears_after_read() {
        // Trigger an error.
        let _ = dlopen(b"libfoo.so\0".as_ptr(), RTLD_LAZY);

        // First call should return the error.
        let err1 = dlerror();
        assert!(!err1.is_null());

        // Second call should return null (error cleared).
        let err2 = dlerror();
        assert!(err2.is_null());
    }

    #[test]
    fn test_dlerror_after_dlsym() {
        let _ = dlerror(); // Clear
        let _ = dlsym(core::ptr::null_mut(), b"func\0".as_ptr());
        let err = dlerror();
        assert!(!err.is_null());
    }

    #[test]
    fn test_dlerror_after_dlclose() {
        let _ = dlerror(); // Clear
        let _ = dlclose(core::ptr::null_mut());
        let err = dlerror();
        assert!(!err.is_null());
    }

    // -- dladdr returns 0 (failure) --

    #[test]
    fn test_dladdr_returns_zero() {
        let mut info = DlInfo {
            dli_fname: core::ptr::null(),
            dli_fbase: core::ptr::null_mut(),
            dli_sname: core::ptr::null(),
            dli_saddr: core::ptr::null_mut(),
        };
        assert_eq!(dladdr(core::ptr::null(), &raw mut info), 0);
    }

    // -- dl_iterate_phdr returns 0 --

    #[test]
    fn test_dl_iterate_phdr_returns_zero() {
        assert_eq!(dl_iterate_phdr(None, core::ptr::null_mut()), 0);
    }

    // -- __tls_get_addr returns NULL --

    #[test]
    fn test_tls_get_addr_returns_null() {
        assert!(__tls_get_addr(core::ptr::null_mut()).is_null());
    }

    // -- DlInfo layout --

    #[test]
    fn test_dlinfo_size() {
        // 4 pointers on x86_64 = 32 bytes
        assert_eq!(core::mem::size_of::<DlInfo>(), 32);
    }

    #[test]
    fn test_dlinfo_alignment() {
        // Pointers are 8-byte aligned on x86_64
        assert_eq!(core::mem::align_of::<DlInfo>(), 8);
    }

    // -- dlopen with various flags --

    #[test]
    fn test_dlopen_rtld_now() {
        assert!(dlopen(b"libfoo.so\0".as_ptr(), RTLD_NOW).is_null());
    }

    #[test]
    fn test_dlopen_rtld_global() {
        assert!(dlopen(b"libfoo.so\0".as_ptr(), RTLD_LAZY | RTLD_GLOBAL).is_null());
    }

    // -- dlsym with various symbols --

    #[test]
    fn test_dlsym_null_symbol() {
        assert!(dlsym(core::ptr::null_mut(), core::ptr::null()).is_null());
    }

    #[test]
    fn test_dlsym_rtld_default() {
        assert!(dlsym(RTLD_DEFAULT, b"printf\0".as_ptr()).is_null());
    }

    // -- dlclose with non-null handle --

    #[test]
    fn test_dlclose_nonzero_handle() {
        assert_eq!(dlclose(1usize as *mut u8), -1);
    }

    // -- dlerror sequence tests --

    #[test]
    fn test_dlerror_after_dlsym_then_clear() {
        let _ = dlerror(); // clear
        let _ = dlsym(core::ptr::null_mut(), b"foo\0".as_ptr());
        let err = dlerror();
        assert!(!err.is_null());
        let err2 = dlerror();
        assert!(err2.is_null());
    }

    #[test]
    fn test_dlerror_message_starts_with_d() {
        let _ = dlerror(); // clear
        let _ = dlclose(core::ptr::null_mut());
        let err = dlerror();
        assert!(!err.is_null());
        assert_eq!(unsafe { *err }, b'd'); // "dynamic linking..."
    }

    // -- dladdr with non-null info fields --

    #[test]
    fn test_dladdr_with_address() {
        let mut info = DlInfo {
            dli_fname: core::ptr::null(),
            dli_fbase: core::ptr::null_mut(),
            dli_sname: core::ptr::null(),
            dli_saddr: core::ptr::null_mut(),
        };
        // Using a non-null address — should still return 0
        assert_eq!(dladdr(0x1000 as *const core::ffi::c_void, &raw mut info), 0);
    }

    // -- dl_iterate_phdr with callback --

    extern "C" fn dummy_phdr_callback(_info: *mut u8, _size: usize, _data: *mut u8) -> i32 {
        42 // should not be called
    }

    #[test]
    fn test_dl_iterate_phdr_with_callback() {
        let ret = dl_iterate_phdr(Some(dummy_phdr_callback), core::ptr::null_mut());
        assert_eq!(ret, 0);
    }

    // -- __tls_get_addr with non-null arg --

    #[test]
    fn test_tls_get_addr_nonzero_arg() {
        let mut ti: u8 = 0;
        assert!(__tls_get_addr(&raw mut ti).is_null());
    }

    // -- RTLD constants are distinct --

    #[test]
    fn test_rtld_lazy_now_disjoint() {
        assert_eq!(RTLD_LAZY & RTLD_NOW, 0);
    }

    #[test]
    fn test_rtld_global_local_disjoint() {
        // RTLD_LOCAL is 0, so any value & 0 == 0
        assert_eq!(RTLD_GLOBAL & RTLD_LOCAL, 0);
    }
}
