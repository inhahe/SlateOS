//! `<linux/landlock.h>` — unprivileged access-control (sandboxing).
//!
//! Landlock is a stackable Linux security module that allows
//! unprivileged processes to restrict their own access rights
//! (filesystem, network) without needing root or a security policy.
//! Available since Linux 5.13.
//!
//! ## Backend status
//!
//! Our kernel does not yet enforce Landlock-style sandboxing — that
//! requires hooks in the VFS and the network stack which haven't
//! been wired up.  Until those land, the syscalls validate their
//! inputs (matching Linux's error contract) and report "no real
//! ruleset machinery" after validation:
//!
//! * [`landlock_create_ruleset`] supports two **probe** forms
//!   (`attr == NULL && size == 0` plus one probe flag) exactly the
//!   way Linux does:
//!     * `flags == LANDLOCK_CREATE_RULESET_VERSION` — returns the
//!       advertised ABI version ([`LANDLOCK_ABI_VERSION`]).  This is
//!       the very first call every Landlock-aware program makes;
//!       without it, the program assumes Landlock is unavailable and
//!       the rest never runs.
//!     * `flags == LANDLOCK_CREATE_RULESET_ERRATA` — returns the
//!       advertised errata bitfield ([`LANDLOCK_ERRATA`]).  Added in
//!       Linux 6.10 so userspace can detect specific kernel quirks
//!       (we have none, so the bitfield is zero — but the probe must
//!       still succeed, otherwise libcap-landlock 0.6+ thinks the
//!       kernel is older than 6.10 and falls back to slower paths).
//! * The non-probe (real-create) form validates `attr`, `size`,
//!   `flags`, and the access-rights bitmask, then returns -1 with
//!   `ENOSYS` — programs that gracefully fall back when create
//!   fails keep working.
//! * [`landlock_add_rule`] and [`landlock_restrict_self`] validate
//!   their inputs (rule type, attribute pointer, flags) and report
//!   `EBADFD` for any `ruleset_fd` — since we never successfully
//!   created one, every fd is by definition not a Landlock ruleset.
//!
//! When the VFS/network hooks land, replace the post-validation
//! arms; the validation surface itself is the final contract.

use crate::errno;

// ---------------------------------------------------------------------------
// Landlock rule types
// ---------------------------------------------------------------------------

/// Path-beneath rule: restrict access under a directory hierarchy.
pub const LANDLOCK_RULE_PATH_BENEATH: u32 = 1;
/// Network port rule: restrict binding/connecting to ports.
pub const LANDLOCK_RULE_NET_PORT: u32 = 2;

// ---------------------------------------------------------------------------
// Filesystem access rights (for LANDLOCK_RULE_PATH_BENEATH)
// ---------------------------------------------------------------------------

/// Execute a file.
pub const LANDLOCK_ACCESS_FS_EXECUTE: u64 = 1 << 0;
/// Open a file with write access.
pub const LANDLOCK_ACCESS_FS_WRITE_FILE: u64 = 1 << 1;
/// Open a file with read access.
pub const LANDLOCK_ACCESS_FS_READ_FILE: u64 = 1 << 2;
/// Open a directory or list its content.
pub const LANDLOCK_ACCESS_FS_READ_DIR: u64 = 1 << 3;
/// Remove an empty directory or rename one.
pub const LANDLOCK_ACCESS_FS_REMOVE_DIR: u64 = 1 << 4;
/// Unlink (remove) a file.
pub const LANDLOCK_ACCESS_FS_REMOVE_FILE: u64 = 1 << 5;
/// Create a character device.
pub const LANDLOCK_ACCESS_FS_MAKE_CHAR: u64 = 1 << 6;
/// Create a directory.
pub const LANDLOCK_ACCESS_FS_MAKE_DIR: u64 = 1 << 7;
/// Create a regular file.
pub const LANDLOCK_ACCESS_FS_MAKE_REG: u64 = 1 << 8;
/// Create a socket.
pub const LANDLOCK_ACCESS_FS_MAKE_SOCK: u64 = 1 << 9;
/// Create a named pipe (FIFO).
pub const LANDLOCK_ACCESS_FS_MAKE_FIFO: u64 = 1 << 10;
/// Create a block device.
pub const LANDLOCK_ACCESS_FS_MAKE_BLOCK: u64 = 1 << 11;
/// Create a symbolic link.
pub const LANDLOCK_ACCESS_FS_MAKE_SYM: u64 = 1 << 12;
/// Link or rename a file to a directory.
pub const LANDLOCK_ACCESS_FS_REFER: u64 = 1 << 13;
/// Truncate a file.
pub const LANDLOCK_ACCESS_FS_TRUNCATE: u64 = 1 << 14;
/// Issue an IOCTL on a device file.
pub const LANDLOCK_ACCESS_FS_IOCTL_DEV: u64 = 1 << 15;

/// Bitmask of every defined filesystem access right.
pub const LANDLOCK_ACCESS_FS_ALL: u64 = LANDLOCK_ACCESS_FS_EXECUTE
    | LANDLOCK_ACCESS_FS_WRITE_FILE
    | LANDLOCK_ACCESS_FS_READ_FILE
    | LANDLOCK_ACCESS_FS_READ_DIR
    | LANDLOCK_ACCESS_FS_REMOVE_DIR
    | LANDLOCK_ACCESS_FS_REMOVE_FILE
    | LANDLOCK_ACCESS_FS_MAKE_CHAR
    | LANDLOCK_ACCESS_FS_MAKE_DIR
    | LANDLOCK_ACCESS_FS_MAKE_REG
    | LANDLOCK_ACCESS_FS_MAKE_SOCK
    | LANDLOCK_ACCESS_FS_MAKE_FIFO
    | LANDLOCK_ACCESS_FS_MAKE_BLOCK
    | LANDLOCK_ACCESS_FS_MAKE_SYM
    | LANDLOCK_ACCESS_FS_REFER
    | LANDLOCK_ACCESS_FS_TRUNCATE
    | LANDLOCK_ACCESS_FS_IOCTL_DEV;

// ---------------------------------------------------------------------------
// Network access rights (for LANDLOCK_RULE_NET_PORT)
// ---------------------------------------------------------------------------

/// Bind to a TCP port.
pub const LANDLOCK_ACCESS_NET_BIND_TCP: u64 = 1 << 0;
/// Connect to a TCP port.
pub const LANDLOCK_ACCESS_NET_CONNECT_TCP: u64 = 1 << 1;

