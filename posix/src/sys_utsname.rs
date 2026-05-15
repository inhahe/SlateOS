//! `<sys/utsname.h>` — system name structure definitions.
//!
//! Re-exports the `Utsname` structure and `uname()` function from
//! the `utsname` module.

pub use crate::utsname::Utsname;
pub use crate::utsname::uname;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_utsname_struct_size() {
        assert!(core::mem::size_of::<Utsname>() > 0);
    }

    #[test]
    fn test_uname_succeeds() {
        let mut buf: Utsname = unsafe { core::mem::zeroed() };
        let ret = uname(&mut buf);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_uname_sysname_not_empty() {
        let mut buf: Utsname = unsafe { core::mem::zeroed() };
        uname(&mut buf);
        // sysname should be set (not all zeros).
        assert_ne!(buf.sysname[0], 0);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(
            core::mem::size_of::<Utsname>(),
            core::mem::size_of::<crate::utsname::Utsname>()
        );
    }
}
