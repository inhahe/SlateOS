//! Which extended-attribute names a file may carry, and who may read and
//! write them: Linux 6.6's `xattr_permission` and `xattr_resolve_name`
//! (`fs/xattr.c`), with commoncap's `cap_inode_setxattr` and
//! `cap_inode_removexattr`, applied by the VFS for both ABIs (lane D's
//! request `d-a-xattr-answers-only-the-filesystem-can-give`).
//!
//! ## Namespaces
//!
//! A name is `namespace.rest`. Linux's filesystems hand each namespace to a
//! handler and refuse a name no handler takes. The VFS keeps three:
//!
//! | Namespace | Read | Write |
//! |---|---|---|
//! | `user.` | regular files and directories; on anything else it reads as absent | regular files and directories; a sticky directory's, its owner only |
//! | `trusted.` | privileged callers; others find it absent | privileged callers |
//! | `security.` | anyone | privileged callers |
//!
//! Any other name is refused where Linux's filesystems refuse it:
//! `NotSupported` (`EOPNOTSUPP`) for a name in no namespace kept here --
//! `foo`, `usr.x`, any `system.` name -- and `InvalidArgument` (`EINVAL`) for
//! a bare prefix, `user.`. Until 2026-10-01 both were stored: memfs kept any
//! name, and ext4 filed an unprefixed one under index 0. One stored then is
//! left where it is, and is neither listed nor read.
//!
//! **`system.` is refused.** On Linux it holds the POSIX ACLs
//! (`system.posix_acl_access`), which its filesystems translate into the ACL
//! they enforce. Here the ACL is `fs::acl`'s, not an attribute, so storing
//! one under that name would grant nothing while claiming to: `setfacl` meets
//! `EOPNOTSUPP`, as on a filesystem mounted without ACLs, and falls back to
//! the mode.
//!
//! ## Privilege
//!
//! "Privileged" is Linux's `CAP_SYS_ADMIN` (for `security.capability`,
//! `CAP_SETFCAP`), which this kernel models as user id 0, as its Linux layer
//! does elsewhere (`sethostname`, `swapon`). A kernel task, acting for no
//! process, is privileged.
//!
//! ## Order
//!
//! Linux 6.6's, which the VFS keeps (`Vfs::xattr_on`):
//!
//! 1. the path; for a change, the mount's writability (`EROFS`);
//! 2. [`namespace_rules`]: an immutable or append-only file takes no change
//!    (`EPERM`); then the `trusted.` and `user.` rules above;
//! 3. the file's own permission -- Linux's `inode_permission`, which here is
//!    the ACL -- for `user.` and names in no namespace only
//!    ([`Namespace::checks_permission`]);
//! 4. [`after_permission`]: a change to `security.` wants privilege; then the
//!    name is resolved ([`resolve`]);
//! 5. the filesystem: `NoAttribute` (`ENODATA`) for an absent attribute,
//!    `AlreadyExists` (`EEXIST`) for `XATTR_CREATE` on a present one.
//!
//! A listing is checked against nothing, as Linux's `listxattr` is not: it
//! shows a name in a namespace kept here, a `trusted.` one to a privileged
//! caller only ([`listed`]).
//!
//! The capability tags (`cap::file_tags`), mandatory access control that
//! Linux does not have, come before all of it, as for every VFS call.

use crate::error::{KernelError, KernelResult};

use super::vfs::{EntryType, FileAttr, FileMeta};

/// The namespace a name begins with, as Linux's `xattr_permission` tells
/// them apart: by its prefix alone, so the bare prefix `user.` is in
/// [`User`](Self::User) for the rules, and refused afterwards by [`resolve`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Namespace {
    /// `user.`: what a program keeps on the files it may write.
    User,
    /// `trusted.`: privileged callers only, reading or writing.
    Trusted,
    /// `security.`: anyone reads; privileged callers write.
    Security,
    /// `system.`: refused (see the module doc).
    System,
    /// No namespace: refused.
    Other,
}

impl Namespace {
    /// The namespace `name` begins with.
    #[must_use]
    pub fn of(name: &[u8]) -> Self {
        if name.starts_with(b"user.") {
            Self::User
        } else if name.starts_with(b"trusted.") {
            Self::Trusted
        } else if name.starts_with(b"security.") {
            Self::Security
        } else if name.starts_with(b"system.") {
            Self::System
        } else {
            Self::Other
        }
    }

