//! `<linux/ntsync.h>` — NT synchronization primitives constants.
//!
//! ntsync (Linux 6.13+) exposes Windows-compatible synchronization
//! primitives (semaphores, mutexes, events) directly in the kernel to
//! accelerate Wine/Proton. Instead of emulating NT sync objects in
//! userspace (which requires expensive futex workarounds for features
//! like WaitForMultipleObjects), Wine can use native kernel objects
//! that match NT semantics exactly: alertable waits, mutex ownership
//! tracking, and atomic multi-object waits.

// ---------------------------------------------------------------------------
// IOCTL commands (on /dev/ntsync)
// ---------------------------------------------------------------------------

/// Create a semaphore object.
pub const NTSYNC_IOC_CREATE_SEM: u32 = 0x00;
/// Create a mutex object.
pub const NTSYNC_IOC_CREATE_MUTEX: u32 = 0x01;
/// Create an event object.
pub const NTSYNC_IOC_CREATE_EVENT: u32 = 0x02;

// ---------------------------------------------------------------------------
// Semaphore IOCTLs (on semaphore fd)
// ---------------------------------------------------------------------------

/// Release (signal) a semaphore, incrementing its count.
pub const NTSYNC_IOC_SEM_RELEASE: u32 = 0x10;
/// Read current semaphore value.
pub const NTSYNC_IOC_SEM_READ: u32 = 0x11;

// ---------------------------------------------------------------------------
// Mutex IOCTLs (on mutex fd)
// ---------------------------------------------------------------------------

/// Release a mutex (decrement recursion count or unlock).
pub const NTSYNC_IOC_MUTEX_RELEASE: u32 = 0x20;
/// Read current mutex state (owner, recursion count).
pub const NTSYNC_IOC_MUTEX_READ: u32 = 0x21;
/// Kill the mutex owner (mark mutex as abandoned).
pub const NTSYNC_IOC_MUTEX_KILL: u32 = 0x22;

// ---------------------------------------------------------------------------
// Event IOCTLs (on event fd)
// ---------------------------------------------------------------------------

/// Set event to signaled state.
pub const NTSYNC_IOC_EVENT_SET: u32 = 0x30;
/// Reset event to non-signaled state.
pub const NTSYNC_IOC_EVENT_RESET: u32 = 0x31;
/// Pulse event (set then immediately reset; wakes waiters).
pub const NTSYNC_IOC_EVENT_PULSE: u32 = 0x32;
/// Read current event state.
pub const NTSYNC_IOC_EVENT_READ: u32 = 0x33;

// ---------------------------------------------------------------------------
// Wait IOCTLs (on /dev/ntsync)
// ---------------------------------------------------------------------------

/// Wait for any one of multiple objects to become signaled.
pub const NTSYNC_IOC_WAIT_ANY: u32 = 0x40;
/// Wait for all objects to become simultaneously signaled.
pub const NTSYNC_IOC_WAIT_ALL: u32 = 0x41;

// ---------------------------------------------------------------------------
// Event types
// ---------------------------------------------------------------------------

/// Manual-reset event (stays signaled until explicitly reset).
pub const NTSYNC_EVENT_MANUAL: u32 = 0;
/// Auto-reset event (automatically resets after releasing one waiter).
pub const NTSYNC_EVENT_AUTO: u32 = 1;

// ---------------------------------------------------------------------------
// Wait return status
// ---------------------------------------------------------------------------

/// Wait completed — object was signaled.
pub const NTSYNC_WAIT_SIGNALED: u32 = 0;
/// Wait completed — mutex was abandoned by its owner.
pub const NTSYNC_WAIT_ABANDONED: u32 = 1;
/// Wait timed out.
pub const NTSYNC_WAIT_TIMEOUT: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_ioctls_distinct() {
        let creates = [
            NTSYNC_IOC_CREATE_SEM,
            NTSYNC_IOC_CREATE_MUTEX,
            NTSYNC_IOC_CREATE_EVENT,
        ];
        for i in 0..creates.len() {
            for j in (i + 1)..creates.len() {
                assert_ne!(creates[i], creates[j]);
            }
        }
    }

    #[test]
    fn test_sem_ioctls_distinct() {
        assert_ne!(NTSYNC_IOC_SEM_RELEASE, NTSYNC_IOC_SEM_READ);
    }

    #[test]
    fn test_mutex_ioctls_distinct() {
        let mutexes = [
            NTSYNC_IOC_MUTEX_RELEASE,
            NTSYNC_IOC_MUTEX_READ,
            NTSYNC_IOC_MUTEX_KILL,
        ];
        for i in 0..mutexes.len() {
            for j in (i + 1)..mutexes.len() {
                assert_ne!(mutexes[i], mutexes[j]);
            }
        }
    }

    #[test]
    fn test_event_ioctls_distinct() {
        let events = [
            NTSYNC_IOC_EVENT_SET, NTSYNC_IOC_EVENT_RESET,
            NTSYNC_IOC_EVENT_PULSE, NTSYNC_IOC_EVENT_READ,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_wait_ioctls_distinct() {
        assert_ne!(NTSYNC_IOC_WAIT_ANY, NTSYNC_IOC_WAIT_ALL);
    }

    #[test]
    fn test_event_types_distinct() {
        assert_ne!(NTSYNC_EVENT_MANUAL, NTSYNC_EVENT_AUTO);
    }

    #[test]
    fn test_wait_status_distinct() {
        let statuses = [
            NTSYNC_WAIT_SIGNALED,
            NTSYNC_WAIT_ABANDONED,
            NTSYNC_WAIT_TIMEOUT,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_no_ioctl_collisions() {
        let all = [
            NTSYNC_IOC_CREATE_SEM, NTSYNC_IOC_CREATE_MUTEX,
            NTSYNC_IOC_CREATE_EVENT,
            NTSYNC_IOC_SEM_RELEASE, NTSYNC_IOC_SEM_READ,
            NTSYNC_IOC_MUTEX_RELEASE, NTSYNC_IOC_MUTEX_READ,
            NTSYNC_IOC_MUTEX_KILL,
            NTSYNC_IOC_EVENT_SET, NTSYNC_IOC_EVENT_RESET,
            NTSYNC_IOC_EVENT_PULSE, NTSYNC_IOC_EVENT_READ,
            NTSYNC_IOC_WAIT_ANY, NTSYNC_IOC_WAIT_ALL,
        ];
        for i in 0..all.len() {
            for j in (i + 1)..all.len() {
                assert_ne!(all[i], all[j]);
            }
        }
    }
}
