//! `<sys/resource.h>` — resource operations.
//!
//! Re-exports resource-limit and usage functions and constants
//! from the `resource` module.

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

pub use crate::resource::Rlimit;
pub use crate::resource::Rusage;

// ---------------------------------------------------------------------------
// Resource limit constants
// ---------------------------------------------------------------------------

pub use crate::resource::RLIM_INFINITY;
pub use crate::resource::RLIMIT_AS;
pub use crate::resource::RLIMIT_CORE;
pub use crate::resource::RLIMIT_CPU;
pub use crate::resource::RLIMIT_DATA;
pub use crate::resource::RLIMIT_FSIZE;
pub use crate::resource::RLIMIT_LOCKS;
pub use crate::resource::RLIMIT_MEMLOCK;
pub use crate::resource::RLIMIT_MSGQUEUE;
pub use crate::resource::RLIMIT_NICE;
pub use crate::resource::RLIMIT_NOFILE;
pub use crate::resource::RLIMIT_NPROC;
pub use crate::resource::RLIMIT_RSS;
pub use crate::resource::RLIMIT_RTPRIO;
pub use crate::resource::RLIMIT_RTTIME;
pub use crate::resource::RLIMIT_SIGPENDING;
pub use crate::resource::RLIMIT_STACK;

// ---------------------------------------------------------------------------
// Usage constants
// ---------------------------------------------------------------------------

pub use crate::resource::RUSAGE_CHILDREN;
pub use crate::resource::RUSAGE_SELF;
pub use crate::resource::RUSAGE_THREAD;

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

pub use crate::resource::getrlimit;
pub use crate::resource::getrusage;
pub use crate::resource::prlimit;
pub use crate::resource::prlimit64;
pub use crate::resource::setrlimit;

// ---------------------------------------------------------------------------
// Priority functions
// ---------------------------------------------------------------------------

/// Process priority for getpriority/setpriority.
pub const PRIO_PROCESS: i32 = 0;

/// Process group priority.
pub const PRIO_PGRP: i32 = 1;

/// User priority.
pub const PRIO_USER: i32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rlimit_struct_size() {
        // Rlimit has two u64 fields: rlim_cur and rlim_max.
        assert_eq!(core::mem::size_of::<Rlimit>(), 16);
    }

    #[test]
    fn test_rusage_struct_size() {
        assert!(core::mem::size_of::<Rusage>() > 0);
    }

    #[test]
    fn test_rlimit_constants_distinct() {
        let limits = [
            RLIMIT_CPU,
            RLIMIT_FSIZE,
            RLIMIT_DATA,
            RLIMIT_STACK,
            RLIMIT_CORE,
            RLIMIT_RSS,
            RLIMIT_NPROC,
            RLIMIT_NOFILE,
            RLIMIT_MEMLOCK,
            RLIMIT_AS,
            RLIMIT_LOCKS,
            RLIMIT_SIGPENDING,
            RLIMIT_MSGQUEUE,
            RLIMIT_NICE,
            RLIMIT_RTPRIO,
            RLIMIT_RTTIME,
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
    fn test_rusage_who_constants() {
        assert_eq!(RUSAGE_SELF, 0);
        assert_eq!(RUSAGE_CHILDREN, -1);
        assert_eq!(RUSAGE_THREAD, 1);
    }

    #[test]
    fn test_prio_constants_distinct() {
        assert_ne!(PRIO_PROCESS, PRIO_PGRP);
        assert_ne!(PRIO_PROCESS, PRIO_USER);
        assert_ne!(PRIO_PGRP, PRIO_USER);
    }

    #[test]
    fn test_getrlimit_nofile() {
        let mut rlim = Rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        let ret = getrlimit(RLIMIT_NOFILE, &mut rlim);
        assert_eq!(ret, 0);
        assert!(rlim.rlim_cur > 0);
        assert!(rlim.rlim_max >= rlim.rlim_cur);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(RLIMIT_NOFILE, crate::resource::RLIMIT_NOFILE);
        assert_eq!(RLIMIT_STACK, crate::resource::RLIMIT_STACK);
        assert_eq!(RLIM_INFINITY, crate::resource::RLIM_INFINITY);
    }
}