    /// Whether Linux's `xattr_permission` ends in `inode_permission` -- the
    /// file's own permission, which here is its ACL -- for a name in this
    /// namespace: for `user.` and for no namespace. `security.` and `system.`
    /// are left to the security hooks, and `trusted.` to privilege.
    #[must_use]
    pub const fn checks_permission(self) -> bool {
        matches!(self, Self::User | Self::Other)
    }
}

/// What an operation does with an attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// `getxattr`.
    Read,
    /// `setxattr`, `removexattr`.
    Write,
}

/// Who is asking: what the rules need to know of the caller. Taken before
/// the filesystem's lock (`Caller::current` takes the thread and process
/// tables' locks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caller {
    /// Linux's `CAP_SYS_ADMIN`, modelled as user id 0; a kernel task, acting
    /// for no process, has it.
    pub privileged: bool,
    /// The calling process's user id; `None` for a kernel task.
    pub uid: Option<u32>,
}

impl Caller {
    /// The calling task.
    #[must_use]
    pub fn current() -> Self {
        let task = crate::sched::current_task_id();
        let pid = crate::proc::thread::owner_process(task).filter(|&pid| pid != 0);
        let uid = pid
            .and_then(crate::proc::pcb::get_credentials)
            .map(|c| c.uid);
        Self {
            privileged: pid.is_none() || uid == Some(0),
            uid,
        }
    }
}

/// Linux's `xattr_permission` up to the file's own permission, with
/// `may_write_xattr` before it: whether the caller may `access` a name in
/// `namespace` on the file `meta` describes.
///
/// # Errors
///
/// - `NotPermitted` (`EPERM`) for a change: to an immutable or append-only
///   file, whoever asks; to `trusted.` without privilege; to `user.` of
///   anything but a regular file or a directory, or of a sticky directory by
///   anyone but its owner without privilege.
/// - `NoAttribute` (`ENODATA`) for a read of `trusted.` without privilege,
///   or of `user.` of anything but a regular file or a directory: the
///   attribute reads as absent, as on Linux.
pub fn namespace_rules(
    namespace: Namespace,
    access: Access,
    meta: &FileMeta,
    caller: Caller,
) -> KernelResult<()> {
    let write = access == Access::Write;
    if write
        && (meta.attributes.contains(FileAttr::IMMUTABLE)
            || meta.attributes.contains(FileAttr::APPEND_ONLY))
    {
        return Err(KernelError::NotPermitted);
    }
    // A refusal is an error to a change and an absence to a read.
    let refused = if write {
        KernelError::NotPermitted
    } else {
        KernelError::NoAttribute
    };
    match namespace {
        Namespace::Trusted if !caller.privileged => Err(refused),
        Namespace::User => {
            if !matches!(meta.entry_type, EntryType::File | EntryType::Directory) {
                return Err(refused);
            }
            let sticky = meta.entry_type == EntryType::Directory && meta.permissions & 0o1000 != 0;
            // A kernel task has no uid and is privileged; a process whose
            // credentials are gone (being torn down) owns nothing.
            let owner = caller.uid == Some(meta.uid);
            if write && sticky && !owner && !caller.privileged {
                return Err(KernelError::NotPermitted);
            }
            Ok(())
        }
        Namespace::Trusted | Namespace::Security | Namespace::System | Namespace::Other => Ok(()),
    }
}

/// What Linux decides after the file's own permission: a change to
/// `security.` wants privilege (commoncap's `cap_inode_setxattr` and
/// `cap_inode_removexattr`); then the name is resolved -- by [`resolve`] on a
/// filesystem that keeps attributes (`supported`), and refused outright on one
/// that keeps none, as Linux's `xattr_resolve_name` refuses an inode without
/// `IOP_XATTR` before it looks at the name.
///
/// # Errors
///
/// `NotPermitted` (`EPERM`) for a change to `security.` without privilege;
/// `NotSupported` (`EOPNOTSUPP`) when not `supported`; [`resolve`]'s.
pub fn after_permission(
    name: &[u8],
    access: Access,
    caller: Caller,
    supported: bool,
) -> KernelResult<()> {
    if access == Access::Write && Namespace::of(name) == Namespace::Security && !caller.privileged {
        return Err(KernelError::NotPermitted);
    }
    if !supported {
        return Err(KernelError::NotSupported);
    }
    resolve(name)
}

