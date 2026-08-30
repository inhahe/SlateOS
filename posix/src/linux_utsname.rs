//! `<linux/utsname.h>` — System identification (kernel view).
//!
//! Defines the utsname structure returned by the `uname()` syscall,
//! containing system name, node name, kernel release/version, and
//! machine architecture.

// ---------------------------------------------------------------------------
// UTS field length
// ---------------------------------------------------------------------------

/// Length of each field in the utsname struct (including NUL).
/// Linux uses 65 bytes per field.
pub const __NEW_UTS_LEN: usize = 64;

// ---------------------------------------------------------------------------
// Re-export from sys_utsname
// ---------------------------------------------------------------------------

pub use crate::sys_utsname::Utsname;
pub use crate::sys_utsname::uname;

// ---------------------------------------------------------------------------
// Domain name length
// ---------------------------------------------------------------------------

/// Maximum domain name length.
pub const __NEW_UTS_DOMAINNAME_LEN: usize = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uts_len() {
        assert_eq!(__NEW_UTS_LEN, 64);
        assert_eq!(__NEW_UTS_DOMAINNAME_LEN, 64);
    }

    #[test]
    fn test_utsname_size() {
        // 6 fields × (__NEW_UTS_LEN + 1) = 390 bytes.  This is the kernel
        // view of the struct, so the size is the syscall ABI: our own
        // `sys_uname` builds a `[u8; 6 * 65]` and copies all of it out to the
        // caller.  This assertion read 325 while the struct had five fields,
        // which meant a caller passing one of these to `uname(2)` was handed
        // 65 bytes past the end of its buffer.
        assert_eq!(
            core::mem::size_of::<Utsname>(),
            6 * (__NEW_UTS_LEN + 1),
            "struct utsname must match the kernel's 6-field layout"
        );
        assert_eq!(core::mem::size_of::<Utsname>(), 390);
    }

    #[test]
    fn test_uname_call() {
        // SAFETY: All-zero is valid for Utsname (array of bytes).
        let mut buf: Utsname = unsafe { core::mem::zeroed() };
        let ret = uname(&mut buf);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_cross_module() {
        let _ = core::mem::size_of::<crate::sys_utsname::Utsname>();
    }
}
