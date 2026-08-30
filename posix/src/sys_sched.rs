//! `<sys/sched.h>` — scheduling re-exports.
//!
//! Re-exports scheduling functions and constants from the `sched`
//! module.  This maps to the `<sched.h>` header in a `<sys/>` path.

pub use crate::pthread::sched_yield;
pub use crate::sched::SCHED_FIFO;
pub use crate::sched::SCHED_OTHER;
pub use crate::sched::SCHED_RR;
pub use crate::sched::SchedParam;
pub use crate::sched::sched_get_priority_max;
pub use crate::sched::sched_get_priority_min;
pub use crate::sched::sched_getscheduler;
pub use crate::sched::sched_setscheduler;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sched_policies() {
        assert_eq!(SCHED_OTHER, 0);
        assert_eq!(SCHED_FIFO, 1);
        assert_eq!(SCHED_RR, 2);
    }

    #[test]
    fn test_sched_param_size() {
        assert!(core::mem::size_of::<SchedParam>() > 0);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(SCHED_OTHER, crate::sched::SCHED_OTHER);
    }
}