/// Linux's `xattr_resolve_name` on a filesystem with handlers for `user.`,
/// `trusted.` and `security.`: whether one of them takes `name`.
///
/// # Errors
///
/// - `InvalidArgument` (`EINVAL`): a bare prefix -- `user.`, `trusted.`,
///   `security.`.
/// - `NotSupported` (`EOPNOTSUPP`): any other name in no namespace kept
///   here, `system.` and its bare prefix among them.
pub fn resolve(name: &[u8]) -> KernelResult<()> {
    let kept: [&[u8]; 3] = [b"user.", b"trusted.", b"security."];
    for prefix in kept {
        if name.starts_with(prefix) {
            return if name.len() == prefix.len() {
                Err(KernelError::InvalidArgument)
            } else {
                Ok(())
            };
        }
    }
    Err(KernelError::NotSupported)
}

/// Whether a listing shows `name`: a name in a namespace kept here, and a
/// `trusted.` one to a privileged caller only, as Linux's filesystems list
/// them (ext4's `ext4_xattr_trusted_list`, tmpfs's `simple_xattr_list`).
#[must_use]
pub fn listed(name: &[u8], privileged: bool) -> bool {
    resolve(name).is_ok() && (privileged || Namespace::of(name) != Namespace::Trusted)
}

/// The rules without a filesystem: names, each refusal, a sticky directory,
/// the order of the steps, the listing filter. `fs::vfs::self_test_xattr_rules`
/// proves the VFS applies them.
///
/// # Errors
///
/// `InternalError` naming the rule that failed.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    fn meta(entry_type: EntryType, permissions: u16, uid: u32, attributes: FileAttr) -> FileMeta {
        let mut m = FileMeta::minimal(entry_type, 0);
        m.permissions = permissions;
        m.uid = uid;
        m.attributes = attributes;
        m
    }
    fn fail(what: &str) -> KernelResult<()> {
        serial_println!("[xattr_policy]   FAIL: {}", what);
        Err(KernelError::InternalError)
    }
    /// Steps 2 and 4 together, as a call meets them when step 3 allows it,
    /// on a filesystem that keeps attributes.
    fn decide(name: &[u8], access: Access, meta: &FileMeta, caller: Caller) -> KernelResult<()> {
        namespace_rules(Namespace::of(name), access, meta, caller)?;
        after_permission(name, access, caller, true)
    }

    // Names: the namespace each is in, and whether a handler takes it.
    let names: [(&[u8], Namespace, KernelResult<()>); 12] = [
        (b"user.mime_type", Namespace::User, Ok(())),
        (b"trusted.overlay.opaque", Namespace::Trusted, Ok(())),
        (b"security.capability", Namespace::Security, Ok(())),
        (
            b"system.posix_acl_access",
            Namespace::System,
            Err(KernelError::NotSupported),
        ),
        (
            b"system.",
            Namespace::System,
            Err(KernelError::NotSupported),
        ),
        (b"foo", Namespace::Other, Err(KernelError::NotSupported)),
        (b"usr.x", Namespace::Other, Err(KernelError::NotSupported)),
        (b"", Namespace::Other, Err(KernelError::NotSupported)),
        (b"user", Namespace::Other, Err(KernelError::NotSupported)),
        (b"user.", Namespace::User, Err(KernelError::InvalidArgument)),
        (
            b"trusted.",
            Namespace::Trusted,
            Err(KernelError::InvalidArgument),
        ),
        (
            b"security.",
            Namespace::Security,
            Err(KernelError::InvalidArgument),
        ),
    ];
    for (name, namespace, resolved) in names {
        if Namespace::of(name) != namespace || resolve(name) != resolved {
            serial_println!(
                "[xattr_policy]   {:?}: {:?}, {:?}",
                name,
                Namespace::of(name),
                resolve(name)
            );
            return fail("a name's namespace or resolution");
        }
    }

    let file = meta(EntryType::File, 0o644, 1000, FileAttr::NONE);
    let dir = meta(EntryType::Directory, 0o755, 1000, FileAttr::NONE);
    let sticky = meta(EntryType::Directory, 0o1777, 1000, FileAttr::NONE);
    let link = meta(EntryType::Symlink, 0o777, 1000, FileAttr::NONE);
    let frozen = meta(EntryType::File, 0o644, 1000, FileAttr::IMMUTABLE);
    let appending = meta(EntryType::File, 0o644, 1000, FileAttr::APPEND_ONLY);
    let (r, w) = (Access::Read, Access::Write);
    let root = Caller {
        privileged: true,
        uid: Some(0),
    };
    let owner = Caller {
        privileged: false,
        uid: Some(1000),
    };
    let stranger = Caller {
        privileged: false,
        uid: Some(2000),
    };
    let denied = Err(KernelError::NotPermitted);
    let absent = Err(KernelError::NoAttribute);
    let cases: [(&str, KernelResult<()>, KernelResult<()>); 22] = [
        // user.: regular files and directories, a sticky one by its owner.
        (
            "user. on a file",
            decide(b"user.a", w, &file, owner),
            Ok(()),
        ),
        (
            "user. on another's directory",
            decide(b"user.a", w, &dir, stranger),
            Ok(()),
        ),
        (
            "user. written on a link",
            decide(b"user.a", w, &link, root),
            denied,
        ),
        (
            "user. read on a link",
            decide(b"user.a", r, &link, root),
            absent,
        ),
        (
            "user. on another's sticky dir",
            decide(b"user.a", w, &sticky, stranger),
            denied,
        ),
        (
            "user. on one's own sticky dir",
            decide(b"user.a", w, &sticky, owner),
            Ok(()),
        ),
        (
            "user. on a sticky dir, privileged",
            decide(b"user.a", w, &sticky, root),
            Ok(()),
        ),
        (
            "user. read on a sticky dir",
            decide(b"user.a", r, &sticky, stranger),
            Ok(()),
        ),
        // trusted.: privileged only, absent to a read otherwise.
        (
            "trusted. read, unprivileged",
            decide(b"trusted.a", r, &file, owner),
            absent,
        ),
        (
            "trusted. written, unprivileged",
            decide(b"trusted.a", w, &file, owner),
            denied,
        ),
        (
            "trusted. written, privileged",
            decide(b"trusted.a", w, &file, root),
            Ok(()),
        ),
        (
            "trusted. on a link, privileged",
            decide(b"trusted.a", w, &link, root),
            Ok(()),
        ),
        // security.: anyone reads; a change wants privilege.
        (
            "security. read",
            decide(b"security.a", r, &file, stranger),
            Ok(()),
        ),
        (
            "security. written, unprivileged",
            decide(b"security.a", w, &file, owner),
            denied,
        ),
        (
            "security. written, privileged",
            decide(b"security.a", w, &file, root),
            Ok(()),
        ),
        // No change to an immutable or append-only file, privileged or not.
        (
            "a change to an immutable file",
            decide(b"user.a", w, &frozen, root),
            denied,
        ),
        (
            "a change to an append-only file",
            decide(b"user.a", w, &appending, root),
            denied,
        ),
        (
            "a read of an immutable file",
            decide(b"user.a", r, &frozen, owner),
            Ok(()),
        ),
        // The order: the rules before the name is resolved.
        (
            "user. bare, on a link",
            decide(b"user.", w, &link, root),
            denied,
        ),
        (
            "trusted. bare, unprivileged",
            decide(b"trusted.", r, &file, owner),
            absent,
        ),
        (
            "trusted. bare, privileged",
            decide(b"trusted.", r, &file, root),
            Err(KernelError::InvalidArgument),
        ),
        (
            "no namespace, immutable file",
            decide(b"foo", w, &frozen, root),
            denied,
        ),
    ];
    for (what, got, want) in cases {
        if got != want {
            serial_println!("[xattr_policy]   {}: {:?}, want {:?}", what, got, want);
            return fail("a permission rule");
        }
    }

    // Where nothing keeps attributes, every name is unsupported -- a bare
    // prefix too -- but a change to security. is still refused first.
    let unkept: [(&[u8], Access, Caller, KernelResult<()>); 4] = [
        (b"user.a", r, root, Err(KernelError::NotSupported)),
        (b"trusted.", r, root, Err(KernelError::NotSupported)),
        (b"foo", w, root, Err(KernelError::NotSupported)),
        (b"security.a", w, owner, denied),
    ];
    for (name, access, caller, want) in unkept {
        if after_permission(name, access, caller, false) != want {
            serial_println!("[xattr_policy]   {:?} where nothing is kept", name);
            return fail("a filesystem without attributes");
        }
    }

    // Linux's file permission is consulted for user. and no namespace only.
    if !Namespace::User.checks_permission()
        || !Namespace::Other.checks_permission()
        || Namespace::Trusted.checks_permission()
        || Namespace::Security.checks_permission()
        || Namespace::System.checks_permission()
    {
        return fail("which namespaces consult the file's permission");
    }

    if !listed(b"user.a", false)
        || listed(b"trusted.a", false)
        || !listed(b"trusted.a", true)
        || !listed(b"security.selinux", false)
        || listed(b"foo", true)
        || listed(b"system.posix_acl_access", true)
        || listed(b"user.", true)
    {
        return fail("the listing filter");
    }
    serial_println!(
        "[xattr_policy]   names, the user./trusted./security. rules, sticky \
         directories, immutable files, their order, listing: OK"
    );
    Ok(())
}
