//! `<dlfcn.h>` — Dynamic linking constants.
//!
//! `dlopen()`, `dlsym()`, `dlclose()`, and `dlerror()` provide
//! runtime dynamic linking.  These constants define the mode
//! flags for `dlopen()` and special handle values for `dlsym()`.

// ---------------------------------------------------------------------------
// dlopen() mode flags
// ---------------------------------------------------------------------------

/// Resolve all undefined symbols before dlopen() returns.
pub const RTLD_NOW: u32 = 0x00002;
/// Defer symbol resolution until first use.
pub const RTLD_LAZY: u32 = 0x00001;
/// Symbols are available for subsequently loaded objects.
pub const RTLD_GLOBAL: u32 = 0x00100;
/// Symbols are not available for subsequently loaded objects (default).
pub const RTLD_LOCAL: u32 = 0x00000;
/// Do not unload the library during dlclose().
pub const RTLD_NODELETE: u32 = 0x01000;
/// Do not load the library; just check if it is already loaded.
pub const RTLD_NOLOAD: u32 = 0x00004;
/// Place the library's symbols ahead of global scope (deep binding).
pub const RTLD_DEEPBIND: u32 = 0x00008;

// ---------------------------------------------------------------------------
// dlsym() special handles
// ---------------------------------------------------------------------------

/// Search the default symbol scope.
pub const RTLD_DEFAULT: usize = 0;
/// Search from the next object after the calling object.
pub const RTLD_NEXT: usize = usize::MAX; // (void*)-1

// ---------------------------------------------------------------------------
// dlinfo() requests (GNU extension)
// ---------------------------------------------------------------------------

/// Get the link map (struct link_map *).
pub const RTLD_DI_LINKMAP: u32 = 2;
/// Get the library search path origin.
pub const RTLD_DI_ORIGIN: u32 = 6;
/// Get the library name (soname).
pub const RTLD_DI_SERINFO: u32 = 4;
/// Get the library search info size.
pub const RTLD_DI_SERINFOSIZE: u32 = 5;
/// Get the TLS module ID.
pub const RTLD_DI_TLS_MODID: u32 = 9;
/// Get the TLS data pointer.
pub const RTLD_DI_TLS_DATA: u32 = 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lazy_is_one() {
        assert_eq!(RTLD_LAZY, 1);
    }

    #[test]
    fn test_now_is_two() {
        assert_eq!(RTLD_NOW, 2);
    }

    #[test]
    fn test_local_is_zero() {
        assert_eq!(RTLD_LOCAL, 0);
    }

    #[test]
    fn test_mode_flags_no_conflict() {
        // LAZY and NOW should not overlap
        assert_eq!(RTLD_LAZY & RTLD_NOW, 0);
        // GLOBAL and mode flags should not overlap
        assert_eq!(RTLD_GLOBAL & RTLD_LAZY, 0);
        assert_eq!(RTLD_GLOBAL & RTLD_NOW, 0);
    }

    #[test]
    fn test_nodelete_value() {
        assert_eq!(RTLD_NODELETE, 0x01000);
    }

    #[test]
    fn test_deepbind_value() {
        assert_eq!(RTLD_DEEPBIND, 0x00008);
    }

    #[test]
    fn test_default_is_zero() {
        assert_eq!(RTLD_DEFAULT, 0);
    }

    #[test]
    fn test_next_is_max() {
        assert_eq!(RTLD_NEXT, usize::MAX);
    }

    #[test]
    fn test_dlinfo_requests_distinct() {
        let reqs = [
            RTLD_DI_LINKMAP, RTLD_DI_ORIGIN, RTLD_DI_SERINFO,
            RTLD_DI_SERINFOSIZE, RTLD_DI_TLS_MODID, RTLD_DI_TLS_DATA,
        ];
        for i in 0..reqs.len() {
            for j in (i + 1)..reqs.len() {
                assert_ne!(reqs[i], reqs[j]);
            }
        }
    }
}
