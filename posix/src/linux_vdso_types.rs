//! `<linux/vdso.h>` — vDSO (virtual dynamic shared object) constants.
//!
//! The vDSO is a small shared library mapped into every process's
//! address space by the kernel. It provides fast userspace
//! implementations of certain syscalls that would otherwise require
//! a kernel transition: clock_gettime(), gettimeofday(), getcpu(),
//! time(). Instead of a syscall, these read kernel-maintained data
//! from a shared page. This reduces clock_gettime() from ~100ns
//! (syscall) to ~20ns (vDSO read).

// ---------------------------------------------------------------------------
// vDSO function identifiers
// ---------------------------------------------------------------------------

/// clock_gettime() vDSO function.
pub const VDSO_CLOCK_GETTIME: u32 = 0;
/// gettimeofday() vDSO function.
pub const VDSO_GETTIMEOFDAY: u32 = 1;
/// time() vDSO function.
pub const VDSO_TIME: u32 = 2;
/// getcpu() vDSO function.
pub const VDSO_GETCPU: u32 = 3;
/// clock_getres() vDSO function.
pub const VDSO_CLOCK_GETRES: u32 = 4;

// ---------------------------------------------------------------------------
// vDSO data page layout identifiers
// ---------------------------------------------------------------------------

/// Sequence counter (for consistent reads of multi-word data).
pub const VDSO_DATA_SEQ_COUNT: u32 = 0;
/// Clock mode (which clocksource is in use).
pub const VDSO_DATA_CLOCK_MODE: u32 = 1;
/// Cycle last (clocksource reading at last update).
pub const VDSO_DATA_CYCLE_LAST: u32 = 2;
/// Mask (clocksource mask for overflow handling).
pub const VDSO_DATA_MASK: u32 = 3;
/// Multiplication factor (for cycle→nanosecond conversion).
pub const VDSO_DATA_MULT: u32 = 4;
/// Shift (for cycle→nanosecond conversion).
pub const VDSO_DATA_SHIFT: u32 = 5;

// ---------------------------------------------------------------------------
// vDSO clock modes
// ---------------------------------------------------------------------------

/// No vDSO support for this clock (must use syscall).
pub const VDSO_CLOCKMODE_NONE: u32 = 0;
/// TSC-based vDSO clock (fastest path).
pub const VDSO_CLOCKMODE_TSC: u32 = 1;
/// HPET-based vDSO clock (requires MMIO read from userspace).
pub const VDSO_CLOCKMODE_HPET: u32 = 2;
/// pvclock (paravirtualized, for VMs).
pub const VDSO_CLOCKMODE_PVCLOCK: u32 = 3;
/// Hyper-V TSC page.
pub const VDSO_CLOCKMODE_HYPERV: u32 = 4;

// ---------------------------------------------------------------------------
// vDSO page flags
// ---------------------------------------------------------------------------

/// vDSO data page is valid (kernel has updated it).
pub const VDSO_FLAG_VALID: u32 = 0x01;
/// vDSO supports coarse clocks.
pub const VDSO_FLAG_COARSE: u32 = 0x02;
/// vDSO supports raw monotonic clock.
pub const VDSO_FLAG_RAW: u32 = 0x04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_ids_distinct() {
        let ids = [
            VDSO_CLOCK_GETTIME, VDSO_GETTIMEOFDAY,
            VDSO_TIME, VDSO_GETCPU, VDSO_CLOCK_GETRES,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_data_fields_distinct() {
        let fields = [
            VDSO_DATA_SEQ_COUNT, VDSO_DATA_CLOCK_MODE,
            VDSO_DATA_CYCLE_LAST, VDSO_DATA_MASK,
            VDSO_DATA_MULT, VDSO_DATA_SHIFT,
        ];
        for i in 0..fields.len() {
            for j in (i + 1)..fields.len() {
                assert_ne!(fields[i], fields[j]);
            }
        }
    }

    #[test]
    fn test_clock_modes_distinct() {
        let modes = [
            VDSO_CLOCKMODE_NONE, VDSO_CLOCKMODE_TSC,
            VDSO_CLOCKMODE_HPET, VDSO_CLOCKMODE_PVCLOCK,
            VDSO_CLOCKMODE_HYPERV,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [VDSO_FLAG_VALID, VDSO_FLAG_COARSE, VDSO_FLAG_RAW];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
