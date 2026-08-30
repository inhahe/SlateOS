//! `<linux/wait.h>` — wait flags (kernel view).
//!
//! Re-exports standard wait flags from `wait` and adds Linux-specific
//! wait constants used by the `waitid` and `wait4` syscalls.

// ---------------------------------------------------------------------------
// Re-exports from wait module
// ---------------------------------------------------------------------------

pub use crate::wait::WCONTINUED;
pub use crate::wait::WEXITED;
pub use crate::wait::WNOHANG;
pub use crate::wait::WNOWAIT;
pub use crate::wait::WSTOPPED;
pub use crate::wait::WUNTRACED;

// ---------------------------------------------------------------------------
// Linux-specific wait flags
// ---------------------------------------------------------------------------

/// Wait for any child (like __WALL).
pub const __WALL: i32 = 0x4000_0000;
/// Wait for cloned children.
pub const __WCLONE: i32 = 0x8000_0000_u32 as i32;
/// Don't reap, just poll status (Linux-specific).
pub const __WNOTHREAD: i32 = 0x2000_0000;

// ---------------------------------------------------------------------------
// P_* id type constants for waitid
// ---------------------------------------------------------------------------

pub use crate::process::P_ALL;
pub use crate::process::P_PGID;
pub use crate::process::P_PID;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_flags() {
        assert_eq!(WNOHANG, 1);
        assert_eq!(WUNTRACED, 2);
        assert_eq!(WCONTINUED, 8);
    }

    #[test]
    fn test_linux_specific_flags() {
        assert_eq!(__WALL, 0x4000_0000);
        assert_eq!(__WNOTHREAD, 0x2000_0000);
    }

    #[test]
    fn test_flags_distinct() {
        let flags = [
            WNOHANG,
            WUNTRACED,
            WCONTINUED,
            WEXITED,
            WNOWAIT,
            __WALL,
            __WNOTHREAD,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(WNOHANG, crate::wait::WNOHANG);
        assert_eq!(WUNTRACED, crate::wait::WUNTRACED);
        assert_eq!(WCONTINUED, crate::wait::WCONTINUED);
    }
}
