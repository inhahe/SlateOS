//! Switching the machine off, restarting it, and restarting SlateOS without
//! the firmware: the C library's `reboot`.
//!
//! Through the C library, as everything in this crate is: on SlateOS that is
//! `posix`'s `reboot`, which checks `CAP_SYS_BOOT` and the command as Linux
//! does and then asks the kernel (`SYS_POWER_OFF`, `SYS_POWER_REBOOT`,
//! `SYS_POWER_RELOAD`). A command that succeeds in switching off or
//! restarting does not return.
//!
//! On a host that is not unix every call answers [`ENOSYS`](crate::ENOSYS).

#[cfg(not(unix))]
use crate::ENOSYS;
#[cfg(unix)]
use crate::last_errno;

/// `RB_AUTOBOOT`: flush, then restart through the firmware.
pub const RB_AUTOBOOT: i32 = 0x0123_4567;
/// `RB_POWER_OFF`: flush, then switch the machine off.
pub const RB_POWER_OFF: i32 = 0x4321_FEDC_u32.cast_signed();
/// `RB_HALT_SYSTEM`: flush, then stop, the power left on.
pub const RB_HALT_SYSTEM: i32 = 0xCDEF_0123_u32.cast_signed();
/// `RB_KEXEC`: restart into another kernel without the firmware -- on
/// SlateOS, the running kernel's own image.
pub const RB_KEXEC: i32 = 0x4558_4543;

#[cfg(unix)]
mod sys {
    unsafe extern "C" {
        pub fn reboot(cmd: i32) -> i32;
    }
}

/// `reboot (cmd)`. Does not return when it switches off or restarts.
///
/// # Errors
///
/// `EPERM` without `CAP_SYS_BOOT`, checked first; `EINVAL` for a command
/// that is none of them; on SlateOS, `ENOSYS` while the C library has no way
/// to ask the kernel for this one; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn reboot(cmd: i32) -> Result<(), i32> {
    reboot_one(cmd)
}

#[cfg(unix)]
fn reboot_one(cmd: i32) -> Result<(), i32> {
    // SAFETY: a number; the call touches no memory of ours.
    let rc = unsafe { sys::reboot(cmd) };
    if rc < 0 { Err(last_errno()) } else { Ok(()) }
}

#[cfg(not(unix))]
fn reboot_one(_cmd: i32) -> Result<(), i32> {
    Err(ENOSYS)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The restated numbers are the library's: Linux's `<sys/reboot.h>`.
    #[test]
    fn the_numbers_are_the_librarys() {
        use posix::process as p;
        assert_eq!(RB_AUTOBOOT.cast_unsigned(), p::LINUX_REBOOT_CMD_RESTART);
        assert_eq!(RB_POWER_OFF.cast_unsigned(), p::LINUX_REBOOT_CMD_POWER_OFF);
        assert_eq!(RB_HALT_SYSTEM.cast_unsigned(), p::LINUX_REBOOT_CMD_HALT);
        assert_eq!(RB_KEXEC.cast_unsigned(), p::LINUX_REBOOT_CMD_KEXEC);
    }

    /// A command that is none of them is refused -- `EPERM` before anything
    /// for a caller without `CAP_SYS_BOOT`, `EINVAL` for one with it -- and
    /// nothing happens either way. Never a real command: a test must not
    /// switch off a machine it happens to be running on as root.
    #[cfg(unix)]
    #[test]
    fn a_command_that_is_none_of_them_is_refused() {
        let answer = reboot(0x1234_5678);
        assert!(
            matches!(answer, Err(1 | 22)),
            "EPERM or EINVAL, got {answer:?}"
        );
    }
}
