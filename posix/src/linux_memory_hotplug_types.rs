//! `<linux/memory_hotplug.h>` — Memory hotplug constants.
//!
//! Memory hotplug allows adding or removing physical memory from a
//! running system. This is common in virtualized environments (balloon
//! drivers, DIMM simulation) and enterprise servers with hot-add DIMM
//! slots. Memory blocks (typically 128MB sections) transition through
//! states: offline → going-online → online (and reverse for removal).
//! The kernel must migrate pages off a block before it can be offlined.

// ---------------------------------------------------------------------------
// Memory block states
// ---------------------------------------------------------------------------

/// Memory block is offline (not usable by the system).
pub const MEM_BLOCK_OFFLINE: u32 = 0;
/// Memory block is going online (transitioning).
pub const MEM_BLOCK_GOING_ONLINE: u32 = 1;
/// Memory block is online (usable for allocation).
pub const MEM_BLOCK_ONLINE: u32 = 2;
/// Memory block is going offline (pages being migrated).
pub const MEM_BLOCK_GOING_OFFLINE: u32 = 3;

// ---------------------------------------------------------------------------
// Memory online types
// ---------------------------------------------------------------------------

/// Online to ZONE_NORMAL (default kernel allocations).
pub const MMOP_ONLINE_KERNEL: u32 = 0;
/// Online to ZONE_MOVABLE (only movable pages, easy offline).
pub const MMOP_ONLINE_MOVABLE: u32 = 1;
/// Online to the zone the firmware recommends.
pub const MMOP_ONLINE_KEEP: u32 = 2;
/// Offline the memory block.
pub const MMOP_OFFLINE: u32 = 3;

// ---------------------------------------------------------------------------
// Memory hotplug notification events
// ---------------------------------------------------------------------------

/// Memory block is going online (notifier can reject).
pub const MEM_NOTIFY_GOING_ONLINE: u32 = 0;
/// Memory block went online successfully.
pub const MEM_NOTIFY_ONLINE: u32 = 1;
/// Memory block is going offline (notifier can reject).
pub const MEM_NOTIFY_GOING_OFFLINE: u32 = 2;
/// Memory block went offline successfully.
pub const MEM_NOTIFY_OFFLINE: u32 = 3;
/// Memory block online failed (rollback).
pub const MEM_NOTIFY_CANCEL_ONLINE: u32 = 4;
/// Memory block offline failed (rollback).
pub const MEM_NOTIFY_CANCEL_OFFLINE: u32 = 5;

// ---------------------------------------------------------------------------
// Memory section size
// ---------------------------------------------------------------------------

/// Memory section size shift (128MB = 2^27).
pub const MEMORY_SECTION_SHIFT: u32 = 27;
/// Memory section size in bytes (128MB).
pub const MEMORY_SECTION_SIZE: u32 = 1 << 27;
/// Pages per memory section (128MB / 4KB = 32768 for standard 4KB pages).
pub const PAGES_PER_SECTION_4K: u32 = 32768;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_states_distinct() {
        let states = [
            MEM_BLOCK_OFFLINE, MEM_BLOCK_GOING_ONLINE,
            MEM_BLOCK_ONLINE, MEM_BLOCK_GOING_OFFLINE,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_online_types_distinct() {
        let types = [
            MMOP_ONLINE_KERNEL, MMOP_ONLINE_MOVABLE,
            MMOP_ONLINE_KEEP, MMOP_OFFLINE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_notify_events_distinct() {
        let events = [
            MEM_NOTIFY_GOING_ONLINE, MEM_NOTIFY_ONLINE,
            MEM_NOTIFY_GOING_OFFLINE, MEM_NOTIFY_OFFLINE,
            MEM_NOTIFY_CANCEL_ONLINE, MEM_NOTIFY_CANCEL_OFFLINE,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_section_size() {
        assert_eq!(MEMORY_SECTION_SIZE, 1 << MEMORY_SECTION_SHIFT);
        assert!(PAGES_PER_SECTION_4K > 0);
    }
}
