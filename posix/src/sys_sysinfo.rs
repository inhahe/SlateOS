//! `<sys/sysinfo.h>` — system information.
//!
//! Re-exports `sysinfo()` and `Sysinfo` from the `unistd` module.

pub use crate::unistd::Sysinfo;
pub use crate::unistd::sysinfo;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysinfo_struct_size() {
        assert!(core::mem::size_of::<Sysinfo>() > 0);
    }

    /// Zero-initialize a Sysinfo by writing zeros over it.
    fn zero_sysinfo() -> Sysinfo {
        // SAFETY: Sysinfo is repr(C) and all-zeros is valid.
        unsafe { core::mem::zeroed() }
    }

    #[test]
    fn test_sysinfo_callable() {
        let mut info = zero_sysinfo();
        let ret = sysinfo(&mut info);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_sysinfo_values() {
        let mut info = zero_sysinfo();
        sysinfo(&mut info);
        // After calling sysinfo, totalram should be non-zero.
        assert!(info.totalram > 0, "totalram should be > 0");
        assert!(info.mem_unit > 0, "mem_unit should be > 0");
        assert!(info.procs > 0, "procs should be > 0");
    }

    #[test]
    fn test_sysinfo_null_returns_error() {
        let ret = sysinfo(core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_sysinfo_freeram_le_totalram() {
        let mut info = zero_sysinfo();
        sysinfo(&mut info);
        assert!(
            info.freeram <= info.totalram,
            "freeram ({}) should be <= totalram ({})",
            info.freeram,
            info.totalram
        );
    }
}
