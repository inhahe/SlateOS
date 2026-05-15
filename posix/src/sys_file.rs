//! `<sys/file.h>` — file locking operations.
//!
//! Re-exports `flock()` and its operation constants from the `file`
//! module.

pub use crate::file::flock;
pub use crate::file::LOCK_SH;
pub use crate::file::LOCK_EX;
pub use crate::file::LOCK_UN;
pub use crate::file::LOCK_NB;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lock_values() {
        assert_eq!(LOCK_SH, 1);
        assert_eq!(LOCK_EX, 2);
        assert_eq!(LOCK_UN, 8);
        assert_eq!(LOCK_NB, 4);
    }

    #[test]
    fn test_lock_nb_combinable() {
        // LOCK_NB can be OR'd with LOCK_SH or LOCK_EX.
        let shared_nb = LOCK_SH | LOCK_NB;
        assert_ne!(shared_nb, LOCK_SH);
        assert_ne!(shared_nb, LOCK_NB);

        let excl_nb = LOCK_EX | LOCK_NB;
        assert_ne!(excl_nb, LOCK_EX);
        assert_ne!(excl_nb, LOCK_NB);
    }

    #[test]
    fn test_lock_values_distinct() {
        let vals = [LOCK_SH, LOCK_EX, LOCK_UN, LOCK_NB];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j]);
            }
        }
    }

    #[test]
    fn test_flock_stub_returns_success() {
        // flock is a stub that always returns 0 (success) so
        // programs that create lock files don't fail.
        let ret = flock(-1, LOCK_SH);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(LOCK_SH, crate::file::LOCK_SH);
        assert_eq!(LOCK_EX, crate::file::LOCK_EX);
        assert_eq!(LOCK_UN, crate::file::LOCK_UN);
        assert_eq!(LOCK_NB, crate::file::LOCK_NB);
    }
}
