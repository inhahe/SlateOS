//! `<linux/user_events.h>` — User events (user_events) tracing constants.
//!
//! User events allow userspace applications to create custom trace
//! events that appear in the kernel's tracing infrastructure (ftrace,
//! perf, eBPF). Applications register event definitions, get a file
//! descriptor, and write event data. The events show up in
//! /sys/kernel/tracing/events/user_events/ alongside kernel events,
//! enabling unified tracing of both kernel and application activity.
//! Added in Linux 6.4.

// ---------------------------------------------------------------------------
// User events IOCTLs (via /sys/kernel/tracing/user_events_data)
// ---------------------------------------------------------------------------

/// Register a new user event.
pub const USER_EVENT_REG: u32 = 0x00;
/// Unregister a user event.
pub const USER_EVENT_UNREG: u32 = 0x01;

// ---------------------------------------------------------------------------
// User event registration flags
// ---------------------------------------------------------------------------

/// Event data is in write() format (default).
pub const USER_EVENT_FLAG_WRITE: u32 = 0;
/// Event uses writev() for data (multi-buffer).
pub const USER_EVENT_FLAG_WRITEV: u32 = 1 << 0;
/// Event should persist across process exit.
pub const USER_EVENT_FLAG_PERSIST: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// User event field types (data layout descriptors)
// ---------------------------------------------------------------------------

/// Unsigned 8-bit integer field.
pub const USER_EVENT_FIELD_U8: u32 = 0;
/// Signed 8-bit integer field.
pub const USER_EVENT_FIELD_S8: u32 = 1;
/// Unsigned 16-bit integer field.
pub const USER_EVENT_FIELD_U16: u32 = 2;
/// Signed 16-bit integer field.
pub const USER_EVENT_FIELD_S16: u32 = 3;
/// Unsigned 32-bit integer field.
pub const USER_EVENT_FIELD_U32: u32 = 4;
/// Signed 32-bit integer field.
pub const USER_EVENT_FIELD_S32: u32 = 5;
/// Unsigned 64-bit integer field.
pub const USER_EVENT_FIELD_U64: u32 = 6;
/// Signed 64-bit integer field.
pub const USER_EVENT_FIELD_S64: u32 = 7;
/// String field (null-terminated).
pub const USER_EVENT_FIELD_STRING: u32 = 8;
/// Dynamic array field (variable length).
pub const USER_EVENT_FIELD_DYN_ARRAY: u32 = 9;

// ---------------------------------------------------------------------------
// User event status (enable status via mmap)
// ---------------------------------------------------------------------------

/// Event is not enabled by any consumer.
pub const USER_EVENT_STATUS_DISABLED: u32 = 0;
/// Event is enabled (at least one consumer active).
pub const USER_EVENT_STATUS_ENABLED: u32 = 1;

// ---------------------------------------------------------------------------
// User event write sizes
// ---------------------------------------------------------------------------

/// Maximum event name length.
pub const USER_EVENT_NAME_MAX: u32 = 64;
/// Maximum event data size per write.
pub const USER_EVENT_DATA_MAX: u32 = 8192;
/// Maximum number of fields per event.
pub const USER_EVENT_FIELDS_MAX: u32 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct() {
        assert_ne!(USER_EVENT_REG, USER_EVENT_UNREG);
    }

    #[test]
    fn test_reg_flags_distinct() {
        // WRITEV and PERSIST are bit flags that don't overlap
        let flags = [USER_EVENT_FLAG_WRITEV, USER_EVENT_FLAG_PERSIST];
        assert_eq!(flags[0] & flags[1], 0);
        for f in flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_field_types_distinct() {
        let types = [
            USER_EVENT_FIELD_U8,
            USER_EVENT_FIELD_S8,
            USER_EVENT_FIELD_U16,
            USER_EVENT_FIELD_S16,
            USER_EVENT_FIELD_U32,
            USER_EVENT_FIELD_S32,
            USER_EVENT_FIELD_U64,
            USER_EVENT_FIELD_S64,
            USER_EVENT_FIELD_STRING,
            USER_EVENT_FIELD_DYN_ARRAY,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_status_distinct() {
        assert_ne!(USER_EVENT_STATUS_DISABLED, USER_EVENT_STATUS_ENABLED);
    }

    #[test]
    fn test_size_limits() {
        assert!(USER_EVENT_NAME_MAX > 0);
        assert!(USER_EVENT_DATA_MAX > 0);
        assert!(USER_EVENT_FIELDS_MAX > 0);
        assert!(USER_EVENT_DATA_MAX.is_power_of_two());
    }
}
