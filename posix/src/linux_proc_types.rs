//! `<linux/proc_fs.h>` — procfs entry type and flag constants.
//!
//! procfs (`/proc`) exposes process information and kernel state as
//! a virtual filesystem. These constants define proc entry types,
//! process state codes, and /proc file permission modes.

// ---------------------------------------------------------------------------
// Process states (from /proc/[pid]/stat)
// ---------------------------------------------------------------------------

/// Running.
pub const PROC_STATE_RUNNING: u8 = b'R';
/// Sleeping in interruptible wait.
pub const PROC_STATE_SLEEPING: u8 = b'S';
/// Waiting in uninterruptible disk sleep.
pub const PROC_STATE_DISK_SLEEP: u8 = b'D';
/// Zombie (terminated, awaiting wait()).
pub const PROC_STATE_ZOMBIE: u8 = b'Z';
/// Stopped (by signal or ptrace).
pub const PROC_STATE_STOPPED: u8 = b'T';
/// Tracing stop.
pub const PROC_STATE_TRACING_STOP: u8 = b't';
/// Dead (should never be seen).
pub const PROC_STATE_DEAD: u8 = b'X';
/// Idle (kernel thread).
pub const PROC_STATE_IDLE: u8 = b'I';

// ---------------------------------------------------------------------------
// /proc/[pid]/status fields (permission bits for hidepid=)
// ---------------------------------------------------------------------------

/// hidepid=0: everybody can access /proc/[pid].
pub const PROC_HIDEPID_OFF: u32 = 0;
/// hidepid=1: only own /proc/[pid] cmdline/status visible.
pub const PROC_HIDEPID_NO_ACCESS: u32 = 1;
/// hidepid=2: /proc/[pid] invisible to non-owners.
pub const PROC_HIDEPID_INVISIBLE: u32 = 2;
/// hidepid=4: only root + same-user can see /proc/[pid].
pub const PROC_HIDEPID_NOT_PTRACEABLE: u32 = 4;

// ---------------------------------------------------------------------------
// /proc/sys/vm / memory info indices
// ---------------------------------------------------------------------------

/// /proc/meminfo total memory field index.
pub const PROC_MEMINFO_TOTAL: u32 = 0;
/// /proc/meminfo free memory field index.
pub const PROC_MEMINFO_FREE: u32 = 1;
/// /proc/meminfo available memory field index.
pub const PROC_MEMINFO_AVAILABLE: u32 = 2;
/// /proc/meminfo buffers field index.
pub const PROC_MEMINFO_BUFFERS: u32 = 3;
/// /proc/meminfo cached field index.
pub const PROC_MEMINFO_CACHED: u32 = 4;

// ---------------------------------------------------------------------------
// /proc/[pid]/oom_score_adj limits
// ---------------------------------------------------------------------------

/// Minimum OOM score adjustment (never kill).
pub const OOM_SCORE_ADJ_MIN: i16 = -1000;
/// Maximum OOM score adjustment (kill first).
pub const OOM_SCORE_ADJ_MAX: i16 = 1000;
/// Disable OOM killer for this process.
pub const OOM_DISABLE: i16 = -17;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_states_distinct() {
        let states = [
            PROC_STATE_RUNNING,
            PROC_STATE_SLEEPING,
            PROC_STATE_DISK_SLEEP,
            PROC_STATE_ZOMBIE,
            PROC_STATE_STOPPED,
            PROC_STATE_TRACING_STOP,
            PROC_STATE_DEAD,
            PROC_STATE_IDLE,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_running_is_r() {
        assert_eq!(PROC_STATE_RUNNING, b'R');
    }

    #[test]
    fn test_hidepid_distinct() {
        let modes = [
            PROC_HIDEPID_OFF,
            PROC_HIDEPID_NO_ACCESS,
            PROC_HIDEPID_INVISIBLE,
            PROC_HIDEPID_NOT_PTRACEABLE,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_oom_score_range() {
        assert_eq!(OOM_SCORE_ADJ_MIN, -1000);
        assert_eq!(OOM_SCORE_ADJ_MAX, 1000);
        assert!(OOM_SCORE_ADJ_MIN < OOM_SCORE_ADJ_MAX);
    }

    #[test]
    fn test_meminfo_fields_sequential() {
        assert_eq!(PROC_MEMINFO_TOTAL, 0);
        assert_eq!(PROC_MEMINFO_FREE, 1);
        assert_eq!(PROC_MEMINFO_AVAILABLE, 2);
    }
}