/// Bitmask of every defined network access right.
pub const LANDLOCK_ACCESS_NET_ALL: u64 =
    LANDLOCK_ACCESS_NET_BIND_TCP | LANDLOCK_ACCESS_NET_CONNECT_TCP;

// ---------------------------------------------------------------------------
// Landlock create flags
// ---------------------------------------------------------------------------

/// Request the latest supported ABI version (used only in the
/// probe form of [`landlock_create_ruleset`]).
pub const LANDLOCK_CREATE_RULESET_VERSION: u32 = 1 << 0;

/// Request the errata bitfield (used only in the probe form of
/// [`landlock_create_ruleset`]).  Added in Linux 6.10.
///
/// Each set bit identifies a known kernel quirk userspace needs to
/// work around.  Linux's first three errata cover ABI versions 1–3;
/// every fresh release of `linux/landlock.h` documents the meaning
/// of each bit.  Our [`LANDLOCK_ERRATA`] is zero — fresh
/// implementation, no quirks to advertise.
pub const LANDLOCK_CREATE_RULESET_ERRATA: u32 = 1 << 1;

/// The Landlock ABI version we advertise.
///
/// Version 1 is the minimum specification (Linux 5.13).  We can
/// bump this as the kernel learns to honour the rule types and
/// access bits introduced by later versions.
pub const LANDLOCK_ABI_VERSION: i32 = 1;

/// The Landlock errata bitfield we advertise.
///
/// Each bit corresponds to a known kernel quirk userspace must work
/// around — but we have none of those quirks (our implementation
/// isn't old enough to have collected workarounds yet), so the
/// bitfield is zero.  Probing this still has to succeed, otherwise
/// callers assume the running kernel is too old to expose errata at
/// all and apply *Linux's* historical workarounds.
pub const LANDLOCK_ERRATA: i32 = 0;

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

/// Ruleset attributes for `landlock_create_ruleset()`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LandlockRulesetAttr {
    /// Filesystem access rights to handle.
    pub handled_access_fs: u64,
    /// Network access rights to handle.
    pub handled_access_net: u64,
}

impl LandlockRulesetAttr {
    /// Create a zeroed ruleset attribute.
    #[must_use]
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

/// Path-beneath rule attribute.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LandlockPathBeneathAttr {
    /// Allowed access rights.
    pub allowed_access: u64,
    /// File descriptor of the directory.
    pub parent_fd: i32,
    /// Padding.
    _pad: i32,
}

impl LandlockPathBeneathAttr {
    /// Create a zeroed path-beneath attribute.
    #[must_use]
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

/// Network port rule attribute.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LandlockNetPortAttr {
    /// Allowed access rights.
    pub allowed_access: u64,
    /// Port number.
    pub port: u64,
}

