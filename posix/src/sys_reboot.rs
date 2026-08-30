//! `<sys/reboot.h>` — system reboot.
//!
//! Re-exports `reboot()` and reboot command constants from the
//! `process` module.

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

pub use crate::process::LINUX_REBOOT_CMD_CAD_OFF;
pub use crate::process::LINUX_REBOOT_CMD_CAD_ON;
pub use crate::process::LINUX_REBOOT_CMD_HALT;
pub use crate::process::LINUX_REBOOT_CMD_KEXEC;
pub use crate::process::LINUX_REBOOT_CMD_POWER_OFF;
pub use crate::process::LINUX_REBOOT_CMD_RESTART;
pub use crate::process::LINUX_REBOOT_CMD_RESTART2;
pub use crate::process::LINUX_REBOOT_CMD_SW_SUSPEND;
pub use crate::process::LINUX_REBOOT_MAGIC1;
pub use crate::process::LINUX_REBOOT_MAGIC2;
pub use crate::process::LINUX_REBOOT_MAGIC2A;
pub use crate::process::LINUX_REBOOT_MAGIC2B;
pub use crate::process::LINUX_REBOOT_MAGIC2C;
pub use crate::process::reboot;
pub use crate::process::reboot_cmd_known;

// ---------------------------------------------------------------------------
// BSD-style aliases
// ---------------------------------------------------------------------------

/// Restart the system (BSD alias).
pub const RB_AUTOBOOT: u32 = LINUX_REBOOT_CMD_RESTART;

/// Halt the system (BSD alias).
pub const RB_HALT_SYSTEM: u32 = LINUX_REBOOT_CMD_HALT;

/// Power off the system (BSD alias).
pub const RB_POWER_OFF: u32 = LINUX_REBOOT_CMD_POWER_OFF;

/// Enable Ctrl-Alt-Delete (BSD alias).
pub const RB_ENABLE_CAD: u32 = LINUX_REBOOT_CMD_CAD_ON;

