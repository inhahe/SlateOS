//! `<linux/resource.h>` — resource limits (kernel view).
//!
//! Re-exports RLIMIT_* constants and rlimit functions from `resource`.

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

pub use crate::resource::RLIMIT_AS;
pub use crate::resource::RLIMIT_CORE;
pub use crate::resource::RLIMIT_CPU;
pub use crate::resource::RLIMIT_DATA;
pub use crate::resource::RLIMIT_FSIZE;
pub use crate::resource::RLIMIT_NOFILE;
pub use crate::resource::RLIMIT_STACK;
pub use crate::resource::RLIMIT_NPROC;
pub use crate::resource::RLIMIT_RSS;
pub use crate::resource::RLIMIT_MSGQUEUE;
pub use crate::resource::RLIMIT_MEMLOCK;
pub use crate::resource::RLIMIT_LOCKS;
pub use crate::resource::RLIMIT_SIGPENDING;
pub use crate::resource::RLIMIT_NICE;
pub use crate::resource::RLIMIT_RTPRIO;
pub use crate::resource::RLIMIT_RTTIME;
pub use crate::resource::RLIM_INFINITY;
pub use crate::resource::Rlimit;
pub use crate::resource::getrlimit;
pub use crate::resource::setrlimit;

// ---------------------------------------------------------------------------
// prlimit64 (Linux-specific, extends getrlimit/setrlimit)
// ---------------------------------------------------------------------------

/// Maximum number of rlimit types.
pub const RLIM_NLIMITS: i32 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rlimit_values() {
        assert_eq!(RLIMIT_CPU, 0);
        assert_eq!(RLIMIT_FSIZE, 1);
        assert_eq!(RLIMIT_NOFILE, 7);
    }

    #[test]
    fn test_rlimits_distinct() {
        let limits = [
            RLIMIT_CPU, RLIMIT_FSIZE, RLIMIT_DATA, RLIMIT_STACK,
            RLIMIT_CORE, RLIMIT_RSS, RLIMIT_NPROC, RLIMIT_NOFILE,
            RLIMIT_MEMLOCK, RLIMIT_AS, RLIMIT_LOCKS,
            RLIMIT_SIGPENDING, RLIMIT_MSGQUEUE, RLIMIT_NICE,
            RLIMIT_RTPRIO, RLIMIT_RTTIME,
        ];
        for i in 0..limits.len() {
            for j in (i + 1)..limits.len() {
                assert_ne!(limits[i], limits[j]);
            }
        }
    }

    #[test]
    fn test_rlim_infinity() {
        assert_eq!(RLIM_INFINITY, u64::MAX);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(RLIMIT_NOFILE, crate::resource::RLIMIT_NOFILE);
        assert_eq!(RLIMIT_NPROC, crate::resource::RLIMIT_NPROC);
        assert_eq!(RLIM_INFINITY, crate::resource::RLIM_INFINITY);
    }
}