impl LandlockNetPortAttr {
    /// Create a zeroed network port attribute.
    #[must_use]
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

const MIN_RULESET_ATTR_SIZE: usize = core::mem::size_of::<LandlockRulesetAttr>();
/// Linux allows attr to grow as new fields are added.  This cap is
/// generous (matches Linux's "if the kernel doesn't recognise extra
/// bytes, they must all be zero") but rejects clearly bogus sizes.
const MAX_RULESET_ATTR_SIZE: usize = 4096;

const fn is_valid_fs_access(mask: u64) -> bool {
    // Every set bit must be a recognised LANDLOCK_ACCESS_FS_* bit.
    mask & !LANDLOCK_ACCESS_FS_ALL == 0
}

const fn is_valid_net_access(mask: u64) -> bool {
    mask & !LANDLOCK_ACCESS_NET_ALL == 0
}

// ---------------------------------------------------------------------------
// landlock_create_ruleset
// ---------------------------------------------------------------------------

/// Create a Landlock ruleset.
///
/// Three forms (matching Linux 6.10+):
///
/// 1. **Version probe**: `attr == NULL && size == 0 && flags ==
///    LANDLOCK_CREATE_RULESET_VERSION` — returns the advertised ABI
///    version ([`LANDLOCK_ABI_VERSION`]).  Every Landlock-aware
///    program does this first.
/// 2. **Errata probe**: `attr == NULL && size == 0 && flags ==
///    LANDLOCK_CREATE_RULESET_ERRATA` — returns the errata bitfield
///    ([`LANDLOCK_ERRATA`]).  Programs that need to work around
///    specific kernel quirks do this after the version probe.
/// 3. **Create**: `flags == 0`, `attr` and `size` describe a valid
///    [`LandlockRulesetAttr`] — would return an fd; in our world
///    validates and reports `ENOSYS` because the kernel-side
///    enforcement isn't wired up.
///
/// Linux's `flags != 0` path requires `attr == NULL && size == 0`
/// (probe mode is *only* about reading metadata) and rejects every
/// other flag combination — including `VERSION | ERRATA` together —
/// with `EINVAL`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn landlock_create_ruleset(
    attr: *const LandlockRulesetAttr,
    size: usize,
    flags: u32,
) -> i32 {
    // -- Probe forms --------------------------------------------------------
    //
    // Linux's order: any non-zero `flags` means we're in probe mode.
    // The only legal probes are exact-match VERSION or exact-match
    // ERRATA, both with attr=NULL and size=0; everything else is
    // EINVAL.  (Mixing probe bits, mixing a probe with attr/size, or
    // setting an unknown bit all fall through to EINVAL.)
    if flags != 0 {
        if attr.is_null() && size == 0 {
            if flags == LANDLOCK_CREATE_RULESET_VERSION {
                return LANDLOCK_ABI_VERSION;
            }
            if flags == LANDLOCK_CREATE_RULESET_ERRATA {
                return LANDLOCK_ERRATA;
            }
        }
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Linux's order (matching `copy_min_struct_from_user` then the
    // explicit page-size check): `size < min` is EINVAL, oversized is
    // E2BIG, then dereferencing a bad pointer is EFAULT.  Critically
    // `size == 0` is EINVAL *before* the NULL-attr EFAULT — a buggy
    // caller that passes `(NULL, 0, 0)` should be steered toward
    // "your size is wrong" rather than "your pointer is wrong",
    // because the right fix is to fill in the size.
    if size < MIN_RULESET_ATTR_SIZE {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if size > MAX_RULESET_ATTR_SIZE {
        errno::set_errno(errno::E2BIG);
        return -1;
    }
    if attr.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // SAFETY: `attr` is non-NULL and `size >= sizeof(LandlockRulesetAttr)`.
    // We only read fields covered by the minimum size.
    let a = unsafe { core::ptr::read_unaligned(attr) };

    if !is_valid_fs_access(a.handled_access_fs) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if !is_valid_net_access(a.handled_access_net) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if a.handled_access_fs == 0 && a.handled_access_net == 0 {
        // Linux specifically returns ENOMSG when *no* access bits are set
        // — a ruleset that controls nothing is useless and almost
        // certainly a caller bug.
        errno::set_errno(errno::ENOMSG);
        return -1;
    }

    // Inputs look valid, but we have no enforcement back-end.
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// landlock_add_rule
// ---------------------------------------------------------------------------

/// Add a rule to a Landlock ruleset.
///
/// We never create a real ruleset, so any positive `ruleset_fd`
/// fails with `EBADFD` (matches Linux's "fd does not refer to a
/// Landlock ruleset").  Validation of the other arguments runs
/// first so the caller still sees a meaningful EINVAL/EFAULT for
/// obviously broken calls.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn landlock_add_rule(
    ruleset_fd: i32,
    rule_type: u32,
    rule_attr: *const u8,
    flags: u32,
) -> i32 {
    // Flags must be zero (no `LANDLOCK_ADD_RULE_*` flags defined yet).
    if flags != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Unknown rule types are rejected before touching `rule_attr`.
    match rule_type {
        LANDLOCK_RULE_PATH_BENEATH | LANDLOCK_RULE_NET_PORT => {}
        _ => {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
    }

    if rule_attr.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // For each rule type, validate the access-rights mask.
    match rule_type {
        LANDLOCK_RULE_PATH_BENEATH => {
            // SAFETY: caller passes a `LandlockPathBeneathAttr` here.
            // We use `read_unaligned` so the caller may use any
            // alignment.
            let attr =
                unsafe { core::ptr::read_unaligned(rule_attr.cast::<LandlockPathBeneathAttr>()) };
            if !is_valid_fs_access(attr.allowed_access) {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            if attr.allowed_access == 0 {
                // Linux: "no access right requested" → ENOMSG.
                errno::set_errno(errno::ENOMSG);
                return -1;
            }
            if attr.parent_fd < 0 {
                errno::set_errno(errno::EBADF);
                return -1;
            }
        }
        LANDLOCK_RULE_NET_PORT => {
            // SAFETY: caller passes a `LandlockNetPortAttr` here.
            let attr =
                unsafe { core::ptr::read_unaligned(rule_attr.cast::<LandlockNetPortAttr>()) };
            if !is_valid_net_access(attr.allowed_access) {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            if attr.allowed_access == 0 {
                errno::set_errno(errno::ENOMSG);
                return -1;
            }
            if attr.port > u64::from(u16::MAX) {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
        }
        _ => unreachable!(),
    }

    // After all input validation: the fd cannot possibly refer to one
    // of our (non-existent) rulesets.  Linux uses EBADFD specifically
    // for "fd is open but not a Landlock ruleset" — distinct from
    // EBADF for "fd not open at all".
    if ruleset_fd < 0 {
        errno::set_errno(errno::EBADF);
    } else {
        errno::set_errno(errno::EBADFD);
    }
    -1
}

// ---------------------------------------------------------------------------
// landlock_restrict_self
// ---------------------------------------------------------------------------

/// Restrict the calling thread with a Landlock ruleset.
///
/// As with [`landlock_add_rule`], we never have a real ruleset fd,
/// so the syscall always fails after the privilege and flag-shape
/// checks succeed.
///
/// Errors (Linux-matching priority order — see
/// `security/landlock/syscalls.c::SYSCALL_DEFINE2(landlock_restrict_self)`):
///
/// 1. **Phase 185:** `EPERM` — caller has neither `no_new_privs`
///    set nor `CAP_SYS_ADMIN`.  Landlock is a stackable-sandbox
///    primitive: it tightens the effective security policy, never
///    loosens it.  Linux therefore requires either that the task
///    has already opted out of privilege-raising (`no_new_privs`)
///    or that it is administratively trusted (`CAP_SYS_ADMIN`),
///    matching the same gate seccomp enforces.  The check uses
///    `ns_capable_noaudit(current_user_ns(), CAP_SYS_ADMIN)` in the
///    kernel; we map to `has_capability(CAP_SYS_ADMIN)` per our
///    single-userns model.  The gate runs **before** the
///    `flags != 0` EINVAL check, so an unprivileged caller passing
///    garbage flags sees EPERM, not EINVAL.
/// 2. `EINVAL` — any bit set in `flags` (no flags are defined yet
///    by upstream Landlock).
/// 3. `EBADF` — `ruleset_fd` is negative.
/// 4. `EBADFD` — `ruleset_fd` is open but not a Landlock ruleset
///    (the only path we can reach in this stub — we have no
///    real Landlock backend).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn landlock_restrict_self(ruleset_fd: i32, flags: u32) -> i32 {
    // Phase 185: Linux's landlock_restrict_self gate.  An
    // unprivileged caller can only install a Landlock ruleset on
    // itself if it has already opted out of privilege-raising via
    // `prctl(PR_SET_NO_NEW_PRIVS, 1, ...)`.  An administrator
    // (CAP_SYS_ADMIN) is exempt because they could already restrict
    // the process by other means.
    //
    // Placement note: this check fires *before* the EINVAL flag-mask
    // check, matching the kernel's source order — so EPERM takes
    // precedence over EINVAL for unprivileged callers.
    if !crate::unistd::no_new_privs_set()
        && !crate::sys_capability::has_capability(crate::sys_capability::CAP_SYS_ADMIN)
    {
        errno::set_errno(errno::EPERM);
        return -1;
    }
    if flags != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if ruleset_fd < 0 {
        errno::set_errno(errno::EBADF);
    } else {
        errno::set_errno(errno::EBADFD);
    }
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn clear_errno() {
        errno::set_errno(0);
    }

    #[test]
    fn test_fs_access_rights_are_powers_of_two() {
        let rights = [
            LANDLOCK_ACCESS_FS_EXECUTE,
            LANDLOCK_ACCESS_FS_WRITE_FILE,
            LANDLOCK_ACCESS_FS_READ_FILE,
            LANDLOCK_ACCESS_FS_READ_DIR,
            LANDLOCK_ACCESS_FS_REMOVE_DIR,
            LANDLOCK_ACCESS_FS_REMOVE_FILE,
            LANDLOCK_ACCESS_FS_MAKE_CHAR,
            LANDLOCK_ACCESS_FS_MAKE_DIR,
            LANDLOCK_ACCESS_FS_MAKE_REG,
            LANDLOCK_ACCESS_FS_MAKE_SOCK,
            LANDLOCK_ACCESS_FS_MAKE_FIFO,
            LANDLOCK_ACCESS_FS_MAKE_BLOCK,
            LANDLOCK_ACCESS_FS_MAKE_SYM,
            LANDLOCK_ACCESS_FS_REFER,
            LANDLOCK_ACCESS_FS_TRUNCATE,
            LANDLOCK_ACCESS_FS_IOCTL_DEV,
        ];
        for r in &rights {
            assert!(r.is_power_of_two(), "right {r:#x} not power of 2");
        }
    }

    #[test]
    fn test_net_access_rights() {
        assert_eq!(LANDLOCK_ACCESS_NET_BIND_TCP, 1);
        assert_eq!(LANDLOCK_ACCESS_NET_CONNECT_TCP, 2);
        assert_ne!(
            LANDLOCK_ACCESS_NET_BIND_TCP,
            LANDLOCK_ACCESS_NET_CONNECT_TCP
        );
    }

    #[test]
    fn test_rule_types() {
        assert_eq!(LANDLOCK_RULE_PATH_BENEATH, 1);
        assert_eq!(LANDLOCK_RULE_NET_PORT, 2);
    }

    #[test]
    fn test_ruleset_attr_size() {
        assert_eq!(core::mem::size_of::<LandlockRulesetAttr>(), 16);
    }

    #[test]
    fn test_path_beneath_attr_size() {
        assert_eq!(core::mem::size_of::<LandlockPathBeneathAttr>(), 16);
    }

    #[test]
    fn test_net_port_attr_size() {
        assert_eq!(core::mem::size_of::<LandlockNetPortAttr>(), 16);
    }

    #[test]
    fn test_fs_all_includes_every_bit() {
        // Every defined FS access bit is in LANDLOCK_ACCESS_FS_ALL.
        assert!(LANDLOCK_ACCESS_FS_ALL & LANDLOCK_ACCESS_FS_TRUNCATE != 0);
        assert!(LANDLOCK_ACCESS_FS_ALL & LANDLOCK_ACCESS_FS_IOCTL_DEV != 0);
    }

    // -- ABI probe form ------------------------------------------------------

    #[test]
    fn test_create_ruleset_probe_returns_abi_version() {
        clear_errno();
        let v = landlock_create_ruleset(core::ptr::null(), 0, LANDLOCK_CREATE_RULESET_VERSION);
        assert_eq!(v, LANDLOCK_ABI_VERSION);
        // Probe success doesn't touch errno.
        assert_eq!(errno::get_errno(), 0);
    }

    #[test]
    fn test_create_ruleset_probe_bad_flags_einval() {
        // attr=NULL, size=0, but flags != LANDLOCK_CREATE_RULESET_VERSION.
        clear_errno();
        let v = landlock_create_ruleset(core::ptr::null(), 0, 0);
        assert_eq!(v, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_create_ruleset_probe_extra_bits_einval() {
        // Probe form must use *only* LANDLOCK_CREATE_RULESET_VERSION.
        clear_errno();
        let v =
            landlock_create_ruleset(core::ptr::null(), 0, LANDLOCK_CREATE_RULESET_VERSION | 0x10);
        assert_eq!(v, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -- Real-create form ---------------------------------------------------

    #[test]
    fn test_create_ruleset_flags_nonzero_einval() {
        let attr = LandlockRulesetAttr {
            handled_access_fs: LANDLOCK_ACCESS_FS_READ_FILE,
            handled_access_net: 0,
        };
        clear_errno();
        let v = landlock_create_ruleset(
            &raw const attr,
            core::mem::size_of::<LandlockRulesetAttr>(),
            LANDLOCK_CREATE_RULESET_VERSION,
        );
        assert_eq!(v, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_create_ruleset_null_attr_with_size_efault() {
        clear_errno();
        let v = landlock_create_ruleset(
            core::ptr::null(),
            core::mem::size_of::<LandlockRulesetAttr>(),
            0,
        );
        assert_eq!(v, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_create_ruleset_undersized_einval() {
        let attr = LandlockRulesetAttr::zeroed();
        clear_errno();
        let v = landlock_create_ruleset(&raw const attr, 4, 0);
        assert_eq!(v, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_create_ruleset_oversized_e2big() {
        let attr = LandlockRulesetAttr {
            handled_access_fs: LANDLOCK_ACCESS_FS_READ_FILE,
            handled_access_net: 0,
        };
        clear_errno();
        let v = landlock_create_ruleset(&raw const attr, 1_000_000, 0);
        assert_eq!(v, -1);
        assert_eq!(errno::get_errno(), errno::E2BIG);
    }

    #[test]
    fn test_create_ruleset_unknown_fs_bit_einval() {
        let attr = LandlockRulesetAttr {
            handled_access_fs: 1u64 << 63, // not a defined bit
            handled_access_net: 0,
        };
        clear_errno();
        let v = landlock_create_ruleset(
            &raw const attr,
            core::mem::size_of::<LandlockRulesetAttr>(),
            0,
        );
        assert_eq!(v, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_create_ruleset_unknown_net_bit_einval() {
        let attr = LandlockRulesetAttr {
            handled_access_fs: 0,
            handled_access_net: 1u64 << 5, // not a defined bit
        };
        clear_errno();
        let v = landlock_create_ruleset(
            &raw const attr,
            core::mem::size_of::<LandlockRulesetAttr>(),
            0,
        );
        assert_eq!(v, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_create_ruleset_no_access_bits_enomsg() {
        let attr = LandlockRulesetAttr::zeroed();
        clear_errno();
        let v = landlock_create_ruleset(
            &raw const attr,
            core::mem::size_of::<LandlockRulesetAttr>(),
            0,
        );
        assert_eq!(v, -1);
        assert_eq!(errno::get_errno(), errno::ENOMSG);
    }

    #[test]
    fn test_create_ruleset_valid_inputs_enosys() {
        let attr = LandlockRulesetAttr {
            handled_access_fs: LANDLOCK_ACCESS_FS_READ_FILE | LANDLOCK_ACCESS_FS_WRITE_FILE,
            handled_access_net: LANDLOCK_ACCESS_NET_BIND_TCP,
        };
        clear_errno();
        let v = landlock_create_ruleset(
            &raw const attr,
            core::mem::size_of::<LandlockRulesetAttr>(),
            0,
        );
        assert_eq!(v, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- landlock_add_rule --------------------------------------------------

    #[test]
    fn test_add_rule_unknown_type_einval() {
        clear_errno();
        let dummy = LandlockPathBeneathAttr::zeroed();
        let ret = landlock_add_rule(3, 999, (&raw const dummy).cast::<u8>(), 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_add_rule_flags_nonzero_einval() {
        clear_errno();
        let dummy = LandlockPathBeneathAttr::zeroed();
        let ret = landlock_add_rule(
            3,
            LANDLOCK_RULE_PATH_BENEATH,
            (&raw const dummy).cast::<u8>(),
            1,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_add_rule_null_attr_efault() {
        clear_errno();
        let ret = landlock_add_rule(3, LANDLOCK_RULE_PATH_BENEATH, core::ptr::null(), 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_add_rule_path_beneath_unknown_access_einval() {
        clear_errno();
        let attr = LandlockPathBeneathAttr {
            allowed_access: 1u64 << 60,
            parent_fd: 5,
            _pad: 0,
        };
        let ret = landlock_add_rule(
            3,
            LANDLOCK_RULE_PATH_BENEATH,
            (&raw const attr).cast::<u8>(),
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_add_rule_path_beneath_zero_access_enomsg() {
        clear_errno();
        let attr = LandlockPathBeneathAttr {
            allowed_access: 0,
            parent_fd: 5,
            _pad: 0,
        };
        let ret = landlock_add_rule(
            3,
            LANDLOCK_RULE_PATH_BENEATH,
            (&raw const attr).cast::<u8>(),
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOMSG);
    }

    #[test]
    fn test_add_rule_path_beneath_negative_parent_fd_ebadf() {
        clear_errno();
        let attr = LandlockPathBeneathAttr {
            allowed_access: LANDLOCK_ACCESS_FS_READ_FILE,
            parent_fd: -1,
            _pad: 0,
        };
        let ret = landlock_add_rule(
            3,
            LANDLOCK_RULE_PATH_BENEATH,
            (&raw const attr).cast::<u8>(),
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_add_rule_net_port_high_port_einval() {
        clear_errno();
        let attr = LandlockNetPortAttr {
            allowed_access: LANDLOCK_ACCESS_NET_BIND_TCP,
            port: 1_000_000,
        };
        let ret = landlock_add_rule(3, LANDLOCK_RULE_NET_PORT, (&raw const attr).cast::<u8>(), 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_add_rule_net_port_zero_access_enomsg() {
        clear_errno();
        let attr = LandlockNetPortAttr {
            allowed_access: 0,
            port: 80,
        };
        let ret = landlock_add_rule(3, LANDLOCK_RULE_NET_PORT, (&raw const attr).cast::<u8>(), 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOMSG);
    }

    #[test]
    fn test_add_rule_valid_inputs_ebadfd_on_nonexistent_ruleset() {
        clear_errno();
        let attr = LandlockPathBeneathAttr {
            allowed_access: LANDLOCK_ACCESS_FS_READ_FILE,
            parent_fd: 1, // valid-looking fd
            _pad: 0,
        };
        let ret = landlock_add_rule(
            3,
            LANDLOCK_RULE_PATH_BENEATH,
            (&raw const attr).cast::<u8>(),
            0,
        );
        assert_eq!(ret, -1);
        // We never had a real ruleset → EBADFD, not EBADF (since the fd
        // value itself is non-negative).
        assert_eq!(errno::get_errno(), errno::EBADFD);
    }

    #[test]
    fn test_add_rule_negative_ruleset_ebadf() {
        clear_errno();
        let attr = LandlockPathBeneathAttr {
            allowed_access: LANDLOCK_ACCESS_FS_READ_FILE,
            parent_fd: 1,
            _pad: 0,
        };
        let ret = landlock_add_rule(
            -1,
            LANDLOCK_RULE_PATH_BENEATH,
            (&raw const attr).cast::<u8>(),
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // -- landlock_restrict_self ---------------------------------------------

    #[test]
    fn test_restrict_self_nonzero_flags_einval() {
        clear_errno();
        let ret = landlock_restrict_self(3, 1);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_restrict_self_negative_fd_ebadf() {
        clear_errno();
        let ret = landlock_restrict_self(-1, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_restrict_self_positive_fd_ebadfd() {
        clear_errno();
        let ret = landlock_restrict_self(3, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADFD);
    }

    // -- Workflow ------------------------------------------------------------

    #[test]
    fn test_typical_probe_then_create_then_fallback_workflow() {
        // 1. Probe.
        let ver = landlock_create_ruleset(core::ptr::null(), 0, LANDLOCK_CREATE_RULESET_VERSION);
        assert!(ver >= 1);

        // 2. Try to create — fails with ENOSYS because we have no backend.
        let attr = LandlockRulesetAttr {
            handled_access_fs: LANDLOCK_ACCESS_FS_READ_FILE,
            handled_access_net: 0,
        };
        let ret = landlock_create_ruleset(
            &raw const attr,
            core::mem::size_of::<LandlockRulesetAttr>(),
            0,
        );
        assert_eq!(ret, -1);
        // The caller will see ENOSYS and gracefully fall back to
        // "Landlock unavailable" without any further calls.
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // ===================================================================
    // Phase 133 — ERRATA probe support (Linux 6.10+) and tightened
    // probe-vs-create validation ordering.
    // ===================================================================

    // -- Constants ----------------------------------------------------------

    #[test]
    fn test_phase133_errata_probe_flag_value() {
        // Matches `LANDLOCK_CREATE_RULESET_ERRATA` in
        // <uapi/linux/landlock.h> as of 6.10.
        assert_eq!(LANDLOCK_CREATE_RULESET_ERRATA, 1 << 1);
    }

    #[test]
    fn test_phase133_errata_distinct_from_version() {
        // The two probe flags share no bits — Linux uses an exact-match
        // dispatch and requires `flags == VERSION` xor `flags == ERRATA`.
        assert_ne!(
            LANDLOCK_CREATE_RULESET_VERSION,
            LANDLOCK_CREATE_RULESET_ERRATA,
        );
        assert_eq!(
            LANDLOCK_CREATE_RULESET_VERSION & LANDLOCK_CREATE_RULESET_ERRATA,
            0,
        );
    }

    #[test]
    fn test_phase133_errata_value_is_zero() {
        // We advertise no errata.  Probing must still succeed and return
        // a *value* (0), not an error; otherwise libcap-landlock thinks
        // the kernel is too old to support the probe and applies its
        // own historical workarounds.
        assert_eq!(LANDLOCK_ERRATA, 0);
    }

    // -- ERRATA probe form --------------------------------------------------

    #[test]
    fn test_phase133_errata_probe_returns_errata_bitfield() {
        clear_errno();
        let v = landlock_create_ruleset(core::ptr::null(), 0, LANDLOCK_CREATE_RULESET_ERRATA);
        assert_eq!(v, LANDLOCK_ERRATA);
        // Probe success must NOT touch errno (even though we return 0,
        // 0 means "no errata", not an error).
        assert_eq!(errno::get_errno(), 0);
    }

    #[test]
    fn test_phase133_errata_probe_zero_is_success_not_failure() {
        // Critical: a return of 0 from the ERRATA probe is the
        // bitfield value, not a "ruleset fd 0".  We don't promise
        // anything about errno on success — clear it first and verify
        // the call doesn't disturb it.
        errno::set_errno(123);
        let v = landlock_create_ruleset(core::ptr::null(), 0, LANDLOCK_CREATE_RULESET_ERRATA);
        assert_eq!(v, 0);
        // Caller can still rely on errno being whatever they last set.
        assert_eq!(errno::get_errno(), 123);
    }

    #[test]
    fn test_phase133_errata_probe_with_non_null_attr_einval() {
        // Probe flags require attr=NULL.  A non-NULL attr with the
        // ERRATA flag is a buggy caller and gets EINVAL (matches
        // Linux's `if (attr != NULL || size != 0) return -EINVAL` in
        // the probe path).
        let attr = LandlockRulesetAttr::zeroed();
        clear_errno();
        let v = landlock_create_ruleset(&raw const attr, 0, LANDLOCK_CREATE_RULESET_ERRATA);
        assert_eq!(v, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_phase133_errata_probe_with_non_zero_size_einval() {
        clear_errno();
        let v = landlock_create_ruleset(core::ptr::null(), 16, LANDLOCK_CREATE_RULESET_ERRATA);
        assert_eq!(v, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -- Probe flag mixing --------------------------------------------------

    #[test]
    fn test_phase133_version_and_errata_together_einval() {
        // Linux uses exact-match (`flags == X`), so combining the two
        // probe bits doesn't trigger either — EINVAL.
        clear_errno();
        let v = landlock_create_ruleset(
            core::ptr::null(),
            0,
            LANDLOCK_CREATE_RULESET_VERSION | LANDLOCK_CREATE_RULESET_ERRATA,
        );
        assert_eq!(v, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_phase133_errata_with_extra_high_bit_einval() {
        // Unknown probe-flag bit set alongside ERRATA → EINVAL.
        clear_errno();
        let v = landlock_create_ruleset(
            core::ptr::null(),
            0,
            LANDLOCK_CREATE_RULESET_ERRATA | (1u32 << 20),
        );
        assert_eq!(v, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_phase133_unknown_probe_bit_alone_einval() {
        // A flag value that's neither VERSION nor ERRATA → EINVAL even
        // if attr=NULL && size=0.
        clear_errno();
        let v = landlock_create_ruleset(core::ptr::null(), 0, 1u32 << 5);
        assert_eq!(v, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -- Create-form ordering fix (size before EFAULT) ----------------------

    #[test]
    fn test_phase133_zero_size_with_null_attr_returns_einval_not_efault() {
        // BEFORE Phase 133: `attr=NULL, size=0, flags=0` returned
        // EFAULT (NULL attr was checked first).
        //
        // AFTER Phase 133: matches Linux's
        // `copy_min_struct_from_user` order — `size < min` is checked
        // first, so this is EINVAL.  The right caller fix is to set
        // size, not to allocate a struct.
        clear_errno();
        let v = landlock_create_ruleset(core::ptr::null(), 0, 0);
        assert_eq!(v, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_phase133_undersized_with_null_attr_returns_einval_not_efault() {
        // Same ordering rule: size=4 (< min=16) wins over NULL-attr.
        clear_errno();
        let v = landlock_create_ruleset(core::ptr::null(), 4, 0);
        assert_eq!(v, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_phase133_oversized_with_null_attr_returns_e2big_not_efault() {
        // E2BIG wins over EFAULT too — `size > MAX` is a check on a
        // value the caller controls, so the diagnostic should point
        // at it before the pointer dereference would fault.
        clear_errno();
        let v = landlock_create_ruleset(core::ptr::null(), 1_000_000, 0);
        assert_eq!(v, -1);
        assert_eq!(errno::get_errno(), errno::E2BIG);
    }

    #[test]
    fn test_phase133_null_attr_with_valid_size_still_efault() {
        // Sanity: when size IS valid, NULL attr still produces EFAULT.
        // The reorder didn't break the existing contract for the
        // typical buggy-caller case.
        clear_errno();
        let v = landlock_create_ruleset(
            core::ptr::null(),
            core::mem::size_of::<LandlockRulesetAttr>(),
            0,
        );
        assert_eq!(v, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // -- Probe vs create dispatch -------------------------------------------

    #[test]
    fn test_phase133_errata_probe_with_flags_set_but_in_create_size() {
        // `flags=ERRATA, attr=non-null, size=16` — non-zero flags
        // means probe path, but attr != NULL means probe rejects with
        // EINVAL.  Should NOT fall through to create-form validation
        // and return ENOSYS.
        let attr = LandlockRulesetAttr {
            handled_access_fs: LANDLOCK_ACCESS_FS_READ_FILE,
            handled_access_net: 0,
        };
        clear_errno();
        let v = landlock_create_ruleset(
            &raw const attr,
            core::mem::size_of::<LandlockRulesetAttr>(),
            LANDLOCK_CREATE_RULESET_ERRATA,
        );
        assert_eq!(v, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -- Workflow -----------------------------------------------------------

    #[test]
    fn test_phase133_libcap_landlock_probe_sequence() {
        // Mimics the libcap-landlock 0.6+ startup probe:
        // 1. Probe ABI version → uses the result to gate which
        //    access bits to request.
        // 2. Probe errata → uses the result to apply quirk workarounds.
        // 3. Try to create with the chosen access set.
        clear_errno();
        let ver = landlock_create_ruleset(core::ptr::null(), 0, LANDLOCK_CREATE_RULESET_VERSION);
        assert!(ver >= 1);
        assert_eq!(errno::get_errno(), 0);

        // After step 1, errno is still clean; step 2 must not disturb it.
        let errata = landlock_create_ruleset(core::ptr::null(), 0, LANDLOCK_CREATE_RULESET_ERRATA);
        // We advertise no errata, but the call succeeded with value 0.
        assert_eq!(errata, LANDLOCK_ERRATA);
        assert_eq!(errno::get_errno(), 0);

        // Step 3: real create.  Fails with ENOSYS until the backend lands.
        let attr = LandlockRulesetAttr {
            handled_access_fs: LANDLOCK_ACCESS_FS_READ_FILE,
            handled_access_net: 0,
        };
        let ret = landlock_create_ruleset(
            &raw const attr,
            core::mem::size_of::<LandlockRulesetAttr>(),
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_phase133_errata_probe_recoverable_after_einval() {
        // A buggy first call shouldn't poison subsequent good calls.
        clear_errno();
        // Bad: probe with non-zero size.
        let bad = landlock_create_ruleset(core::ptr::null(), 16, LANDLOCK_CREATE_RULESET_ERRATA);
        assert_eq!(bad, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        // Good: same flag, valid arguments.
        let good = landlock_create_ruleset(core::ptr::null(), 0, LANDLOCK_CREATE_RULESET_ERRATA);
        assert_eq!(good, LANDLOCK_ERRATA);
    }

    // ----------------------------------------------------------------------
    // Phase 185 — landlock_restrict_self privilege gate (NNP || CAP_SYS_ADMIN)
    //
    // Linux source (security/landlock/syscalls.c::SYSCALL_DEFINE2(
    //              landlock_restrict_self)):
    //
    //   if (!landlock_initialized) return -EOPNOTSUPP;
    //   if (!task_no_new_privs(current) &&
    //       !ns_capable_noaudit(current_user_ns(), CAP_SYS_ADMIN))
    //       return -EPERM;
    //   if (flags) return -EINVAL;
    //   ...
    //
    // So the gate runs *before* the flag-mask EINVAL check.  In our
    // model `task_no_new_privs(current)` maps to
    // `crate::unistd::no_new_privs_set()` (Phase 160 added the
    // atomic) and `ns_capable_noaudit(..., CAP_SYS_ADMIN)` to
    // `crate::sys_capability::has_capability(CAP_SYS_ADMIN)` (we
    // have a single user namespace).
    //
    // Host test build holds CAP_SYS_ADMIN by default, so all 49
    // pre-existing landlock tests reach the existing EBADF / EBADFD
    // / EINVAL paths unchanged.
    // ----------------------------------------------------------------------

    mod restrict_self_cap_phase185 {
        use super::*;

        struct CapGuard {

            lo: u32,

            hi: u32,

            // Held for the lifetime of the guard. See

            // `sys_capability::CAP_TEST_LOCK` for why.

            _lock: crate::sys_capability::CapTestLockGuard,

        }
        impl CapGuard {
            fn snapshot() -> Self {
            // Re-entrant lock guard: outermost acquire on the
            // thread takes the global mutex; nested acquires
            // (some tests stack a scoped CapGuard inside an
            // outer one) are no-ops for the lock but still
            // snapshot/restore caps independently.
            let lock = crate::sys_capability::CapTestLockGuard::acquire();
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            Self { lo, hi, _lock: lock }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
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

        /// RAII guard that resets `no_new_privs` to `false` on Drop.
        /// `PR_SET_NO_NEW_PRIVS` is irreversible by user API but we
        /// expose `_test_reset_no_new_privs` for cross-module tests.
        struct NnpGuard;
        impl NnpGuard {
            fn snapshot_and_clear() -> Self {
                crate::unistd::_test_reset_no_new_privs(false);
                Self
            }
        }
        impl Drop for NnpGuard {
            fn drop(&mut self) {
                crate::unistd::_test_reset_no_new_privs(false);
            }
        }

        fn drop_cap(cap: u32) {
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if cap < 32 {
                (lo & !(1u32 << cap), hi)
            } else {
                (lo, hi & !(1u32 << (cap - 32)))
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
            assert_eq!(rc, 0, "capset must succeed");
            assert!(!crate::sys_capability::has_capability(cap));
        }

        fn drop_sys_admin() {
            drop_cap(crate::sys_capability::CAP_SYS_ADMIN);
        }

        // -- Per-error-class ----------------------------------------------

        /// No CAP_SYS_ADMIN and NNP cleared → EPERM (the unprivileged
        /// caller path with no opt-out).
        #[test]
        fn test_landlock_phase185_no_cap_no_nnp_returns_eperm() {
            let _g = CapGuard::snapshot();
            let _n = NnpGuard::snapshot_and_clear();
            drop_sys_admin();
            errno::set_errno(0);
            let ret = landlock_restrict_self(0, 0);
            assert_eq!(ret, -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// CAP_SYS_ADMIN held → bypass NNP requirement → reach
        /// existing EBADFD path (no real ruleset).
        #[test]
        fn test_landlock_phase185_with_cap_no_nnp_reaches_ebadfd() {
            let _n = NnpGuard::snapshot_and_clear();
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_ADMIN
            ));
            errno::set_errno(0);
            let ret = landlock_restrict_self(0, 0);
            assert_eq!(ret, -1);
            assert_eq!(errno::get_errno(), errno::EBADFD);
        }

        /// No CAP_SYS_ADMIN but NNP set → bypass cap requirement →
        /// reach existing EBADFD path.
        #[test]
        fn test_landlock_phase185_with_nnp_no_cap_reaches_ebadfd() {
            let _g = CapGuard::snapshot();
            let _n = NnpGuard::snapshot_and_clear();
            drop_sys_admin();
            crate::unistd::_test_reset_no_new_privs(true);
            assert!(crate::unistd::no_new_privs_set());
            errno::set_errno(0);
            let ret = landlock_restrict_self(0, 0);
            assert_eq!(ret, -1);
            assert_eq!(errno::get_errno(), errno::EBADFD);
        }

        /// Errno must be EPERM (capable() convention), not EACCES —
        /// Linux's deliberate distinction.
        #[test]
        fn test_landlock_phase185_errno_is_eperm_not_eacces() {
            let _g = CapGuard::snapshot();
            let _n = NnpGuard::snapshot_and_clear();
            drop_sys_admin();
            errno::set_errno(0);
            let ret = landlock_restrict_self(0, 0);
            assert_eq!(ret, -1);
            assert_ne!(errno::get_errno(), errno::EACCES);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Ordering matrix ---------------------------------------------

        /// Unprivileged caller + invalid flag bit → EPERM beats
        /// EINVAL (cap gate runs first).
        #[test]
        fn test_landlock_phase185_eperm_beats_einval_invalid_flag() {
            let _g = CapGuard::snapshot();
            let _n = NnpGuard::snapshot_and_clear();
            drop_sys_admin();
            errno::set_errno(0);
            let ret = landlock_restrict_self(0, 0x1);
            assert_eq!(ret, -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// Unprivileged caller + negative ruleset_fd → EPERM beats
        /// EBADF (cap gate runs first).
        #[test]
        fn test_landlock_phase185_eperm_beats_ebadf_negative_fd() {
            let _g = CapGuard::snapshot();
            let _n = NnpGuard::snapshot_and_clear();
            drop_sys_admin();
            errno::set_errno(0);
            let ret = landlock_restrict_self(-1, 0);
            assert_eq!(ret, -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// Privileged caller + invalid flag bit → falls through to
        /// EINVAL (cap gate doesn't fire — confirms privileged path
        /// retains the EINVAL precedence over EBADF).
        #[test]
        fn test_landlock_phase185_admin_caller_invalid_flag_still_einval() {
            let _n = NnpGuard::snapshot_and_clear();
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_ADMIN
            ));
            errno::set_errno(0);
            let ret = landlock_restrict_self(-1, 0x1);
            assert_eq!(ret, -1);
            // Even with a bad fd, flag check runs first → EINVAL.
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        // -- No-side-effect ----------------------------------------------

        /// Denial must not mutate the cap set or NNP bit — proof
        /// the gate is a read-only check.
        #[test]
        fn test_landlock_phase185_eperm_preserves_caps_and_nnp() {
            let _g = CapGuard::snapshot();
            let _n = NnpGuard::snapshot_and_clear();
            drop_sys_admin();
            let (lo_before, hi_before) = crate::sys_capability::current_caps_effective();
            let nnp_before = crate::unistd::no_new_privs_set();
            errno::set_errno(0);
            let _ = landlock_restrict_self(0, 0);
            let (lo_after, hi_after) = crate::sys_capability::current_caps_effective();
            let nnp_after = crate::unistd::no_new_privs_set();
            assert_eq!(lo_before, lo_after);
            assert_eq!(hi_before, hi_after);
            assert_eq!(nnp_before, nnp_after);
        }

        // -- Recovery ----------------------------------------------------

        /// Drop cap + clear NNP → EPERM.  Then set NNP → success
        /// path is reachable (reaches EBADFD).
        #[test]
        fn test_landlock_phase185_recovery_set_nnp_reaches_ebadfd() {
            let _g = CapGuard::snapshot();
            let _n = NnpGuard::snapshot_and_clear();
            drop_sys_admin();
            errno::set_errno(0);
            let denied = landlock_restrict_self(0, 0);
            assert_eq!(denied, -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            crate::unistd::_test_reset_no_new_privs(true);
            errno::set_errno(0);
            let reached = landlock_restrict_self(0, 0);
            assert_eq!(reached, -1);
            assert_eq!(errno::get_errno(), errno::EBADFD);
        }

        // -- Workflow ----------------------------------------------------

        /// Typical unprivileged sandbox: a process drops CAP_SYS_ADMIN,
        /// sets PR_SET_NO_NEW_PRIVS, then calls landlock_restrict_self
        /// — matches the openssh, systemd, browser-sandbox pattern.
        /// Reaches EBADFD because we have no real ruleset backend
        /// (the equivalent of "Landlock not built into kernel").
        #[test]
        fn test_landlock_phase185_workflow_unprivileged_sandbox_with_nnp() {
            let _g = CapGuard::snapshot();
            let _n = NnpGuard::snapshot_and_clear();
            drop_sys_admin();
            crate::unistd::_test_reset_no_new_privs(true);
            // Probe + restrict in one shot.
            errno::set_errno(0);
            let ret = landlock_restrict_self(/* ruleset_fd */ 0, 0);
            assert_eq!(ret, -1);
            assert_eq!(errno::get_errno(), errno::EBADFD);
        }

        /// Buggy sandboxing pattern: process drops admin but forgets
        /// to call prctl(PR_SET_NO_NEW_PRIVS) before
        /// landlock_restrict_self.  Linux refuses with EPERM —
        /// catching this misconfiguration before any real
        /// restriction is in place.
        #[test]
        fn test_landlock_phase185_buggy_caller_forgets_nnp_returns_eperm() {
            let _g = CapGuard::snapshot();
            let _n = NnpGuard::snapshot_and_clear();
            drop_sys_admin();
            // (forgot: crate::unistd::_test_reset_no_new_privs(true);)
            errno::set_errno(0);
            let ret = landlock_restrict_self(0, 0);
            assert_eq!(ret, -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Sentinel ----------------------------------------------------

        /// CAP_SYS_PTRACE alone (without CAP_SYS_ADMIN) does NOT
        /// satisfy the gate — only CAP_SYS_ADMIN is honoured here,
        /// matching Linux's specific cap requirement.
        #[test]
        fn test_landlock_phase185_sentinel_ptrace_cap_does_not_satisfy() {
            let _g = CapGuard::snapshot();
            let _n = NnpGuard::snapshot_and_clear();
            drop_sys_admin();
            // CAP_SYS_PTRACE remains held by default (cap 19 in
            // DEFAULT_CAPS_LOW = u32::MAX).
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_PTRACE
            ));
            assert!(!crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_ADMIN
            ));
            errno::set_errno(0);
            let ret = landlock_restrict_self(0, 0);
            assert_eq!(ret, -1);
            // Still EPERM — PTRACE doesn't open the gate.
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Cross-checks ------------------------------------------------

        /// With NNP set the gate accepts any caller — symmetric
        /// for both held and dropped CAP_SYS_ADMIN.
        #[test]
        fn test_landlock_phase185_nnp_path_symmetric_across_caps() {
            // Held cap + NNP set.
            {
                let _n = NnpGuard::snapshot_and_clear();
                crate::unistd::_test_reset_no_new_privs(true);
                errno::set_errno(0);
                let ret = landlock_restrict_self(0, 0);
                assert_eq!(ret, -1);
                assert_eq!(errno::get_errno(), errno::EBADFD);
            }
            // Dropped cap + NNP set.
            {
                let _g = CapGuard::snapshot();
                let _n = NnpGuard::snapshot_and_clear();
                drop_sys_admin();
                crate::unistd::_test_reset_no_new_privs(true);
                errno::set_errno(0);
                let ret = landlock_restrict_self(0, 0);
                assert_eq!(ret, -1);
                assert_eq!(errno::get_errno(), errno::EBADFD);
            }
        }
    }
}
