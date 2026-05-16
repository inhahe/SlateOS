//! `<linux/fadvise.h>` — file access advice constants.
//!
//! Re-exports the POSIX_FADV_* constants and `posix_fadvise()`
//! from the `file` module.

pub use crate::file::POSIX_FADV_NORMAL;
pub use crate::file::POSIX_FADV_SEQUENTIAL;
pub use crate::file::POSIX_FADV_RANDOM;
pub use crate::file::POSIX_FADV_NOREUSE;
pub use crate::file::POSIX_FADV_WILLNEED;
pub use crate::file::POSIX_FADV_DONTNEED;
pub use crate::file::posix_fadvise;
pub use crate::file::fadvise64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fadv_constants() {
        assert_eq!(POSIX_FADV_NORMAL, 0);
        assert_ne!(POSIX_FADV_SEQUENTIAL, POSIX_FADV_RANDOM);
    }

    #[test]
    fn test_fadv_values_distinct() {
        let vals = [
            POSIX_FADV_NORMAL, POSIX_FADV_RANDOM, POSIX_FADV_SEQUENTIAL,
            POSIX_FADV_WILLNEED, POSIX_FADV_DONTNEED, POSIX_FADV_NOREUSE,
        ];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j]);
            }
        }
    }

    #[test]
    fn test_posix_fadvise_stub() {
        assert_eq!(posix_fadvise(0, 0, 0, POSIX_FADV_NORMAL), 0);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(POSIX_FADV_NORMAL, crate::file::POSIX_FADV_NORMAL);
    }
}
