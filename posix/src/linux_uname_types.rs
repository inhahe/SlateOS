//! `<linux/utsname.h>` — uname() structure field size constants.
//!
//! uname() returns system identification: OS name, hostname, kernel
//! release, kernel version, machine architecture, and (Linux-specific)
//! domain name. Fields are fixed-size character arrays in the kernel's
//! utsname structure.

// ---------------------------------------------------------------------------
// Field sizes (bytes, including null terminator)
// ---------------------------------------------------------------------------

/// Size of each utsname field (sysname, nodename, release, version, machine).
pub const UTS_FIELD_SIZE: u32 = 65;
/// Size of domainname field.
pub const UTS_DOMAIN_SIZE: u32 = 65;

// ---------------------------------------------------------------------------
// Default/expected field values
// ---------------------------------------------------------------------------

/// Expected sysname value.
pub const UTS_SYSNAME: &str = "Linux";
/// Machine architecture for x86-64.
pub const UTS_MACHINE_X86_64: &str = "x86_64";
/// Machine architecture for ARM64.
pub const UTS_MACHINE_AARCH64: &str = "aarch64";
/// Machine architecture for RISC-V 64.
pub const UTS_MACHINE_RISCV64: &str = "riscv64";
/// Machine architecture for i686.
pub const UTS_MACHINE_I686: &str = "i686";

// ---------------------------------------------------------------------------
// Version string format components
// ---------------------------------------------------------------------------

/// Minimum kernel version string length (e.g., "5.0.0").
pub const UTS_VERSION_MIN_LEN: u32 = 5;
/// Release string typical pattern: "major.minor.patch".
pub const UTS_RELEASE_PARTS: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field_sizes() {
        assert_eq!(UTS_FIELD_SIZE, 65);
        assert_eq!(UTS_DOMAIN_SIZE, 65);
    }

    #[test]
    fn test_sysname() {
        assert_eq!(UTS_SYSNAME, "Linux");
    }

    #[test]
    fn test_machine_strings_distinct() {
        let machines = [
            UTS_MACHINE_X86_64, UTS_MACHINE_AARCH64,
            UTS_MACHINE_RISCV64, UTS_MACHINE_I686,
        ];
        for i in 0..machines.len() {
            for j in (i + 1)..machines.len() {
                assert_ne!(machines[i], machines[j]);
            }
        }
    }

    #[test]
    fn test_field_size_fits_strings() {
        assert!((UTS_MACHINE_X86_64.len() as u32) < UTS_FIELD_SIZE);
        assert!((UTS_SYSNAME.len() as u32) < UTS_FIELD_SIZE);
    }
}
