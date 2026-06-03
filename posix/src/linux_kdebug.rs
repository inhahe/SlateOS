//! `<asm/kdebug.h>` — Kernel debug notification constants.
//!
//! The kdebug subsystem provides notifications for kernel debug
//! events (breakpoints, traps, oops, panics, NMIs). Debuggers
//! like kgdb and kprobes register on the die_chain notifier
//! to intercept these events.

// ---------------------------------------------------------------------------
// Die notification events
// ---------------------------------------------------------------------------

/// Oops event.
pub const DIE_OOPS: u32 = 1;
/// INT3 (breakpoint) event.
pub const DIE_INT3: u32 = 2;
/// Debug trap event.
pub const DIE_DEBUG: u32 = 3;
/// Panic event.
pub const DIE_PANIC: u32 = 4;
/// NMI event.
pub const DIE_NMI: u32 = 5;
/// Die (fatal) event.
pub const DIE_DIE: u32 = 6;
/// NMI IPI (inter-processor interrupt).
pub const DIE_NMIUNKNOWN: u32 = 7;
/// NMI watchdog event.
pub const DIE_NMIWATCHDOG: u32 = 8;
/// Kernel page fault event.
pub const DIE_KERNELDEBUG: u32 = 9;
/// Trap event.
pub const DIE_TRAP: u32 = 10;
/// GPF (General Protection Fault).
pub const DIE_GPF: u32 = 11;
/// Call event.
pub const DIE_CALL: u32 = 12;

// ---------------------------------------------------------------------------
// Debug register constants (x86)
// ---------------------------------------------------------------------------

/// Number of hardware debug registers.
pub const NUM_DEBUG_REGISTERS: usize = 4;
/// DR7: local exact breakpoint enable.
pub const DR7_LOCAL_ENABLE_SHIFT: u32 = 0;
/// DR7: global exact breakpoint enable.
pub const DR7_GLOBAL_ENABLE_SHIFT: u32 = 1;
/// DR7: break on execute.
pub const DR7_BREAK_ON_EXEC: u32 = 0;
/// DR7: break on write.
pub const DR7_BREAK_ON_WRITE: u32 = 1;
/// DR7: break on I/O read/write.
pub const DR7_BREAK_ON_IO: u32 = 2;
/// DR7: break on read/write (no exec).
pub const DR7_BREAK_ON_RW: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_die_events_distinct() {
        let events = [
            DIE_OOPS,
            DIE_INT3,
            DIE_DEBUG,
            DIE_PANIC,
            DIE_NMI,
            DIE_DIE,
            DIE_NMIUNKNOWN,
            DIE_NMIWATCHDOG,
            DIE_KERNELDEBUG,
            DIE_TRAP,
            DIE_GPF,
            DIE_CALL,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_num_debug_registers() {
        assert_eq!(NUM_DEBUG_REGISTERS, 4);
    }

    #[test]
    fn test_dr7_break_types_distinct() {
        let types = [
            DR7_BREAK_ON_EXEC,
            DR7_BREAK_ON_WRITE,
            DR7_BREAK_ON_IO,
            DR7_BREAK_ON_RW,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
