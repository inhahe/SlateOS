//! `<linux/personality.h>` — execution personality (kernel view).
//!
//! Re-exports from `sys_personality`.

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

pub use crate::sys_personality::PER_BSD;
pub use crate::sys_personality::PER_LINUX;
pub use crate::sys_personality::PER_LINUX32;
pub use crate::sys_personality::PER_LINUX32_3GB;
pub use crate::sys_personality::PER_SVR4;

pub use crate::sys_personality::ADDR_COMPAT_LAYOUT;
pub use crate::sys_personality::ADDR_LIMIT_3GB;
pub use crate::sys_personality::ADDR_LIMIT_32BIT;
pub use crate::sys_personality::ADDR_NO_RANDOMIZE;
pub use crate::sys_personality::MMAP_PAGE_ZERO;
pub use crate::sys_personality::READ_IMPLIES_EXEC;
pub use crate::sys_personality::SHORT_INODE;
pub use crate::sys_personality::STICKY_TIMEOUTS;
pub use crate::sys_personality::WHOLE_SECONDS;

pub use crate::unistd::personality;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_per_linux() {
        assert_eq!(PER_LINUX, 0);
    }

    #[test]
    fn test_personality_query() {
        // PERSONALITY_STATE is a process-global atomic and cargo runs these
        // tests on parallel threads, so take the cross-test lock and set the
        // value this test asserts on rather than assuming it.  See
        // `unistd::PERSONALITY_TEST_LOCK`.
        let _g = crate::unistd::lock_personality_for_test();
        personality(u64::from(PER_LINUX));
        let ret = personality(0xFFFF_FFFF);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_addr_flags_distinct() {
        let flags = [
            ADDR_NO_RANDOMIZE,
            MMAP_PAGE_ZERO,
            ADDR_COMPAT_LAYOUT,
            READ_IMPLIES_EXEC,
            ADDR_LIMIT_32BIT,
            SHORT_INODE,
            WHOLE_SECONDS,
            STICKY_TIMEOUTS,
            ADDR_LIMIT_3GB,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(PER_LINUX, crate::sys_personality::PER_LINUX);
        assert_eq!(ADDR_NO_RANDOMIZE, crate::sys_personality::ADDR_NO_RANDOMIZE);
    }
}
