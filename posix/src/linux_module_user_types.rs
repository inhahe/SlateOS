//! `<linux/module.h>` — kernel-module loading ABI.
//!
//! `init_module(2)` and `finit_module(2)` are how `kmod`, `udev`, and
//! `systemd-modules-load` insert kernel modules. `delete_module(2)`
//! unloads them. Modern userspace uses `finit_module` exclusively
//! because it accepts an fd, enabling signature verification by the
//! kernel without a userspace copy.

// ---------------------------------------------------------------------------
// `finit_module(2)` flags
// ---------------------------------------------------------------------------

/// Skip the modversion (CRC) check.
pub const MODULE_INIT_IGNORE_MODVERSIONS: u32 = 1;
/// Skip the vermagic check.
pub const MODULE_INIT_IGNORE_VERMAGIC: u32 = 2;
/// Use the embedded compression metadata to decompress before insmod.
pub const MODULE_INIT_COMPRESSED_FILE: u32 = 4;

/// Mask covering all valid `finit_module` flags.
pub const MODULE_INIT_ALL_FLAGS: u32 = MODULE_INIT_IGNORE_MODVERSIONS
    | MODULE_INIT_IGNORE_VERMAGIC
    | MODULE_INIT_COMPRESSED_FILE;

// ---------------------------------------------------------------------------
// `delete_module(2)` flags
// ---------------------------------------------------------------------------

/// Don't wait for the module to be idle — error if busy.
pub const O_NONBLOCK_DELETE_MODULE: u32 = 0o4000;
/// Block until the module is idle.
pub const O_TRUNC_DELETE_MODULE: u32 = 0o1000;

// ---------------------------------------------------------------------------
// Syscall numbers (x86_64)
// ---------------------------------------------------------------------------

pub const NR_INIT_MODULE: u32 = 175;
pub const NR_DELETE_MODULE: u32 = 176;
pub const NR_FINIT_MODULE: u32 = 313;

// ---------------------------------------------------------------------------
// Module name / parameter limits
// ---------------------------------------------------------------------------

/// `MODULE_NAME_LEN` — the kernel-side hard cap on module names.
pub const MODULE_NAME_LEN: usize = 64 - 8;
/// `MODULE_FLAGS_LEN` — flags string (live/coming/going/unformed).
pub const MODULE_STATE_NAMES_MAX: usize = 12;

// ---------------------------------------------------------------------------
// Sysfs paths
// ---------------------------------------------------------------------------

pub const PROC_MODULES_PATH: &str = "/proc/modules";
pub const SYS_MODULE_DIR: &str = "/sys/module";

// ---------------------------------------------------------------------------
// Module taint flags (single ASCII letter encoded as `u8`).
// ---------------------------------------------------------------------------

pub const TAINT_PROPRIETARY_MODULE: u8 = b'P';
pub const TAINT_FORCED_MODULE: u8 = b'F';
pub const TAINT_OUT_OF_TREE_MODULE: u8 = b'O';
pub const TAINT_UNSIGNED_MODULE: u8 = b'E';
pub const TAINT_LIVEPATCH: u8 = b'K';

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_finit_flags_dense_and_single_bit() {
        let f = [
            MODULE_INIT_IGNORE_MODVERSIONS,
            MODULE_INIT_IGNORE_VERMAGIC,
            MODULE_INIT_COMPRESSED_FILE,
        ];
        for v in f {
            assert!(v.is_power_of_two());
        }
        // Three dense bits.
        assert_eq!(MODULE_INIT_ALL_FLAGS, 0x7);
    }

    #[test]
    fn test_syscall_numbers_ordering() {
        assert_eq!(NR_DELETE_MODULE, NR_INIT_MODULE + 1);
        // finit_module added much later as a separate number.
        assert!(NR_FINIT_MODULE > NR_DELETE_MODULE);
        assert_eq!(NR_FINIT_MODULE, 313);
    }

    #[test]
    fn test_name_length_cap() {
        // MODULE_NAME_LEN == 56 bytes ('struct module' layout).
        assert_eq!(MODULE_NAME_LEN, 56);
    }

    #[test]
    fn test_taint_letters_unique_and_ascii() {
        let t = [
            TAINT_PROPRIETARY_MODULE,
            TAINT_FORCED_MODULE,
            TAINT_OUT_OF_TREE_MODULE,
            TAINT_UNSIGNED_MODULE,
            TAINT_LIVEPATCH,
        ];
        for i in 0..t.len() {
            // Uppercase ASCII letter.
            assert!(t[i].is_ascii_uppercase());
            for j in (i + 1)..t.len() {
                assert_ne!(t[i], t[j]);
            }
        }
    }

    #[test]
    fn test_paths() {
        assert_eq!(PROC_MODULES_PATH, "/proc/modules");
        assert_eq!(SYS_MODULE_DIR, "/sys/module");
    }
}
