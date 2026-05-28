//! `<linux/module.h>` — Kernel module management.
//!
//! Provides constants and validated entry points for the
//! `init_module`, `finit_module`, and `delete_module` syscalls used
//! by `modprobe`, `insmod`, `rmmod`, `systemd-modules-load`,
//! `kmod`, and `depmod`'s symbol-resolution preload.
//!
//! Every entry point validates its input shape against Linux's
//! contract (fd bounds, flag-bit allowlists, non-NULL pointer
//! requirements, name length) and then returns `-1` /
//! `errno = ENOSYS`. Real module loading is intentionally not
//! supported — this is a microkernel, drivers run in userspace,
//! and "modules" as Linux understands them (in-kernel
//! dynamically-loaded code) are not part of our architecture per
//! `design.txt` ("Microkernel: drivers run in userspace. Only
//! scheduler, memory manager, IPC, capability enforcement, and
//! interrupt routing run in kernel space."). The validation surface
//! is still meaningful because tooling that probes for module
//! support at startup (`modprobe --dry-run`, the libkmod library
//! used by systemd, `lspci -k`'s "Kernel modules:" line, the Rust
//! `kmod-sys` crate) needs a real EINVAL/EFAULT/EBADF response for
//! malformed inputs and a clean ENOSYS for well-formed inputs.

use crate::errno;

// ---------------------------------------------------------------------------
// Module init flags (finit_module / init_module)
// ---------------------------------------------------------------------------

/// Module init: ignore unknown module parameters.
pub const MODULE_INIT_IGNORE_MODVERSIONS: u32 = 1;
/// Module init: ignore kernel version mismatch.
pub const MODULE_INIT_IGNORE_VERMAGIC: u32 = 2;
/// Module init: compressed module (let kernel decompress).
pub const MODULE_INIT_COMPRESSED_FILE: u32 = 4;
/// Valid flag mask for `init_module` and `finit_module`.
const MODULE_INIT_FLAGS_VALID: u32 =
    MODULE_INIT_IGNORE_MODVERSIONS | MODULE_INIT_IGNORE_VERMAGIC | MODULE_INIT_COMPRESSED_FILE;

// ---------------------------------------------------------------------------
// Module delete flags
// ---------------------------------------------------------------------------

/// Force module removal even if in use (Linux maps this to O_TRUNC=0x200).
pub const O_TRUNC_DELETE: u32 = 1;
/// Non-blocking: return EWOULDBLOCK if module is in use
/// (Linux maps this to O_NONBLOCK=0x800).
pub const O_NONBLOCK_DELETE: u32 = 2;
/// Valid flag mask for `delete_module`. We accept either the legacy
/// internal constants above or the Linux open(2)-style values that
/// real `rmmod`/`libkmod` callers pass, so off-the-shelf userspace
/// works against our shim without translation.
const MODULE_DELETE_FLAGS_VALID: u32 = O_TRUNC_DELETE | O_NONBLOCK_DELETE | 0x200 | 0x800;

// ---------------------------------------------------------------------------
// Module state (from /sys/module/*/initstate)
// ---------------------------------------------------------------------------

/// Module is live (loaded and initialized).
pub const MODULE_STATE_LIVE: u32 = 0;
/// Module is being initialized.
pub const MODULE_STATE_COMING: u32 = 1;
/// Module is being removed.
pub const MODULE_STATE_GOING: u32 = 2;
/// Module is unformed (allocation complete, but not initialized).
pub const MODULE_STATE_UNFORMED: u32 = 3;

// ---------------------------------------------------------------------------
// Bounds
// ---------------------------------------------------------------------------

