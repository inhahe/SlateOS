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
//! * [`landlock_create_ruleset`] supports the **ABI probe** form
//!   (`attr == NULL && size == 0 && flags == LANDLOCK_CREATE_RULESET_VERSION`)
//!   exactly the way Linux does — returns the advertised ABI
//!   version ([`LANDLOCK_ABI_VERSION`]).  This is the very first
//!   call every Landlock-aware program makes; without it, the
//!   program assumes Landlock is unavailable and the rest never
//!   runs.
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
pub const LANDLOCK_ACCESS_FS_ALL: u64 =
    LANDLOCK_ACCESS_FS_EXECUTE
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

/// The Landlock ABI version we advertise.
///
/// Version 1 is the minimum specification (Linux 5.13).  We can
/// bump this as the kernel learns to honour the rule types and
/// access bits introduced by later versions.
pub const LANDLOCK_ABI_VERSION: i32 = 1;

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
/// Two forms:
///
/// 1. **Probe**: `attr == NULL && size == 0 && flags == LANDLOCK_CREATE_RULESET_VERSION`
///    — returns the advertised ABI version (positive int).  Every
///    Landlock-aware program does this first.
/// 2. **Create**: `flags == 0`, `attr` and `size` describe a valid
///    [`LandlockRulesetAttr`] — would return an fd; in our world
///    validates and reports `ENOSYS` because the kernel-side
///    enforcement isn't wired up.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn landlock_create_ruleset(
    attr: *const LandlockRulesetAttr,
    size: usize,
    flags: u32,
) -> i32 {
    // -- Probe form ---------------------------------------------------------
    //
    // The kernel: `attr == NULL && size == 0` makes any other `flags`
    // (including extra bits) EINVAL; only `LANDLOCK_CREATE_RULESET_VERSION`
    // alone is the probe.
    if attr.is_null() && size == 0 {
        if flags == LANDLOCK_CREATE_RULESET_VERSION {
            return LANDLOCK_ABI_VERSION;
        }
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // -- Real-create form ---------------------------------------------------
    //
    // Linux rejects any flags here (only the probe-mode flag is defined,
    // and it's not valid in create mode).
    if flags != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // NULL attr with non-zero size, or non-NULL attr with zero size, are
    // both nonsensical.  Linux reports EFAULT for the first and EINVAL
    // for the second (size doesn't cover the minimum attr struct).
    if attr.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if size < MIN_RULESET_ATTR_SIZE {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if size > MAX_RULESET_ATTR_SIZE {
        errno::set_errno(errno::E2BIG);
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
            let attr = unsafe {
                core::ptr::read_unaligned(rule_attr.cast::<LandlockPathBeneathAttr>())
            };
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
            let attr = unsafe {
                core::ptr::read_unaligned(rule_attr.cast::<LandlockNetPortAttr>())
            };
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
/// so this always fails with `EBADFD` after validating `flags`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn landlock_restrict_self(ruleset_fd: i32, flags: u32) -> i32 {
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
        assert_ne!(LANDLOCK_ACCESS_NET_BIND_TCP, LANDLOCK_ACCESS_NET_CONNECT_TCP);
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
        let v = landlock_create_ruleset(
            core::ptr::null(),
            0,
            LANDLOCK_CREATE_RULESET_VERSION,
        );
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
        let v = landlock_create_ruleset(
            core::ptr::null(),
            0,
            LANDLOCK_CREATE_RULESET_VERSION | 0x10,
        );
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
            handled_access_fs: LANDLOCK_ACCESS_FS_READ_FILE
                | LANDLOCK_ACCESS_FS_WRITE_FILE,
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
        let ret = landlock_add_rule(
            3,
            LANDLOCK_RULE_PATH_BENEATH,
            core::ptr::null(),
            0,
        );
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
        let ret = landlock_add_rule(
            3,
            LANDLOCK_RULE_NET_PORT,
            (&raw const attr).cast::<u8>(),
            0,
        );
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
        let ret = landlock_add_rule(
            3,
            LANDLOCK_RULE_NET_PORT,
            (&raw const attr).cast::<u8>(),
            0,
        );
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
        let ver = landlock_create_ruleset(
            core::ptr::null(),
            0,
            LANDLOCK_CREATE_RULESET_VERSION,
        );
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
}
