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

/// Size of the ELF header that Linux's `copy_module_from_user` requires
/// the module image to be at least as large as before accepting it.
/// Linux uses `sizeof(struct elf64_hdr)` on 64-bit kernels (= 64 bytes);
/// any shorter image is rejected with `-ENOEXEC` because it cannot
/// possibly contain a valid ELF header. We mirror that bound exactly
/// so probes like libkmod's `kmod_module_probe_insert_module()` see the
/// same errno on truncated/zero-length images as on real Linux.
const ELF64_HDR_SIZE: usize = 64;

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
/// # Linux semantics
///
/// Linux's `SYSCALL_DEFINE3(init_module, void __user *umod,
/// unsigned long len, const char __user *uargs)` in
/// `kernel/module/main.c` has no flag arg. Its validation order is:
///
/// 1. `may_init_module()` → `-EPERM` if `CAP_SYS_MODULE` is missing or
///    modules are disabled.
/// 2. `copy_module_from_user(umod, len, &info)`:
///    - `info->len < sizeof(*(info->hdr))` → `-ENOEXEC` (64 bytes on
///      64-bit kernels; the image cannot contain a valid ELF header).
///    - `__vmalloc(info->len, ...)` fails → `-ENOMEM`.
///    - `copy_chunked_from_user(info->hdr, umod, info->len)` fails →
///      `-EFAULT` (this is where a NULL `umod` is observed, *after*
///      the length check).
/// 3. `load_module()` eventually calls `strndup_user(uargs, ...)` which
///    rejects NULL `uargs` with `-EFAULT`.
///
/// We honour that exact ordering — length-too-small is checked before
/// NULL `module_image`, so a caller passing both `len = 0` and
/// `module_image = NULL` sees `ENOEXEC`, not `EFAULT`, just like Linux.
/// (Phase 128 — previously we returned `EINVAL` for `len == 0`, which
/// no Linux call site does.)
///
/// # Errors
///
/// - **Phase 174:** `EPERM`: caller lacks `CAP_SYS_MODULE`.  Linux's
///   `may_init_module` runs *first* — before `copy_module_from_user`
///   — so an unprivileged caller passing `len = 0` sees `EPERM`, not
///   `ENOEXEC`.  This matches `kernel/module/main.c::may_init_module`
///   exactly: cap probe gates everything else.
/// - `ENOEXEC`: `len < 64` (the ELF64 header size). Returned before any
///   pointer check, matching Linux's `copy_module_from_user` prologue.
/// - `E2BIG`: `len > MODULE_IMAGE_MAX (256 MiB)` — guards against
///   runaway values that on Linux would hit `-ENOMEM` from `__vmalloc`.
///   We pick `E2BIG` over `ENOMEM` because the cap is a hard policy
///   bound, not a transient out-of-memory condition.
/// - `EFAULT`: NULL `module_image` (when `len >= 64`) or NULL `params`.
/// - `ENOSYS`: all checks pass — real module loading is not supported.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn init_module(module_image: *const u8, len: usize, params: *const u8) -> i32 {
    // Phase 174: Linux's `may_init_module` runs at the top of
    // SYSCALL_DEFINE3(init_module) — before `copy_module_from_user`
    // touches the user pointer or even reads `len`.  Mirror that order
    // so unprivileged callers always observe EPERM regardless of the
    // other arguments.
    if !crate::sys_capability::has_capability(
        crate::sys_capability::CAP_SYS_MODULE,
    ) {
        errno::set_errno(errno::EPERM);
        return -1;
    }
    // Linux's `copy_module_from_user` checks `info->len <
    // sizeof(*(info->hdr))` before touching the userspace pointer, so
    // a buggy caller passing both `len = 0` and a NULL image observes
    // ENOEXEC, not EFAULT. Mirror that ordering exactly.
    if len < ELF64_HDR_SIZE {
        errno::set_errno(errno::ENOEXEC);
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
/// - **Phase 174:** `EPERM`: caller lacks `CAP_SYS_MODULE`.  Linux's
///   `may_init_module` runs first — before the flag check or
///   `fdget(fd)` — so an unprivileged caller probing module support
///   sees `EPERM` regardless of `flags` / `fd` / `params`.
/// - `EINVAL`: unknown flag bit (checked first among the argument
///   guards).
/// - `EBADF`: `fd < 0` (checked second).
/// - `EFAULT`: NULL `params` (checked last; Linux requires "" not NULL).
/// - `ENOSYS`: all checks pass — no module subsystem to dispatch to.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn finit_module(fd: i32, params: *const u8, flags: u32) -> i32 {
    // Phase 174: Linux's `may_init_module` runs at the top of
    // SYSCALL_DEFINE3(finit_module), before the flag check or the
    // fdget.  Unprivileged callers see EPERM regardless of other args.
    if !crate::sys_capability::has_capability(
        crate::sys_capability::CAP_SYS_MODULE,
    ) {
        errno::set_errno(errno::EPERM);
        return -1;
    }
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
/// - **Phase 174:** `EPERM`: caller lacks `CAP_SYS_MODULE`.  Linux's
///   `SYSCALL_DEFINE2(delete_module)` performs
///   `if (!capable(CAP_SYS_MODULE) || modules_disabled) return -EPERM;`
///   *before* `strncpy_from_user(name, name_user, ...)`, so EPERM
///   beats EFAULT and EINVAL.
/// - `EFAULT`: NULL `name`.
/// - `EINVAL`: empty name, name too long (> 60 bytes), name contains
///   `/`, or unknown flag bit (accepts both our legacy constants and
///   the Linux O_TRUNC=0x200/O_NONBLOCK=0x800 values that real
///   `rmmod`/`libkmod` pass).
/// - `ENOSYS`: all checks pass.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn delete_module(name: *const u8, flags: u32) -> i32 {
    // Phase 174: cap check is the very first thing Linux's
    // SYSCALL_DEFINE2(delete_module) does, before reading `name`.
    if !crate::sys_capability::has_capability(
        crate::sys_capability::CAP_SYS_MODULE,
    ) {
        errno::set_errno(errno::EPERM);
        return -1;
    }
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
    fn test_init_module_zero_len_enoexec() {
        let buf = [0u8; 1];
        let params = b"\0";
        let r = init_module(buf.as_ptr(), 0, params.as_ptr());
        assert_eq!(r, -1);
        // Phase 128: Linux's copy_module_from_user returns -ENOEXEC for
        // len < sizeof(elf64_hdr) (64). len = 0 hits the same path.
        assert_eq!(errno::get_errno(), errno::ENOEXEC);
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
        let buf = [0u8; 128];
        let r = init_module(buf.as_ptr(), 128, ptr::null());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_init_module_valid_reaches_enosys() {
        let buf = [0u8; 128];
        let params = b"\0";
        let r = init_module(buf.as_ptr(), 128, params.as_ptr());
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
        let buf = [0u8; 128];
        let params = b"\0";
        let r = init_module(buf.as_ptr(), 128, params.as_ptr());
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

    // -----------------------------------------------------------------
    // Phase 128 — init_module ENOEXEC parity with Linux
    //
    // Linux's `SYSCALL_DEFINE3(init_module, ...)` defers all length
    // validation to `copy_module_from_user()`, which begins:
    //
    //     info->len = len;
    //     if (info->len < sizeof(*(info->hdr)))
    //         return -ENOEXEC;
    //
    // `info->hdr` is `Elf_Ehdr *`, which is `struct elf64_hdr` on
    // 64-bit kernels — exactly 64 bytes. Any image shorter than that
    // cannot possibly contain a valid ELF header, so Linux short-
    // circuits with -ENOEXEC *before* it ever touches the userspace
    // pointer. Previous behaviour returned -EINVAL for `len == 0` and
    // didn't distinguish 1..=63, which would mislead probes
    // (libkmod's kmod_module_probe_insert_module, modprobe's stat()
    // -> finit_module fallback that drops to init_module on old
    // kernels) into thinking they passed a flag-validation problem
    // rather than "this isn't an ELF image at all".
    //
    // These tests pin the ENOEXEC behaviour and the precedence of the
    // length check over the NULL-image check, exactly as on Linux.
    // -----------------------------------------------------------------

    #[test]
    fn test_init_module_phase128_len_one_below_hdr_enoexec() {
        // 63 bytes is one short of sizeof(struct elf64_hdr); Linux
        // rejects with ENOEXEC.
        let buf = [0u8; 64];
        let params = b"\0";
        let r = init_module(buf.as_ptr(), 63, params.as_ptr());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOEXEC);
    }

    #[test]
    fn test_init_module_phase128_len_at_hdr_reaches_enosys() {
        // Exactly 64 bytes passes the length check. The image is not
        // a real ELF, but our shim doesn't attempt to parse — it just
        // confirms ENOSYS dispatch.
        let buf = [0u8; 64];
        let params = b"\0";
        let r = init_module(buf.as_ptr(), 64, params.as_ptr());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_init_module_phase128_len_zero_with_null_image_is_enoexec_not_efault() {
        // Linux's `copy_module_from_user` checks `len < sizeof(hdr)`
        // BEFORE the userspace copy, so a caller passing both
        // len == 0 AND NULL umod observes ENOEXEC, not EFAULT.
        // Phase 128 fixes our precedence to match.
        let r = init_module(ptr::null(), 0, b"\0".as_ptr());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOEXEC);
    }

    #[test]
    fn test_init_module_phase128_len_one_with_null_image_is_enoexec_not_efault() {
        // Same precedence test at len = 1 (still below 64-byte ELF
        // header bound) — length check wins over NULL pointer.
        let r = init_module(ptr::null(), 1, b"\0".as_ptr());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOEXEC);
    }

    #[test]
    fn test_init_module_phase128_len_below_hdr_with_null_params_is_enoexec() {
        // Length check also wins over the NULL-params EFAULT.
        let buf = [0u8; 64];
        let r = init_module(buf.as_ptr(), 32, ptr::null());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOEXEC);
    }

    #[test]
    fn test_init_module_phase128_huge_len_e2big_wins_over_null_image() {
        // The MODULE_IMAGE_MAX cap is policy, not a Linux-derived
        // bound, but the precedence rule (size check before NULL
        // pointer check) is the same shape as Linux's behaviour.
        let r = init_module(ptr::null(), usize::MAX, b"\0".as_ptr());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::E2BIG);
    }

    #[test]
    fn test_init_module_phase128_null_image_with_valid_len_efault() {
        // Now that length passes, NULL image is observed as EFAULT,
        // matching Linux's `copy_chunked_from_user` failure mode.
        let r = init_module(ptr::null(), 1024, b"\0".as_ptr());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_init_module_phase128_null_params_with_valid_len_efault() {
        // After length and image checks pass, NULL params triggers
        // Linux's `strndup_user(uargs, ...)` EFAULT path.
        let buf = [0u8; 128];
        let r = init_module(buf.as_ptr(), 128, ptr::null());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_init_module_phase128_libkmod_truncated_image_workflow() {
        // libkmod (used by modprobe / systemd-modules-load) reads a
        // .ko file into a buffer and calls init_module(buf, n, ""). If
        // the file is truncated to fewer than 64 bytes (e.g. a corrupt
        // download), Linux returns ENOEXEC so the tool can log
        // "module file is truncated" rather than "memory fault".
        let buf = [0xCAu8; 16]; // garbage, definitely not an ELF
        let params = b"\0";
        let r = init_module(buf.as_ptr(), 16, params.as_ptr());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOEXEC);
    }

    #[test]
    fn test_init_module_phase128_recovery_after_enoexec() {
        // After a length-driven ENOEXEC, a subsequent well-shaped
        // call still produces a clean ENOSYS (errno is rewritten,
        // not sticky).
        let buf = [0u8; 128];
        let params = b"\0";

        let r1 = init_module(buf.as_ptr(), 4, params.as_ptr());
        assert_eq!(r1, -1);
        assert_eq!(errno::get_errno(), errno::ENOEXEC);

        let r2 = init_module(buf.as_ptr(), 128, params.as_ptr());
        assert_eq!(r2, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_init_module_phase128_buggy_caller_passes_garbage_len_field() {
        // A C caller doing `init_module(buf, sizeof_struct, "")` with
        // a stale `sizeof_struct == 0` from a partially-initialized
        // struct hits the length check first and gets ENOEXEC, not a
        // segfault from dereferencing the buffer at length 0.
        let buf = [0u8; 256];
        let params = b"\0";
        let r = init_module(buf.as_ptr(), 0, params.as_ptr());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOEXEC);
    }

    #[test]
    fn test_init_module_phase128_huge_len_does_not_overflow() {
        // The MODULE_IMAGE_MAX check uses `>` not `>=`, but at
        // usize::MAX there's no arithmetic to overflow. Confirm we
        // hit E2BIG cleanly.
        let buf = [0u8; 128];
        let params = b"\0";
        let r = init_module(buf.as_ptr(), usize::MAX, params.as_ptr());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::E2BIG);
    }

    #[test]
    fn test_init_module_phase128_just_above_cap_e2big() {
        // MODULE_IMAGE_MAX + 1 must hit E2BIG, not pass through to the
        // NULL-image check.
        let r = init_module(ptr::null(), MODULE_IMAGE_MAX + 1, b"\0".as_ptr());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::E2BIG);
    }

    #[test]
    fn test_init_module_phase128_doc_comment_no_longer_lies_about_flags() {
        // Sentinel test: ensures the function compiles and runs after
        // the doc-comment fix that removed the spurious "unknown flag
        // bit" line. init_module takes (umod, len, uargs) — no flags.
        // This test simply pins the success path so any future regr-
        // ession that reintroduces a flag arg would be caught.
        let buf = [0u8; 128];
        let r = init_module(buf.as_ptr(), 128, b"\0".as_ptr());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // ======================================================================
    // Phase 174 — module syscalls CAP_SYS_MODULE gate
    //
    // Linux performs the CAP_SYS_MODULE check at the very top of all
    // three module syscalls (`may_init_module` for init_module /
    // finit_module, an explicit `capable()` for delete_module).  EPERM
    // therefore beats every other errno path — even bad-fd, NULL
    // pointers, and unknown flag bits.
    //
    // These tests use the established CapGuard snapshot/restore pattern
    // from Phases 168 – 173 and must run with `--test-threads=1`.
    // ======================================================================

    mod module_cap_phase174 {
        use super::*;

        struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            fn snapshot() -> Self {
                let (lo, hi) =
                    crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version:
                        crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
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
                let _ =
                    crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        fn drop_cap_sys_module() {
            use crate::sys_capability::CAP_SYS_MODULE;
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if CAP_SYS_MODULE < 32 {
                (lo & !(1u32 << CAP_SYS_MODULE), hi)
            } else {
                (lo, hi & !(1u32 << (CAP_SYS_MODULE - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
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
            let rc =
                crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0,
                "capset must succeed when dropping CAP_SYS_MODULE");
            assert!(!crate::sys_capability::has_capability(CAP_SYS_MODULE));
        }

        // -- init_module --------------------------------------------------

        /// init_module with valid args but no cap → EPERM.
        #[test]
        fn test_init_module_phase174_valid_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_module();
            let buf = [0u8; 128];
            errno::set_errno(0);
            assert_eq!(init_module(buf.as_ptr(), 128, b"\0".as_ptr()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// init_module(NULL, 0, NULL) without cap → EPERM (NOT ENOEXEC).
        /// On Linux may_init_module runs before copy_module_from_user,
        /// so even a totally bogus call observes EPERM first.
        #[test]
        fn test_init_module_phase174_eperm_beats_enoexec() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_module();
            errno::set_errno(0);
            assert_eq!(
                init_module(ptr::null(), 0, ptr::null()),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// init_module with len > MODULE_IMAGE_MAX without cap → EPERM
        /// (NOT E2BIG).
        #[test]
        fn test_init_module_phase174_eperm_beats_e2big() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_module();
            errno::set_errno(0);
            assert_eq!(
                init_module(ptr::null(), MODULE_IMAGE_MAX + 1, ptr::null()),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// init_module(NULL, valid_len, valid_params) without cap →
        /// EPERM (NOT EFAULT).
        #[test]
        fn test_init_module_phase174_eperm_beats_efault() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_module();
            errno::set_errno(0);
            assert_eq!(
                init_module(ptr::null(), 128, b"\0".as_ptr()),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- finit_module -----------------------------------------------

        /// finit_module with valid args but no cap → EPERM.
        #[test]
        fn test_finit_module_phase174_valid_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_module();
            errno::set_errno(0);
            assert_eq!(finit_module(3, b"\0".as_ptr(), 0), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// finit_module with unknown flag bit → EPERM (NOT EINVAL).
        /// may_init_module runs before the flag check.
        #[test]
        fn test_finit_module_phase174_eperm_beats_einval() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_module();
            errno::set_errno(0);
            assert_eq!(finit_module(3, b"\0".as_ptr(), 0xFFFF_0000), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// finit_module with negative fd → EPERM (NOT EBADF).
        #[test]
        fn test_finit_module_phase174_eperm_beats_ebadf() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_module();
            errno::set_errno(0);
            assert_eq!(finit_module(-1, b"\0".as_ptr(), 0), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// finit_module with NULL params → EPERM (NOT EFAULT).
        #[test]
        fn test_finit_module_phase174_eperm_beats_efault() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_module();
            errno::set_errno(0);
            assert_eq!(finit_module(3, ptr::null(), 0), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- delete_module ----------------------------------------------

        /// delete_module with valid name but no cap → EPERM.
        #[test]
        fn test_delete_module_phase174_valid_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_module();
            errno::set_errno(0);
            assert_eq!(delete_module(b"foo\0".as_ptr(), 0), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// delete_module with NULL name → EPERM (NOT EFAULT).
        #[test]
        fn test_delete_module_phase174_eperm_beats_efault() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_module();
            errno::set_errno(0);
            assert_eq!(delete_module(ptr::null(), 0), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// delete_module with bad flags → EPERM (NOT EINVAL).
        #[test]
        fn test_delete_module_phase174_eperm_beats_einval_flags() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_module();
            errno::set_errno(0);
            assert_eq!(
                delete_module(b"foo\0".as_ptr(), 0xFFFF_0000),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// delete_module with empty name → EPERM (NOT EINVAL).
        #[test]
        fn test_delete_module_phase174_eperm_beats_einval_empty_name() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_module();
            errno::set_errno(0);
            assert_eq!(delete_module(b"\0".as_ptr(), 0), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Workflow / recovery -----------------------------------------

        /// rmmod-like probe: drop cap → delete_module EPERM → restore
        /// cap → delete_module reaches ENOSYS.
        #[test]
        fn test_delete_module_phase174_workflow_drop_restore() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_module();
            errno::set_errno(0);
            assert_eq!(delete_module(b"snd_hda\0".as_ptr(), 0), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);

            // Restore CAP_SYS_MODULE and re-try.
            use crate::sys_capability::CAP_SYS_MODULE;
            let (lo, hi) =
                crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if CAP_SYS_MODULE < 32 {
                (lo | (1u32 << CAP_SYS_MODULE), hi)
            } else {
                (lo, hi | (1u32 << (CAP_SYS_MODULE - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
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
            assert_eq!(
                crate::sys_capability::capset(&mut hdr, data.as_ptr()),
                0,
            );
            errno::set_errno(0);
            assert_eq!(delete_module(b"snd_hda\0".as_ptr(), 0), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        // -- Sentinel: cap-held privileged path still works -------------

        /// With CAP_SYS_MODULE held (default), every module syscall
        /// reaches ENOSYS — verifies the gate doesn't fire spuriously.
        #[test]
        fn test_module_phase174_sentinel_cap_held_reaches_enosys() {
            let _g = CapGuard::snapshot();
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_MODULE,
            ));
            let buf = [0u8; 128];
            errno::set_errno(0);
            assert_eq!(init_module(buf.as_ptr(), 128, b"\0".as_ptr()), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
            errno::set_errno(0);
            assert_eq!(finit_module(3, b"\0".as_ptr(), 0), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
            errno::set_errno(0);
            assert_eq!(delete_module(b"foo\0".as_ptr(), 0), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        // -- Cross-check: dropping CAP_SYS_MODULE isolates other caps ---

        /// Dropping CAP_SYS_MODULE must not disturb other caps.
        #[test]
        fn test_module_phase174_drop_isolates_other_caps() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_module();
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_ADMIN,
            ));
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_NICE,
            ));
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_TIME,
            ));
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYSLOG,
            ));
            assert!(!crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_MODULE,
            ));
        }
    }
}