/// Linux's `MODULE_NAME_LEN - 1` — the longest module name accepted by
/// `delete_module`. Anything longer hits EINVAL (some kernels return
/// ENAMETOOLONG instead; Linux's `delete_module` returns EINVAL).
const MODULE_NAME_MAX: usize = 60;
/// Maximum size of the module image (`len` argument to `init_module`).
/// Real Linux caps this at the system's RLIMIT_AS / available memory;
/// we cap at 256 MiB to reject runaway values that no legitimate
/// caller would pass.
const MODULE_IMAGE_MAX: usize = 256 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Walks a NUL-terminated C string up to `max + 1` bytes and returns
/// the length (excluding the NUL) or `None` if no NUL is found within
/// `max + 1` bytes (i.e. the name is too long).
///
/// # Safety
///
/// Caller must ensure `name` is non-NULL and points to at least
/// `max + 1` accessible bytes (or to a NUL-terminated string).
unsafe fn name_length(name: *const u8, max: usize) -> Option<usize> {
    let mut i = 0usize;
    while i <= max {
        // SAFETY: caller-provided pointer; we stop at the first NUL
        // or at `max + 1` iterations.
        let b = unsafe { *name.add(i) };
        if b == 0 {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Validates a module name: non-NULL, contains a NUL within
/// `MODULE_NAME_MAX + 1` bytes, no slash, not empty.
///
/// # Safety
///
/// Caller must ensure `name` is non-NULL.
unsafe fn validate_name(name: *const u8) -> Result<(), i32> {
    // SAFETY: caller has confirmed `name` is non-NULL.
    let len = match unsafe { name_length(name, MODULE_NAME_MAX) } {
        Some(0) => return Err(errno::EINVAL), // empty name
        Some(n) => n,
        None => return Err(errno::EINVAL),    // too long
    };
    // Reject any slash to forbid path traversal. Linux rejects names
    // containing `/` because they'd alias the on-disk module path.
    for i in 0..len {
        // SAFETY: i < len < MODULE_NAME_MAX, all within the validated string.
        let b = unsafe { *name.add(i) };
        if b == b'/' {
            return Err(errno::EINVAL);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Syscalls
// ---------------------------------------------------------------------------

/// Load a kernel module from an in-memory image (Linux `init_module(2)`).
///
/// Validates the caller-supplied image, length, and params before
/// returning `-1` / `errno = ENOSYS`. Real module loading is not
/// supported on this microkernel by design.
///
/// # Errors
///
/// - `EFAULT`: NULL `module_image` (when `len > 0`) or NULL `params`.
/// - `EINVAL`: `len == 0`, or unknown flag bit (this fn takes no flags
///   in Linux, but we accept a NUL-terminated params string and
///   require it to fit in a sane length).
/// - `E2BIG`: `len > MODULE_IMAGE_MAX (256 MiB)` — guards against
///   runaway values.
/// - `ENOSYS`: all checks pass.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn init_module(module_image: *const u8, len: usize, params: *const u8) -> i32 {
    if len == 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if len > MODULE_IMAGE_MAX {
        errno::set_errno(errno::E2BIG);
        return -1;
    }
    if module_image.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if params.is_null() {
        // params is "" by convention, not NULL, in real Linux callers.
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Load a kernel module from a file descriptor (Linux `finit_module(2)`).
///
/// Validates `flags`, `fd`, and `params` (in that order, matching
/// Linux's `SYSCALL_DEFINE3(finit_module)` prologue in
/// `kernel/module/main.c`: the kernel rejects unknown flag bits with
/// `EINVAL` *before* calling `fdget(fd)`, and `params` is only read
/// via `copy_from_user` after the fd is resolved) before returning
/// `-1` / `errno = ENOSYS`. Real module loading is not supported on
/// this microkernel by design.
///
/// # Errors
///
/// - `EINVAL`: unknown flag bit (checked first).
/// - `EBADF`: `fd < 0` (checked second).
/// - `EFAULT`: NULL `params` (checked last; Linux requires "" not NULL).
/// - `ENOSYS`: all checks pass — no module subsystem to dispatch to.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn finit_module(fd: i32, params: *const u8, flags: u32) -> i32 {
    // Linux's `SYSCALL_DEFINE3(finit_module)` rejects unknown flag
    // bits before calling `fdget(fd)`, so a buggy caller that passes
    // both a bad fd and a bad flag observes EINVAL, not EBADF. Match
    // that ordering exactly so userspace probing of module-support
    // behaviour (libkmod, systemd-modules-load) sees the same errno
    // sequence as on real Linux.
    if (flags & !MODULE_INIT_FLAGS_VALID) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if params.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Remove a kernel module (Linux `delete_module(2)`).
///
/// Validates `name` and `flags` before returning `-1` /
/// `errno = ENOSYS`. Real module unloading is not supported on this
/// microkernel by design.
///
/// # Errors
///
/// - `EFAULT`: NULL `name`.
/// - `EINVAL`: empty name, name too long (> 60 bytes), name contains
///   `/`, or unknown flag bit (accepts both our legacy constants and
///   the Linux O_TRUNC=0x200/O_NONBLOCK=0x800 values that real
///   `rmmod`/`libkmod` pass).
/// - `ENOSYS`: all checks pass.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn delete_module(name: *const u8, flags: u32) -> i32 {
    if name.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if (flags & !MODULE_DELETE_FLAGS_VALID) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // SAFETY: name is non-NULL.
    if let Err(e) = unsafe { validate_name(name) } {
        errno::set_errno(e);
        return -1;
    }
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;

    #[test]
    fn test_init_flags_powers_of_two() {
        let flags = [
            MODULE_INIT_IGNORE_MODVERSIONS,
            MODULE_INIT_IGNORE_VERMAGIC,
            MODULE_INIT_COMPRESSED_FILE,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "flag {f} not power of 2");
        }
    }

    #[test]
    fn test_states_sequential() {
        assert_eq!(MODULE_STATE_LIVE, 0);
        assert_eq!(MODULE_STATE_COMING, 1);
        assert_eq!(MODULE_STATE_GOING, 2);
        assert_eq!(MODULE_STATE_UNFORMED, 3);
    }

    // -----------------------------------------------------------------
    // init_module tests
    // -----------------------------------------------------------------

    #[test]
    fn test_init_module_zero_len_einval() {
        let buf = [0u8; 1];
        let params = b"\0";
        let r = init_module(buf.as_ptr(), 0, params.as_ptr());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_init_module_huge_len_e2big() {
        let buf = [0u8; 1];
        let params = b"\0";
        let r = init_module(buf.as_ptr(), usize::MAX, params.as_ptr());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::E2BIG);
    }

    #[test]
    fn test_init_module_null_image_efault() {
        let params = b"\0";
        let r = init_module(ptr::null(), 1024, params.as_ptr());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_init_module_null_params_efault() {
        let buf = [0u8; 4];
        let r = init_module(buf.as_ptr(), 4, ptr::null());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_init_module_valid_reaches_enosys() {
        let buf = [0u8; 4];
        let params = b"\0";
        let r = init_module(buf.as_ptr(), 4, params.as_ptr());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -----------------------------------------------------------------
    // finit_module tests
    // -----------------------------------------------------------------

    #[test]
    fn test_finit_module_negative_fd_ebadf() {
        let params = b"\0";
        let r = finit_module(-1, params.as_ptr(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_finit_module_null_params_efault() {
        let r = finit_module(3, ptr::null(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_finit_module_unknown_flag_einval() {
        let params = b"\0";
        let r = finit_module(3, params.as_ptr(), 0x8000);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_finit_module_each_known_flag_reaches_enosys() {
        let params = b"\0";
        for &f in &[
            0,
            MODULE_INIT_IGNORE_MODVERSIONS,
            MODULE_INIT_IGNORE_VERMAGIC,
            MODULE_INIT_COMPRESSED_FILE,
            MODULE_INIT_IGNORE_MODVERSIONS | MODULE_INIT_IGNORE_VERMAGIC,
        ] {
            let r = finit_module(3, params.as_ptr(), f);
            assert_eq!(r, -1, "flags={f:#x}");
            assert_eq!(errno::get_errno(), errno::ENOSYS, "flags={f:#x}");
        }
    }

    #[test]
    fn test_finit_module_valid_reaches_enosys() {
        let params = b"verbose=1\0";
        let r = finit_module(3, params.as_ptr(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -----------------------------------------------------------------
    // delete_module tests
    // -----------------------------------------------------------------

    #[test]
    fn test_delete_module_null_name_efault() {
        let r = delete_module(ptr::null(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_delete_module_empty_name_einval() {
        let name = b"\0";
        let r = delete_module(name.as_ptr(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_delete_module_slash_in_name_einval() {
        let name = b"e1000/foo\0";
        let r = delete_module(name.as_ptr(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_delete_module_too_long_name_einval() {
        // 61 'a' bytes followed by NUL: above MODULE_NAME_MAX=60.
        let mut buf = [b'a'; 62];
        buf[61] = 0;
        let r = delete_module(buf.as_ptr(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_delete_module_unknown_flag_einval() {
        let name = b"e1000\0";
        let r = delete_module(name.as_ptr(), 0x1000);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_delete_module_legacy_trunc_ok() {
        let name = b"e1000\0";
        let r = delete_module(name.as_ptr(), O_TRUNC_DELETE);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_delete_module_linux_nonblock_ok() {
        let name = b"e1000\0";
        // 0x800 == O_NONBLOCK on Linux x86_64.
        let r = delete_module(name.as_ptr(), 0x800);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_delete_module_linux_trunc_ok() {
        let name = b"e1000\0";
        // 0x200 == O_TRUNC on Linux x86_64.
        let r = delete_module(name.as_ptr(), 0x200);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_delete_module_force_and_nonblock_combo_ok() {
        let name = b"e1000\0";
        let r = delete_module(name.as_ptr(), 0x200 | 0x800);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_delete_module_valid_reaches_enosys() {
        let name = b"e1000\0";
        let r = delete_module(name.as_ptr(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_delete_module_name_at_max_ok() {
        // 60 'a' bytes followed by NUL: right at MODULE_NAME_MAX.
        let mut buf = [b'a'; 61];
        buf[60] = 0;
        let r = delete_module(buf.as_ptr(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -----------------------------------------------------------------
    // Workflow tests
    // -----------------------------------------------------------------

    #[test]
    fn test_modprobe_dry_run_workflow() {
        // modprobe -n e1000:
        //   open("e1000.ko") -> fd
        //   finit_module(fd, "", 0) -> -1, ENOSYS
        //   modprobe prints "FATAL: Module e1000 not found." and exits.
        let params = b"\0";
        let r = finit_module(3, params.as_ptr(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_rmmod_force_workflow() {
        // rmmod -f e1000: passes O_TRUNC=0x200 to delete_module.
        let name = b"e1000\0";
        let r = delete_module(name.as_ptr(), 0x200);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_libkmod_probe_workflow() {
        // libkmod's kmod_module_get_initstate first tries finit_module
        // with IGNORE_MODVERSIONS to check if the kernel supports
        // module loading at all.
        let params = b"\0";
        let r = finit_module(3, params.as_ptr(), MODULE_INIT_IGNORE_MODVERSIONS);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
        // libkmod sees ENOSYS and reports "module loading not supported".
    }

    #[test]
    fn test_errno_preserved_on_validation_success() {
        errno::set_errno(errno::EBADF);
        let buf = [0u8; 4];
        let params = b"\0";
        let r = init_module(buf.as_ptr(), 4, params.as_ptr());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -----------------------------------------------------------------
    // Phase 111 — finit_module validation-order parity with Linux
    //
    // Linux's `SYSCALL_DEFINE3(finit_module)` (kernel/module/main.c):
    //   1. flags & ~(KNOWN) -> -EINVAL
    //   2. fdget(fd)         -> -EBADF if fd < 0 or not open
    //   3. copy_from_user(uargs) inside load_module path -> -EFAULT
    //
    // These tests pin that exact order so a userspace caller probing
    // module support (libkmod, systemd-modules-load, modprobe --dry-run)
    // sees the same errno on every malformed-input combination as it
    // would on real Linux. Phase 110 reordered io_uring_enter/register
    // the same way; Phase 111 is the kernel-module analogue.
    // -----------------------------------------------------------------

    #[test]
    fn test_finit_module_phase111_einval_wins_over_ebadf() {
        // Bad flag AND negative fd: Linux checks flags first -> EINVAL.
        let params = b"\0";
        let r = finit_module(-1, params.as_ptr(), 0x8000);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_finit_module_phase111_einval_wins_over_efault() {
        // Bad flag AND NULL params: Linux checks flags first -> EINVAL.
        let r = finit_module(3, ptr::null(), 0x8000);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_finit_module_phase111_einval_wins_over_ebadf_and_efault() {
        // All three malformed: Linux still returns EINVAL first.
        let r = finit_module(-1, ptr::null(), 0x8000);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_finit_module_phase111_ebadf_wins_over_efault() {
        // Good flags, bad fd, NULL params: flags pass, fdget fails -> EBADF.
        let r = finit_module(-1, ptr::null(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_finit_module_phase111_high_bit_unknown_flag_einval() {
        // Sign bit (0x8000_0000) is an unknown flag bit -> EINVAL,
        // not an arithmetic-shift bug or sign-extension surprise.
        let params = b"\0";
        let r = finit_module(3, params.as_ptr(), 0x8000_0000);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_finit_module_phase111_first_invalid_bit_above_mask_einval() {
        // 0x08 is the first bit just past MODULE_INIT_COMPRESSED_FILE
        // (=0x04) and is unused by Linux as of 6.x -> EINVAL.
        let params = b"\0";
        let r = finit_module(3, params.as_ptr(), 0x08);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_finit_module_phase111_all_known_flag_bits_reach_enosys() {
        // 0x07 = IGNORE_MODVERSIONS | IGNORE_VERMAGIC | COMPRESSED_FILE.
        // No unknown bits -> pass the mask check -> reach ENOSYS.
        let params = b"\0";
        let r = finit_module(3, params.as_ptr(), MODULE_INIT_FLAGS_VALID);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_finit_module_phase111_i32_min_fd_ebadf() {
        // Catastrophic fd value: must still report EBADF, not panic or
        // accidentally pass via sign-extension into a valid range.
        let params = b"\0";
        let r = finit_module(i32::MIN, params.as_ptr(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_finit_module_phase111_i32_max_fd_reaches_enosys() {
        // i32::MAX is >= 0, so it passes the fd bound check and the
        // call proceeds to the ENOSYS stub (we don't open the fd).
        let params = b"\0";
        let r = finit_module(i32::MAX, params.as_ptr(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_finit_module_phase111_recovery_after_einval() {
        // After a flag-mask EINVAL, a subsequent valid call still
        // produces a clean ENOSYS (errno is rewritten, not sticky).
        let params = b"\0";
        let r1 = finit_module(3, params.as_ptr(), 0x8000);
        assert_eq!(r1, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        let r2 = finit_module(3, params.as_ptr(), 0);
        assert_eq!(r2, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_finit_module_phase111_modprobe_force_vermagic_workflow() {
        // `modprobe --force-vermagic e1000` opens the .ko, then calls
        //   finit_module(fd, "", MODULE_INIT_IGNORE_VERMAGIC)
        // libkmod combines IGNORE_VERMAGIC + COMPRESSED_FILE when the
        // .ko on disk is xz-compressed (which is the Debian default).
        let params = b"\0";
        let flags = MODULE_INIT_IGNORE_VERMAGIC | MODULE_INIT_COMPRESSED_FILE;
        let r = finit_module(3, params.as_ptr(), flags);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_finit_module_phase111_systemd_modules_load_workflow() {
        // systemd-modules-load reads /etc/modules-load.d/*.conf and
        // calls finit_module(fd, "", 0) for each entry. It expects
        // ENOSYS to silently disable module loading rather than
        // logging EINVAL/EBADF noise into the journal.
        let params = b"\0";
        let r = finit_module(7, params.as_ptr(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_finit_module_phase111_buggy_caller_passes_negative_one_flags() {
        // A C caller doing `finit_module(fd, "", -1)` (which casts to
        // 0xFFFF_FFFF unsigned) hits the unknown-bit check first and
        // gets EINVAL — exactly the same as on Linux. Confirms we
        // don't accidentally accept all-ones as a wildcard.
        let params = b"\0";
        let r = finit_module(3, params.as_ptr(), u32::MAX);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }
}
