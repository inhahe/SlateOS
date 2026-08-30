//! `<linux/secretmem.h>` — `memfd_secret(2)` flag constants.
//!
//! `memfd_secret()` returns an anonymous file descriptor whose pages
//! are removed from the kernel's direct map for the duration of their
//! lifetime, providing a small confidentiality benefit against
//! kernel-memory disclosure bugs. Userspace key-management and
//! password vaults (e.g., systemd-creds, libssh secret memory) call
//! this with the flags below.

// ---------------------------------------------------------------------------
// memfd_secret() flag bits
// ---------------------------------------------------------------------------

/// Set `FD_CLOEXEC` on the returned file descriptor.
pub const SECRETMEM_FLAG_CLOEXEC: u32 = 0x0008_0000;
/// Compatibility alias matching `O_CLOEXEC` numeric value (used by some
/// older headers that derive the flag from `O_*`).
pub const FD_CLOEXEC_NUMERIC: u32 = 0x0008_0000;

// ---------------------------------------------------------------------------
// Page-fault diagnostic — `vm_area_struct.vm_flags` bit set on secret
// mappings (informational; userspace sees this through `/proc/self/smaps`
// "Locked" + "lo" markers).
// ---------------------------------------------------------------------------

/// Vm-flag bit reported via smaps (`VM_LOCKED` and `VM_LOCKONFAULT` are
/// also set, but `VM_SECRETMEM` is what marks the mapping in 5.14+).
pub const VM_SECRETMEM_BIT: u32 = 27;

// ---------------------------------------------------------------------------
// Bit-mask covering all accepted flags
// ---------------------------------------------------------------------------

/// Bit-mask of every accepted flag (used by userspace to reject
/// unknown bits up-front).
pub const SECRETMEM_VALID_FLAGS: u32 = SECRETMEM_FLAG_CLOEXEC;

// ---------------------------------------------------------------------------
// Limits enforced by the kernel
// ---------------------------------------------------------------------------

/// Hard kernel default limit for total secretmem pages per system
/// (configurable via `kernel.parameters.secretmem.enable` / mlock cap).
/// 0 means "use mlock RLIMIT".
pub const SECRETMEM_DEFAULT_RLIMIT: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flag_value_matches_o_cloexec() {
        // The kernel chose 0o2000000 == 0x80000 specifically so the
        // single-flag set composes with future memfd-style flags.
        assert_eq!(SECRETMEM_FLAG_CLOEXEC, 0x0008_0000);
        assert_eq!(FD_CLOEXEC_NUMERIC, SECRETMEM_FLAG_CLOEXEC);
        assert!(SECRETMEM_FLAG_CLOEXEC.is_power_of_two());
    }

    #[test]
    fn test_valid_flags_mask_only_known_bits() {
        assert_eq!(SECRETMEM_VALID_FLAGS, SECRETMEM_FLAG_CLOEXEC);
        // Sanity: arbitrary unrelated bits are not part of the mask.
        assert_eq!(SECRETMEM_VALID_FLAGS & 0x1, 0);
        assert_eq!(SECRETMEM_VALID_FLAGS & 0x4000_0000, 0);
    }

    #[test]
    fn test_vm_secretmem_bit_in_vmflags_range() {
        // `vm_flags` is a u64; bit 27 corresponds to VM_SECRETMEM in
        // mainline. Must fit in the field.
        assert!(VM_SECRETMEM_BIT < 64);
    }

    #[test]
    fn test_default_rlimit_is_zero() {
        // 0 means "follow the standard RLIMIT_MEMLOCK accounting".
        assert_eq!(SECRETMEM_DEFAULT_RLIMIT, 0);
    }
}