/// Disable Ctrl-Alt-Delete (BSD alias).
pub const RB_DISABLE_CAD: u32 = LINUX_REBOOT_CMD_CAD_OFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reboot_magic() {
        assert_eq!(LINUX_REBOOT_MAGIC1, 0xfee1_dead);
    }

    #[test]
    fn test_bsd_aliases() {
        assert_eq!(RB_AUTOBOOT, LINUX_REBOOT_CMD_RESTART);
        assert_eq!(RB_HALT_SYSTEM, LINUX_REBOOT_CMD_HALT);
        assert_eq!(RB_POWER_OFF, LINUX_REBOOT_CMD_POWER_OFF);
        assert_eq!(RB_ENABLE_CAD, LINUX_REBOOT_CMD_CAD_ON);
        assert_eq!(RB_DISABLE_CAD, LINUX_REBOOT_CMD_CAD_OFF);
    }

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            LINUX_REBOOT_CMD_RESTART,
            LINUX_REBOOT_CMD_HALT,
            LINUX_REBOOT_CMD_POWER_OFF,
            LINUX_REBOOT_CMD_CAD_ON,
            LINUX_REBOOT_CMD_CAD_OFF,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_reboot_stub() {
        let ret = reboot(LINUX_REBOOT_CMD_RESTART as i32);
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // Phase 77 — reboot argument-domain validation
    //
    // Linux `kernel/reboot.c::sys_reboot` rejects unknown commands with
    // EINVAL and missing CAP_SYS_BOOT with EPERM.  We validate `cmd`
    // against the known set first, then surface ENOSYS once the call
    // is otherwise well-formed.  The capability check exists in case a
    // test (or future privilege-dropper) clears CAP_SYS_BOOT — the
    // default process holds it.
    // -----------------------------------------------------------------------

    use crate::errno;
    use crate::sys_capability::{CAP_SYS_BOOT, has_capability};

    /// Save the current effective-cap state and restore it on drop so
    /// EPERM tests don't bleed into the cooperating-default tests.
    struct CapGuard {
        lo: u32,
        hi: u32,
    }
    impl CapGuard {
        fn snapshot() -> Self {
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            Self { lo, hi }
        }
    }
    impl Drop for CapGuard {
        fn drop(&mut self) {
            // Restore via capset path so any side effects (atime, etc.)
            // are exercised in the same way they would be on rollback.
            let mut hdr = crate::sys_capability::CapUserHeader {
                version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: self.lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: self.hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let _ = crate::sys_capability::capset(&mut hdr, data.as_ptr());
        }
    }

    fn drop_cap_sys_boot() {
        let (lo, hi) = crate::sys_capability::current_caps_effective();
        let (new_lo, new_hi) = if CAP_SYS_BOOT < 32 {
            (lo & !(1u32 << CAP_SYS_BOOT), hi)
        } else {
            (lo, hi & !(1u32 << (CAP_SYS_BOOT - 32)))
        };
        let mut hdr = crate::sys_capability::CapUserHeader {
            version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
            pid: 0,
        };
        let data = [
            crate::sys_capability::CapUserData {
                effective: new_lo,
                permitted: u32::MAX,
                inheritable: 0,
            },
            crate::sys_capability::CapUserData {
                effective: new_hi,
                permitted: u32::MAX,
                inheritable: 0,
            },
        ];
        let rc = crate::sys_capability::capset(&mut hdr, data.as_ptr());
        assert_eq!(rc, 0, "capset must succeed when dropping CAP_SYS_BOOT");
        assert!(!has_capability(CAP_SYS_BOOT));
    }

    // -- Helper: reboot_cmd_known ------------------------------------------

    #[test]
    fn test_phase77_reboot_cmd_known_accepts_all_documented_cmds() {
        assert!(reboot_cmd_known(LINUX_REBOOT_CMD_RESTART));
        assert!(reboot_cmd_known(LINUX_REBOOT_CMD_HALT));
        assert!(reboot_cmd_known(LINUX_REBOOT_CMD_POWER_OFF));
        assert!(reboot_cmd_known(LINUX_REBOOT_CMD_CAD_ON));
        assert!(reboot_cmd_known(LINUX_REBOOT_CMD_CAD_OFF));
        assert!(reboot_cmd_known(LINUX_REBOOT_CMD_RESTART2));
        assert!(reboot_cmd_known(LINUX_REBOOT_CMD_SW_SUSPEND));
        assert!(reboot_cmd_known(LINUX_REBOOT_CMD_KEXEC));
    }

    #[test]
    fn test_phase77_reboot_cmd_known_rejects_unknown() {
        // Garbage / near-misses must be rejected.
        assert!(!reboot_cmd_known(0xDEAD_BEEF));
        assert!(!reboot_cmd_known(0x01234566)); // RESTART - 1
        assert!(!reboot_cmd_known(0x01234568)); // RESTART + 1
        assert!(!reboot_cmd_known(0xCDEF0124)); // HALT + 1
        assert!(!reboot_cmd_known(1));
        assert!(!reboot_cmd_known(2));
        assert!(!reboot_cmd_known(0xFFFFFFFF));
        // CAD_OFF is 0; only the exact zero value is accepted there, so
        // 0 is *known* but nearby values are not.
        assert!(reboot_cmd_known(0));
        assert!(!reboot_cmd_known(0x10000));
    }

    // -- Per-cmd EINVAL/ENOSYS branches -------------------------------------

    #[test]
    fn test_phase77_reboot_unknown_cmd_einval() {
        let _g = CapGuard::snapshot();
        let ret = reboot(0xDEAD_BEEFu32 as i32);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_phase77_reboot_negative_one_einval() {
        // -1 is 0xFFFFFFFF as u32 — explicitly outside the known set.
        let _g = CapGuard::snapshot();
        let ret = reboot(-1);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_phase77_reboot_one_einval() {
        // cmd=1 is not in the known set (the value 1 looks tempting but
        // none of the LINUX_REBOOT_CMD_* constants equal 1).
        let _g = CapGuard::snapshot();
        let ret = reboot(1);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_phase77_reboot_restart_enosys_when_capable() {
        let _g = CapGuard::snapshot();
        assert!(has_capability(CAP_SYS_BOOT));
        let ret = reboot(LINUX_REBOOT_CMD_RESTART as i32);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_phase77_reboot_halt_enosys_when_capable() {
        let _g = CapGuard::snapshot();
        // HALT has the high bit set; ensure the i32 cast survives the
        // round-trip through u32 in the validator.
        let ret = reboot(LINUX_REBOOT_CMD_HALT as i32);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_phase77_reboot_power_off_enosys_when_capable() {
        let _g = CapGuard::snapshot();
        let ret = reboot(LINUX_REBOOT_CMD_POWER_OFF as i32);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_phase77_reboot_cad_on_enosys_when_capable() {
        let _g = CapGuard::snapshot();
        let ret = reboot(LINUX_REBOOT_CMD_CAD_ON as i32);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_phase77_reboot_cad_off_enosys_when_capable() {
        let _g = CapGuard::snapshot();
        // CAD_OFF == 0; this is a notable edge case because callers
        // might confuse "zero cmd" with "uninitialised cmd".  Validate
        // it still hits the ENOSYS path, not EINVAL.
        let ret = reboot(LINUX_REBOOT_CMD_CAD_OFF as i32);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_phase77_reboot_restart2_enosys_when_capable() {
        let _g = CapGuard::snapshot();
        let ret = reboot(LINUX_REBOOT_CMD_RESTART2 as i32);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_phase77_reboot_sw_suspend_enosys_when_capable() {
        let _g = CapGuard::snapshot();
        let ret = reboot(LINUX_REBOOT_CMD_SW_SUSPEND as i32);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_phase77_reboot_kexec_enosys_when_capable() {
        let _g = CapGuard::snapshot();
        let ret = reboot(LINUX_REBOOT_CMD_KEXEC as i32);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- Capability check ---------------------------------------------------

    #[test]
    fn test_phase77_reboot_eperm_without_cap_sys_boot() {
        let _g = CapGuard::snapshot();
        drop_cap_sys_boot();
        let ret = reboot(LINUX_REBOOT_CMD_RESTART as i32);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_phase77_reboot_eperm_for_each_known_cmd() {
        let _g = CapGuard::snapshot();
        drop_cap_sys_boot();
        for &cmd in &[
            LINUX_REBOOT_CMD_RESTART,
            LINUX_REBOOT_CMD_HALT,
            LINUX_REBOOT_CMD_POWER_OFF,
            LINUX_REBOOT_CMD_CAD_ON,
            LINUX_REBOOT_CMD_CAD_OFF,
            LINUX_REBOOT_CMD_RESTART2,
            LINUX_REBOOT_CMD_SW_SUSPEND,
            LINUX_REBOOT_CMD_KEXEC,
        ] {
            let ret = reboot(cmd as i32);
            assert_eq!(ret, -1, "cmd 0x{cmd:08X} should fail");
            assert_eq!(
                errno::get_errno(),
                errno::EPERM,
                "cmd 0x{cmd:08X} should report EPERM when CAP_SYS_BOOT is missing"
            );
        }
    }

    // -- Validation-order parity -------------------------------------------

    #[test]
    fn test_phase77_reboot_einval_precedes_eperm() {
        // Even without CAP_SYS_BOOT, an unknown cmd reports EINVAL —
        // not EPERM.  This matches the glibc-mediated observable order
        // (unknown cmds never reach the kernel's capability check).
        let _g = CapGuard::snapshot();
        drop_cap_sys_boot();
        let ret = reboot(0xDEAD_BEEFu32 as i32);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -- Magic constants ----------------------------------------------------

    #[test]
    fn test_phase77_magic1_value() {
        assert_eq!(LINUX_REBOOT_MAGIC1, 0xfee1_dead);
    }

    #[test]
    fn test_phase77_magic2_values() {
        // All four magic2 variants are distinct integers, as Linux
        // accepts any of them as the second magic argument.
        assert_eq!(LINUX_REBOOT_MAGIC2, 672_274_793);
        assert_eq!(LINUX_REBOOT_MAGIC2A, 85_072_278);
        assert_eq!(LINUX_REBOOT_MAGIC2B, 369_367_448);
        assert_eq!(LINUX_REBOOT_MAGIC2C, 537_993_216);
        let m = [
            LINUX_REBOOT_MAGIC2,
            LINUX_REBOOT_MAGIC2A,
            LINUX_REBOOT_MAGIC2B,
            LINUX_REBOOT_MAGIC2C,
        ];
        for i in 0..m.len() {
            for j in (i + 1)..m.len() {
                assert_ne!(m[i], m[j]);
            }
        }
    }

    #[test]
    fn test_phase77_all_cmd_constants_distinct() {
        let cmds = [
            LINUX_REBOOT_CMD_RESTART,
            LINUX_REBOOT_CMD_HALT,
            LINUX_REBOOT_CMD_POWER_OFF,
            LINUX_REBOOT_CMD_CAD_ON,
            LINUX_REBOOT_CMD_CAD_OFF,
            LINUX_REBOOT_CMD_RESTART2,
            LINUX_REBOOT_CMD_SW_SUSPEND,
            LINUX_REBOOT_CMD_KEXEC,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j], "reboot CMDs must be distinct");
            }
        }
    }

    // -- Buggy-caller cases -------------------------------------------------

    #[test]
    fn test_phase77_buggy_caller_passes_magic_as_cmd() {
        // A common bug: caller swaps the magic and cmd arguments.
        // Magic1 is 0xfee1dead which is not in our known cmd set, so
        // the validator must report EINVAL — surfacing the bug cleanly
        // rather than silently halting the system.
        let _g = CapGuard::snapshot();
        let ret = reboot(LINUX_REBOOT_MAGIC1 as i32);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_phase77_buggy_caller_passes_magic2_as_cmd() {
        let _g = CapGuard::snapshot();
        // Magic2 = 672274793 = 0x28121969 — also not a known cmd.
        let ret = reboot(LINUX_REBOOT_MAGIC2 as i32);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_phase77_workflow_init_shutdown_sequence() {
        // Simulate an init-style shutdown sequence: each step must
        // surface ENOSYS (not silently succeed) so the caller can fall
        // back to platform-specific shutdown.
        let _g = CapGuard::snapshot();
        for &cmd in &[
            LINUX_REBOOT_CMD_CAD_OFF,   // disable Ctrl-Alt-Del first
            LINUX_REBOOT_CMD_POWER_OFF, // request power-off
        ] {
            let ret = reboot(cmd as i32);
            assert_eq!(ret, -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }
    }
}
