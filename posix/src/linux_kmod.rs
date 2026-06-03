//! `<linux/kmod.h>` — Kernel module loading constants.
//!
//! The kmod subsystem handles loading and unloading of kernel
//! modules. Modules are loaded via init_module/finit_module
//! syscalls or usermode helper (modprobe). Module dependencies
//! and aliases are resolved by the userspace modprobe tool.

// ---------------------------------------------------------------------------
// Module loading flags (for finit_module)
// ---------------------------------------------------------------------------

/// Ignore module signature verification.
pub const MODULE_INIT_IGNORE_MODVERSIONS: u32 = 1 << 0;
/// Ignore module version magic.
pub const MODULE_INIT_IGNORE_VERMAGIC: u32 = 1 << 1;
/// Compressed module (kernel handles decompression).
pub const MODULE_INIT_COMPRESSED_FILE: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Delete module flags (for delete_module)
// ---------------------------------------------------------------------------

/// Force module removal (even if in use).
pub const O_TRUNC_MODULE: u32 = 1;
/// Non-blocking removal (fail if busy).
pub const O_NONBLOCK_MODULE: u32 = 2;

// ---------------------------------------------------------------------------
// Module states
// ---------------------------------------------------------------------------

/// Module is live (loaded and active).
pub const MODULE_STATE_LIVE: u32 = 0;
/// Module is being loaded (init running).
pub const MODULE_STATE_COMING: u32 = 1;
/// Module is being removed (exit running).
pub const MODULE_STATE_GOING: u32 = 2;
/// Module is unformed (not yet linked).
pub const MODULE_STATE_UNFORMED: u32 = 3;

// ---------------------------------------------------------------------------
// Module paths
// ---------------------------------------------------------------------------

/// Default module directory prefix.
pub const MODULE_DIR: &str = "/lib/modules";
/// Module dependency file.
pub const MODULES_DEP: &str = "modules.dep";
/// Module alias file.
pub const MODULES_ALIAS: &str = "modules.alias";
/// Module symbol file.
pub const MODULES_SYMBOLS: &str = "modules.symbols";
/// Built-in modules list.
pub const MODULES_BUILTIN: &str = "modules.builtin";
/// Module ordering file.
pub const MODULES_ORDER: &str = "modules.order";

// ---------------------------------------------------------------------------
// Modprobe paths
// ---------------------------------------------------------------------------

/// Default modprobe binary.
pub const MODPROBE_PATH: &str = "/sbin/modprobe";
/// Module blacklist configuration.
pub const MODPROBE_BLACKLIST_CONF: &str = "/etc/modprobe.d/blacklist.conf";

// ---------------------------------------------------------------------------
// Module file extension
// ---------------------------------------------------------------------------

/// Uncompressed kernel module extension.
pub const MODULE_EXT_KO: &str = ".ko";
/// gzip-compressed module.
pub const MODULE_EXT_KO_GZ: &str = ".ko.gz";
/// xz-compressed module.
pub const MODULE_EXT_KO_XZ: &str = ".ko.xz";
/// zstd-compressed module.
pub const MODULE_EXT_KO_ZSTD: &str = ".ko.zst";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_flags_powers_of_two() {
        let flags = [
            MODULE_INIT_IGNORE_MODVERSIONS,
            MODULE_INIT_IGNORE_VERMAGIC,
            MODULE_INIT_COMPRESSED_FILE,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_init_flags_no_overlap() {
        let flags = [
            MODULE_INIT_IGNORE_MODVERSIONS,
            MODULE_INIT_IGNORE_VERMAGIC,
            MODULE_INIT_COMPRESSED_FILE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_delete_flags_distinct() {
        assert_ne!(O_TRUNC_MODULE, O_NONBLOCK_MODULE);
    }

    #[test]
    fn test_module_states_distinct() {
        let states = [
            MODULE_STATE_LIVE,
            MODULE_STATE_COMING,
            MODULE_STATE_GOING,
            MODULE_STATE_UNFORMED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_module_files_distinct() {
        let files = [
            MODULES_DEP,
            MODULES_ALIAS,
            MODULES_SYMBOLS,
            MODULES_BUILTIN,
            MODULES_ORDER,
        ];
        for i in 0..files.len() {
            for j in (i + 1)..files.len() {
                assert_ne!(files[i], files[j]);
            }
        }
    }

    #[test]
    fn test_extensions_distinct() {
        let exts = [
            MODULE_EXT_KO,
            MODULE_EXT_KO_GZ,
            MODULE_EXT_KO_XZ,
            MODULE_EXT_KO_ZSTD,
        ];
        for i in 0..exts.len() {
            for j in (i + 1)..exts.len() {
                assert_ne!(exts[i], exts[j]);
            }
        }
    }

    #[test]
    fn test_extensions_start_with_ko() {
        let exts = [
            MODULE_EXT_KO,
            MODULE_EXT_KO_GZ,
            MODULE_EXT_KO_XZ,
            MODULE_EXT_KO_ZSTD,
        ];
        for ext in &exts {
            assert!(ext.starts_with(".ko"), "{}", ext);
        }
    }
}
