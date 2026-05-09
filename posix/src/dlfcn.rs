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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn dlerror() -> *const u8 {
    let err = unsafe { *core::ptr::addr_of!(DL_ERROR) };
    if err.is_null() {
        return core::ptr::null();
    }
    // Clear error state.
    unsafe { core::ptr::addr_of_mut!(DL_ERROR).write(core::ptr::null()); }
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
#[unsafe(no_mangle)]
pub extern "C" fn dladdr(
    _addr: *const core::ffi::c_void,
    _info: *mut DlInfo,
) -> i32 {
    0 // 0 = failure for dladdr (unlike most POSIX functions).
}
