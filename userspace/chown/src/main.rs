//! Slate OS File Ownership and Permission Utility
//!
//! Dual-mode binary: invoked as `chown` it changes file owner/group; invoked
//! as `chmod` it changes file permission bits. Mode detection is via `argv[0]`.
//!
//! User/group name resolution reads `/etc/users.yaml`, the Slate OS user database.
//!
//! # Usage (chown mode)
//!
//! ```text
//! chown OWNER[:GROUP] FILE...         Change owner (and optionally group)
//! chown :GROUP FILE...                Change group only
//! chown -R OWNER FILE...              Recursive
//! chown -v OWNER FILE...              Verbose: report every file processed
//! chown -c OWNER FILE...              Report only actual changes
//! chown -f OWNER FILE...              Suppress error messages
//! chown -h OWNER LINK                 Change symlink itself, not target
//! chown --from=CUR:GRP OWNER FILE     Only change if current owner/group match
//! chown --reference=REF FILE...       Copy owner/group from REF
//! chown --json OWNER FILE...          JSON output
//! ```
//!
//! # Usage (chmod mode)
//!
//! ```text
//! chmod 755 FILE...                   Octal mode
//! chmod u+x FILE...                   Symbolic: add execute for user
//! chmod g-w,o-w FILE...               Symbolic: remove write for group+other
//! chmod a=rx FILE...                  Symbolic: set exact permissions for all
//! chmod -R 644 DIR/...                Recursive
//! chmod -v 755 FILE                   Verbose
//! chmod -c 755 FILE                   Report only changes
//! chmod --reference=REF FILE...       Copy mode from REF
//! chmod --json 755 FILE               JSON output
//! ```

use quoting::quoteaf_os;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;

use userdb::UserDb;

// ============================================================================
// Syscall numbers (fs zone: 600-799)
// ============================================================================
//
// These map to the real Slate OS VFS handlers. The previous version targeted
// Linux numbers 30/31 — which on Slate OS are IRQ_REGISTER / IRQ_WAIT, so a chown
// or chmod would have tried to register or block on a hardware interrupt line.

/// Read file metadata (`SYS_FS_METADATA`).
///
/// arg0 = path pointer, arg1 = path length, arg2 = output buffer pointer
/// (`FS_META_SIZE` bytes). On success returns 0 and fills the buffer.
const SYS_FS_METADATA: u64 = 628;

/// Change file owner and group (`SYS_FS_SET_OWNER`).
///
/// arg0 = path pointer, arg1 = path length, arg2 = uid (u32), arg3 = gid (u32),
/// arg4 bit 0 = NO_FOLLOW.
/// A uid or gid of `u32::MAX` means "leave that field unchanged"; the kernel
/// resolves the sentinel against the file's current owner.
///
/// arg4 is what libc's `lchown` passes (`posix/src/file.rs` →
/// `set_owner_path_ex`): clear, the kernel resolves the final symlink and
/// chowns its target; set, it chowns the link inode itself.
const SYS_FS_SET_OWNER: u64 = 630;

/// Change file permission mode bits (`SYS_FS_SET_PERMS`).
///
/// arg0 = path pointer, arg1 = path length, arg2 = mode (low 12 bits used:
/// rwx + setuid/setgid/sticky).
const SYS_FS_SET_PERMS: u64 = 631;

/// Size of the `SYS_FS_METADATA` output buffer, in bytes.
const FS_META_SIZE: usize = 64;

/// Byte offset of the u32 uid field within the metadata buffer.
const META_OFF_UID: usize = 48;
/// Byte offset of the u32 gid field within the metadata buffer.
const META_OFF_GID: usize = 52;
/// Byte offset of the u16 permission-bits field within the metadata buffer.
const META_OFF_PERMS: usize = 56;

// ============================================================================
// Low-level syscall interface
// ============================================================================

/// Issue a four-argument syscall using the x86-64 `syscall` instruction.
///
/// Register mapping follows the Slate OS syscall ABI:
///   rax = syscall number, rdi = arg0, rsi = arg1, rdx = arg2, r10 = arg3
///   Return value in rax. rcx and r11 are clobbered by the CPU.
///
/// Three-argument syscalls pass 0 for `a4`.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall4(nr: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller ensures arguments are valid for the given syscall number.
    // The `syscall` instruction is the defined kernel entry point on x86-64.
    // The kernel reads arg3 from r10 (not rcx, which the syscall instruction
    // overwrites with the return address). rcx and r11 are clobbered per the
    // hardware specification.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            in("r10") a4,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Issue a five-argument syscall. `a5` goes in r8, continuing the register
/// mapping documented on [`syscall4`].
///
/// Needed for `SYS_FS_SET_OWNER`'s fifth argument, the NO_FOLLOW bit — the
/// difference between changing a symbolic link and changing whatever it points
/// at. Without it there is no way to express `lchown(2)`, and `chown -R` on a
/// tree containing `link -> /etc/shadow` hands `/etc/shadow` to the new owner.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall5(nr: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller ensures arguments are valid for the given syscall number.
    // Identical contract to `syscall4`, plus arg4 in r8 as the Slate OS ABI
    // specifies. rcx and r11 are clobbered per the hardware specification.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            in("r10") a4,
            in("r8") a5,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Convenience wrapper for three-argument syscalls.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    // SAFETY: forwarded to syscall4 with a zero fourth argument; the safety
    // contract is identical and upheld by the caller.
    unsafe { syscall4(nr, a1, a2, a3, 0) }
}

// ============================================================================
// Error helpers
// ============================================================================

/// Map a negative Slate OS kernel error code to a human-readable string.
///
/// These are `KernelError` discriminants (see kernel `error.rs`), NOT Linux
/// errnos — e.g. -2 is "operation not supported", not ENOENT.
fn kernel_error_to_string(code: i64) -> String {
    let msg = match code {
        -1 => "internal kernel error",
        -2 => "operation not supported",
        -3 => "invalid argument",
        -400 => "permission denied",
        -401 => "invalid capability",
        -500 => "no such file or directory",
        -502 => "not a directory",
        -503 => "is a directory",
        -505 => "invalid handle",
        -506 => "too many symbolic links",
        -509 => "read-only filesystem",
        -600 => "I/O error",
        -601 => "no such device",
        _ => return format!("error {code}"),
    };
    format!("{msg} ({code})")
}

// ============================================================================
// User/group database (reads /etc/users.yaml)
// ============================================================================

/// A resolved group with a numeric GID.
///
/// Slate OS assigns GIDs by order of appearance in the groups collected across
/// all user entries. Group 0 = "root", group 1 = "admin", etc. The exact
/// mapping is built at runtime from `/etc/users.yaml`.
struct GroupEntry {
    gid: u32,
    name: String,
}

/// Read the user database, treating an absent or unreadable file as empty.
///
/// An empty database means names cannot be resolved, so `chown alice f` fails
/// with "invalid user" rather than silently doing something else — which is the
/// right failure: chown's whole job is to name an owner, and guessing one would
/// change the file to an owner the user did not ask for.
fn read_users() -> UserDb {
    match UserDb::load(userdb::DEFAULT_PATH) {
        Ok(db) => db,
        Err(e) => {
            if e.kind() != io::ErrorKind::NotFound {
                eprintln!("chown: cannot read {}: {e}", userdb::DEFAULT_PATH);
            }
            UserDb::new()
        }
    }
}

/// Build the group table by collecting every unique group name from all users
/// and assigning GIDs in order. Well-known groups get fixed IDs:
///   root=0, admin=1, users=100.
fn build_group_table(users: &UserDb) -> Vec<GroupEntry> {
    let mut groups = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Well-known groups first.
    for (name, gid) in [("root", 0u32), ("admin", 1), ("users", 100)] {
        groups.push(GroupEntry {
            gid,
            name: name.to_string(),
        });
        seen.insert(name.to_string());
    }

    let mut next_gid: u32 = 101;
    for user in users.records() {
        // An administrator is a member of `wheel` whether or not the field
        // lists it: the database records administrator-ness as a flag, and a
        // `chgrp wheel` that failed with "invalid group" on a machine that
        // plainly has administrators would be inexplicable.
        let mut names = user.groups();
        if user.is_admin() {
            names.push("wheel".to_string());
        }
        for g in names {
            if !seen.contains(&g) {
                groups.push(GroupEntry {
                    gid: next_gid,
                    name: g.clone(),
                });
                seen.insert(g);
                next_gid = next_gid.saturating_add(1);
            }
        }
    }

    groups
}

/// Resolve a username to a UID.
fn resolve_uid(name: &str, users: &UserDb) -> Option<u32> {
    // Try numeric first.
    if let Ok(n) = name.parse::<u32>() {
        return Some(n);
    }
    users.find(name).and_then(userdb::Record::uid)
}

/// Resolve a group name to a GID.
fn resolve_gid(name: &str, groups: &[GroupEntry]) -> Option<u32> {
    // Try numeric first.
    if let Ok(n) = name.parse::<u32>() {
        return Some(n);
    }
    groups.iter().find(|g| g.name == name).map(|g| g.gid)
}

// ============================================================================
// Filesystem helpers
// ============================================================================

/// Resolved file metadata fields that chown/chmod care about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileMeta {
    uid: u32,
    gid: u32,
    /// Permission bits (low 12: rwx + setuid/setgid/sticky).
    perms: u32,
}

/// Parse the uid/gid/perms fields out of a raw `SYS_FS_METADATA` buffer.
///
/// Split out from [`read_metadata`] so it can be unit-tested on the host where
/// the syscall cannot run. Returns `None` if the buffer is too small.
fn parse_metadata_buffer(buf: &[u8]) -> Option<FileMeta> {
    let uid_bytes = buf.get(META_OFF_UID..META_OFF_UID + 4)?;
    let gid_bytes = buf.get(META_OFF_GID..META_OFF_GID + 4)?;
    let perm_bytes = buf.get(META_OFF_PERMS..META_OFF_PERMS + 2)?;

    let uid = u32::from_le_bytes([uid_bytes[0], uid_bytes[1], uid_bytes[2], uid_bytes[3]]);
    let gid = u32::from_le_bytes([gid_bytes[0], gid_bytes[1], gid_bytes[2], gid_bytes[3]]);
    let perms = u16::from_le_bytes([perm_bytes[0], perm_bytes[1]]) as u32;

    Some(FileMeta { uid, gid, perms })
}

/// Read a file's metadata via `SYS_FS_METADATA`.
///
/// Returns the owner uid, group gid, and permission bits. Used both to
/// implement `--reference` (copy owner/mode from another file) and to detect
/// whether an operation actually changed anything (for `-c` / `-v`).
#[cfg(target_arch = "x86_64")]
fn read_metadata(path: &str) -> Result<FileMeta, String> {
    let mut buf = [0u8; FS_META_SIZE];

    // SAFETY: SYS_FS_METADATA reads `path.len()` bytes from `path.as_ptr()` and
    // writes exactly `FS_META_SIZE` bytes to `buf`. Both the path slice and the
    // stack buffer are valid for the duration of the syscall, and `buf` is sized
    // to the ABI-defined output length.
    let ret = unsafe {
        syscall3(
            SYS_FS_METADATA,
            path.as_ptr() as u64,
            path.len() as u64,
            buf.as_mut_ptr() as u64,
        )
    };

    if ret < 0 {
        return Err(kernel_error_to_string(ret));
    }

    parse_metadata_buffer(&buf).ok_or_else(|| "metadata buffer too small".to_string())
}

/// Host fallback: the metadata syscall cannot run on the build host.
#[cfg(not(target_arch = "x86_64"))]
fn read_metadata(_path: &str) -> Result<FileMeta, String> {
    Err("metadata unavailable on this platform".to_string())
}

/// Perform the chown syscall on a single path.
///
/// `uid` and `gid` are the new owner/group. Pass `u32::MAX` for either to
/// leave it unchanged (the kernel interprets `0xFFFFFFFF` as "no change",
/// resolving the sentinel against the file's current owner in the VFS layer).
///
/// `no_follow` selects `lchown(2)` semantics: the symbolic link itself is
/// chowned rather than its target. Every caller must think about this — see
/// [`follow_operand`] for which way round it goes and why.
#[cfg(target_arch = "x86_64")]
fn do_chown(path: &str, uid: u32, gid: u32, no_follow: bool) -> Result<(), String> {
    // SAFETY: SYS_FS_SET_OWNER reads `path.len()` bytes from `path.as_ptr()`
    // and takes uid in arg2, gid in arg3 and the NO_FOLLOW bit in arg4. The
    // path slice outlives the call.
    let ret = unsafe {
        syscall5(
            SYS_FS_SET_OWNER,
            path.as_ptr() as u64,
            path.len() as u64,
            uid as u64,
            gid as u64,
            u64::from(no_follow),
        )
    };

    if ret < 0 {
        Err(kernel_error_to_string(ret))
    } else {
        Ok(())
    }
}

/// Host fallback so the crate compiles for tests on non-x86_64 hosts.
#[cfg(not(target_arch = "x86_64"))]
fn do_chown(_path: &str, _uid: u32, _gid: u32, _no_follow: bool) -> Result<(), String> {
    Err("chown syscall unavailable on this platform".to_string())
}

/// Perform the chmod syscall on a single path.
#[cfg(target_arch = "x86_64")]
fn do_chmod(path: &str, mode: u32) -> Result<(), String> {
    // SAFETY: SYS_FS_SET_PERMS reads `path.len()` bytes from `path.as_ptr()`
    // and takes the new mode (low 12 bits) in arg2. The path slice outlives
    // the call.
    let ret = unsafe {
        syscall3(
            SYS_FS_SET_PERMS,
            path.as_ptr() as u64,
            path.len() as u64,
            (mode & 0o7777) as u64,
        )
    };

    if ret < 0 {
        Err(kernel_error_to_string(ret))
    } else {
        Ok(())
    }
}

/// Host fallback so the crate compiles for tests on non-x86_64 hosts.
#[cfg(not(target_arch = "x86_64"))]
fn do_chmod(_path: &str, _mode: u32) -> Result<(), String> {
    Err("chmod syscall unavailable on this platform".to_string())
}

// ============================================================================
// Recursive traversal — and the symlink rules that make it safe
// ============================================================================
//
// `-R` walks a tree the caller named. Every other path in that tree was named
// by whoever created the files, which under `/tmp`, a download directory or a
// user's home is not the caller. A symbolic link is therefore a hostile edge,
// and the two questions below decide whether it is also an exit.
//
//   1. Do we walk *into* it? POSIX's answer for `chown -R` with none of
//      `-H`/`-L`/`-P` given is `-P`: no. `srv/x -> /etc` must not turn
//      `chown -R alice srv/` into `chown -R alice /etc`.
//   2. Do we chown the link or its target? `chown(2)` follows, so the naive
//      answer hands `/etc/shadow` to alice via `srv/x -> /etc/shadow`. The
//      answer is the link, i.e. `lchown(2)`.
//
// chmod has a third rule of its own: it does not touch symbolic links at all
// during a recursive walk, because their mode bits are meaningless and the
// only thing a chmod on one can do is change the target's.

/// One entry from a recursive walk.
///
/// `is_symlink` comes from `read_dir`'s own `file_type()`, which is `lstat`-
/// based and free — no extra syscall, and no window between the walk deciding
/// what a name is and the caller acting on it.
struct WalkEntry {
    path: PathBuf,
    is_symlink: bool,
}

/// Recursively collect all paths under a directory (depth-first).
///
/// The directory itself is included as the last entry so that ownership/mode
/// changes propagate from leaves to root (allowing the directory to remain
/// readable during traversal).
///
/// Symbolic links are collected but never descended into, whatever they point
/// at. This is `-P`, POSIX's default for `chown -R`, and it is the reason the
/// walk cannot leave the tree it was pointed at.
fn collect_recursive(base: &Path) -> Vec<WalkEntry> {
    let mut results = Vec::new();
    collect_recursive_inner(base, &mut results);
    results
}

fn collect_recursive_inner(dir: &Path, out: &mut Vec<WalkEntry>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => {
            // Cannot read this directory -- include it anyway so the caller
            // can report the error during the actual chown/chmod call.
            out.push(WalkEntry {
                path: dir.to_path_buf(),
                is_symlink: false,
            });
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => {
                // Unknown type: treat it as a symlink, the conservative
                // reading. The worst that costs is a link left unchanged;
                // guessing the other way costs the target.
                out.push(WalkEntry {
                    path,
                    is_symlink: true,
                });
                continue;
            }
        };

        // `file_type()` is lstat-based, so `is_dir()` is false for a symlink
        // to a directory and the recursion below cannot follow one.
        if ft.is_dir() {
            collect_recursive_inner(&path, out);
        } else {
            out.push(WalkEntry {
                path,
                is_symlink: ft.is_symlink(),
            });
        }
    }

    // Directory itself comes last (leaf-first order).
    out.push(WalkEntry {
        path: dir.to_path_buf(),
        is_symlink: false,
    });
}

/// Whether a **command-line operand** should be dereferenced.
///
/// Split out as a pure function so the rule is testable on the build host,
/// where none of the syscalls above exist. The rule:
///
/// * `-h`/`--no-dereference` always wins — that is the whole point of it.
/// * Without `-R`, a bare `chown alice link` follows, as POSIX requires: the
///   operand was named by the caller, who can see what it is.
/// * With `-R`, it does not. POSIX makes `-P` the default for recursion
///   specifically so that a link cannot smuggle the walk out of the named
///   tree, and that applies to the root of the walk as much as to its leaves.
fn follow_operand(recursive: bool, no_deref: bool) -> bool {
    !no_deref && !recursive
}

/// Whether an entry found *during* a recursive walk should be dereferenced.
///
/// Never. This binary implements neither `-H` nor `-L`, so there is no flag
/// that could ask for it, and following here is exactly the escape described
/// at the top of this section.
const fn follow_child() -> bool {
    false
}

// ============================================================================
// Mode parsing (chmod)
// ============================================================================
//
// This section used to be ~260 lines of hand-written parser -- `ModeClause`,
// `parse_symbolic_mode`, `clause_bits`, `clause_who_mask`, `apply_symbolic_mode`
// and `parse_mode`. It was the third of four independent implementations of one
// grammar in this tree, and like the other two it was wrong in the permissive
// direction. `modechange` is that grammar written once, checked against 24,480
// rows generated from GNU coreutils 9.4; see design-decisions.md 364 and
// known-issues.md TD-B-THREE-UTILITIES-STILL-CARRY-THEIR-OWN-MODE-PARSER.
//
// What the deleted parser got wrong, all of it now fixed by construction:
//
//   * `b'x' | b'X' => x = true` -- `X` is not `x`. `X` sets an execute bit only
//     on a directory or on a file that already has one, which is the entire
//     point of `chmod -R a+rX` on a source tree: it makes directories traversable
//     without making every `.c` file executable. The old code made them all
//     executable.
//   * The umask was never consulted, so `chmod +w f` granted write to group and
//     other. GNU masks a clause that names no `who`: under `umask 022`,
//     `chmod +w` is `u+w`. `chmod a+w` is unaffected, which is how a caller asks
//     for the broad grant explicitly.
//   * `if part.is_empty() { continue; }` accepted `,` and `u+r,` as valid.
//   * Only one operator per clause: `u+r-w` was rejected, and `=u` (copy the
//     user triad to another) was not implemented at all.
//   * `chmod 0 f` was rejected -- `strip_prefix('0')` left an empty string, which
//     fell through to the symbolic parser and died on a missing operator.
//   * `-R` stripped setgid off directories. An octal mode is applied verbatim
//     here, but gnulib protects setuid and setgid on a *directory* from any
//     change that did not name them, so `chmod -R 755 d` leaves a setgid
//     directory setgid. This is the one that silently changed the meaning of a
//     shared group tree.

use modechange::{Changes, adjust, compile, from_reference};

/// The file-mode creation mask.
///
/// POSIX has no read-only spelling of it -- reading it means setting it -- so
/// this is the libc call rather than anything in `std`.
#[cfg(unix)]
unsafe extern "C" {
    fn umask(mask: u32) -> u32;
}

/// Read the process umask, restoring it immediately.
///
/// Cached because `umask(0)` *writes* as well as reads: a second uncached call
/// would read back the zero the first one wrote. `chmod` reads it once per run,
/// but the cache makes that a property of the function rather than of the call
/// site.
#[cfg(unix)]
fn read_umask() -> u32 {
    use std::sync::OnceLock;
    static UMASK: OnceLock<u32> = OnceLock::new();
    *UMASK.get_or_init(|| {
        // SAFETY: `umask` is a POSIX call that cannot fail and touches only this
        // process's file-mode creation mask. The second call restores what the
        // first read, so no other thread observes a zero mask for longer than
        // these two instructions.
        unsafe {
            let previous = umask(0);
            umask(previous);
            previous
        }
    })
}

/// The build host is Windows, which has no umask. Zero means "mask nothing",
/// so a who-less clause is taken at its word there.
#[cfg(not(unix))]
fn read_umask() -> u32 {
    0
}

/// A compiled mode, plus the umask to apply to any clause of it that named no
/// `who`.
///
/// The two travel together because they are only meaningful together, and
/// because `--reference` supplies a change list that must *not* be masked: it
/// copies bits that already exist on a real file, and a umask has no business
/// filtering them. That is why the umask is a field rather than read at the
/// point of use.
struct ModeSpec {
    changes: Changes,
    umask_value: u32,
}

impl ModeSpec {
    /// The mode `path` should end up with, given what it has now.
    fn resolve(&self, old_mode: u32, is_dir: bool) -> u32 {
        adjust(old_mode, is_dir, self.umask_value, &self.changes).mode
    }
}

// ============================================================================
// chown ownership spec parsing
// ============================================================================

/// Parsed ownership specification from `OWNER[:GROUP]` or `:GROUP`.
struct OwnerSpec {
    /// New owner UID, or `None` to leave unchanged.
    uid: Option<u32>,
    /// New group GID, or `None` to leave unchanged.
    gid: Option<u32>,
}

/// Parse an ownership string like `root`, `root:admin`, `:users`, `1000:100`.
fn parse_owner_spec(
    spec: &str,
    users: &UserDb,
    groups: &[GroupEntry],
) -> Result<OwnerSpec, String> {
    if let Some(group_name) = spec.strip_prefix(':') {
        // `:GROUP` -- change group only
        let gid = resolve_gid(group_name, groups)
            .ok_or_else(|| format!("unknown group: '{group_name}'"))?;
        return Ok(OwnerSpec {
            uid: None,
            gid: Some(gid),
        });
    }

    if let Some(colon_pos) = spec.find(':') {
        // `OWNER:GROUP`
        let owner_str = &spec[..colon_pos];
        let group_str = &spec[colon_pos + 1..];

        let uid =
            resolve_uid(owner_str, users).ok_or_else(|| format!("unknown user: '{owner_str}'"))?;

        let gid = if group_str.is_empty() {
            // `OWNER:` -- set group to the owner's primary group
            users
                .find_uid(uid)
                .and_then(|u| u.groups().first().and_then(|g| resolve_gid(g, groups)))
        } else {
            Some(
                resolve_gid(group_str, groups)
                    .ok_or_else(|| format!("unknown group: '{group_str}'"))?,
            )
        };

        return Ok(OwnerSpec {
            uid: Some(uid),
            gid,
        });
    }

    // Plain `OWNER` -- change owner only
    let uid = resolve_uid(spec, users).ok_or_else(|| format!("unknown user: '{spec}'"))?;
    Ok(OwnerSpec {
        uid: Some(uid),
        gid: None,
    })
}

/// Parse a `--from=CURRENT_OWNER:CURRENT_GROUP` filter. Either side may be
/// empty to mean "don't check".
fn parse_from_filter(
    spec: &str,
    users: &UserDb,
    groups: &[GroupEntry],
) -> Result<(Option<u32>, Option<u32>), String> {
    if let Some(colon_pos) = spec.find(':') {
        let owner_str = &spec[..colon_pos];
        let group_str = &spec[colon_pos + 1..];

        let uid = if owner_str.is_empty() {
            None
        } else {
            Some(
                resolve_uid(owner_str, users)
                    .ok_or_else(|| format!("unknown user in --from: '{owner_str}'"))?,
            )
        };

        let gid = if group_str.is_empty() {
            None
        } else {
            Some(
                resolve_gid(group_str, groups)
                    .ok_or_else(|| format!("unknown group in --from: '{group_str}'"))?,
            )
        };

        Ok((uid, gid))
    } else {
        // Just an owner, no group filter.
        let uid =
            resolve_uid(spec, users).ok_or_else(|| format!("unknown user in --from: '{spec}'"))?;
        Ok((Some(uid), None))
    }
}

// ============================================================================
// JSON output helpers
// ============================================================================

/// Escape a string for JSON output (handles quotes and backslashes).
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// Print a JSON change record for chown.
fn print_chown_json(path: &str, uid: Option<u32>, gid: Option<u32>, ok: bool, err: &str) {
    let uid_str = match uid {
        Some(u) => format!("{u}"),
        None => "null".to_string(),
    };
    let gid_str = match gid {
        Some(g) => format!("{g}"),
        None => "null".to_string(),
    };
    println!(
        "{{\"path\":\"{}\",\"uid\":{},\"gid\":{},\"ok\":{},\"error\":\"{}\"}}",
        json_escape(path),
        uid_str,
        gid_str,
        ok,
        json_escape(err),
    );
}

/// Print a JSON change record for chmod.
fn print_chmod_json(path: &str, mode: u32, ok: bool, err: &str) {
    println!(
        "{{\"path\":\"{}\",\"mode\":\"{:04o}\",\"ok\":{},\"error\":\"{}\"}}",
        json_escape(path),
        mode,
        ok,
        json_escape(err),
    );
}

// ============================================================================
// Argument parsing
// ============================================================================

/// Which binary personality we are running as.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Chown,
    Chmod,
}

/// Parsed command-line options (shared between chown and chmod).
struct Options {
    mode: Mode,
    /// -R / --recursive
    recursive: bool,
    /// -v / --verbose (report every file)
    verbose: bool,
    /// -c / --changes (report only actual changes)
    changes: bool,
    /// -f / --silent (suppress errors)
    silent: bool,
    /// -h / --no-dereference (affect symlink, not target)
    no_deref: bool,
    /// --json output
    json: bool,
    /// --from=OWNER:GROUP filter (chown only)
    from_uid: Option<u32>,
    from_gid: Option<u32>,
    /// --reference=FILE
    reference: Option<String>,
    /// The ownership spec string (chown) or mode string (chmod).
    spec: String,
    /// Target files.
    files: Vec<String>,
}

/// Detect whether argv[0] ends in "chmod".
fn detect_mode(argv0: &str) -> Mode {
    let basename = argv0
        .rsplit('/')
        .next()
        .unwrap_or(argv0)
        .rsplit('\\')
        .next()
        .unwrap_or(argv0);
    if basename == "chmod" || basename.starts_with("chmod.") {
        Mode::Chmod
    } else {
        Mode::Chown
    }
}

fn parse_args(args: &[String], users: &UserDb, groups: &[GroupEntry]) -> Result<Options, String> {
    if args.is_empty() {
        return Err("no arguments provided".to_string());
    }

    let mode = detect_mode(&args[0]);

    let mut opts = Options {
        mode,
        recursive: false,
        verbose: false,
        changes: false,
        silent: false,
        no_deref: false,
        json: false,
        from_uid: None,
        from_gid: None,
        reference: None,
        spec: String::new(),
        files: Vec::new(),
    };

    let mut i = 1;
    let mut found_spec = false;

    while i < args.len() {
        let arg = &args[i];

        // End-of-options marker.
        if arg == "--" {
            i += 1;
            break;
        }

        if arg == "--help" {
            return Err(String::new());
        }

        if arg == "-R" || arg == "--recursive" {
            opts.recursive = true;
            i += 1;
            continue;
        }

        if arg == "-v" || arg == "--verbose" {
            opts.verbose = true;
            i += 1;
            continue;
        }

        if arg == "-c" || arg == "--changes" {
            opts.changes = true;
            i += 1;
            continue;
        }

        if arg == "-f" || arg == "--silent" || arg == "--quiet" {
            opts.silent = true;
            i += 1;
            continue;
        }

        if arg == "--json" {
            opts.json = true;
            i += 1;
            continue;
        }

        if (arg == "-h" || arg == "--no-dereference") && mode == Mode::Chown {
            opts.no_deref = true;
            i += 1;
            continue;
        }

        // --from=OWNER:GROUP (chown only)
        if let Some(from_val) = arg.strip_prefix("--from=") {
            if mode != Mode::Chown {
                return Err("--from is only valid in chown mode".to_string());
            }
            let (fuid, fgid) = parse_from_filter(from_val, users, groups)?;
            opts.from_uid = fuid;
            opts.from_gid = fgid;
            i += 1;
            continue;
        }

        // --reference=FILE
        if let Some(ref_val) = arg.strip_prefix("--reference=") {
            opts.reference = Some(ref_val.to_string());
            i += 1;
            continue;
        }

        // The first non-flag argument is the spec (unless --reference is given,
        // in which case all non-flag args are files).
        if !found_spec && opts.reference.is_none() && !arg.starts_with('-') {
            opts.spec = arg.clone();
            found_spec = true;
            i += 1;
            continue;
        }

        // Everything else is a file.
        opts.files.push(arg.clone());
        i += 1;
    }

    // Remaining args after `--` are files.
    while i < args.len() {
        opts.files.push(args[i].clone());
        i += 1;
    }

    // Validate: need at least one file.
    if opts.files.is_empty() {
        return Err("missing file operand".to_string());
    }

    // When --reference is used, no spec is needed.
    if opts.reference.is_none() && opts.spec.is_empty() {
        let what = if mode == Mode::Chown { "owner" } else { "mode" };
        return Err(format!("missing {what} operand"));
    }

    Ok(opts)
}

// ============================================================================
// chown execution
// ============================================================================

/// Run chown on a single file. Returns (changed: bool, error: Option<String>).
///
/// `follow` decides whether a symbolic link at `path` is dereferenced. It is
/// never inferred here: the caller knows whether this path is an operand the
/// user named or a name the filesystem handed us, and only the caller can tell
/// those apart. See [`follow_operand`].
fn chown_one(path: &str, spec: &OwnerSpec, opts: &Options, follow: bool) -> (bool, Option<String>) {
    // Read current metadata (best-effort) for --from matching and accurate
    // change detection. If it fails we fall back to assuming a field changes
    // whenever it is specified.
    let current = read_metadata(path).ok();

    // --from filter: only operate on files whose current owner/group match.
    if opts.from_uid.is_some() || opts.from_gid.is_some() {
        match &current {
            Some(meta) => {
                let uid_match = opts.from_uid.is_none_or(|u| u == meta.uid);
                let gid_match = opts.from_gid.is_none_or(|g| g == meta.gid);
                if !uid_match || !gid_match {
                    // Current ownership does not match the filter: skip.
                    return (false, None);
                }
            }
            None => {
                // Cannot verify the current ownership, so we cannot safely
                // honor --from. Skip rather than risk an unwanted change.
                if !opts.silent {
                    eprintln!(
                        "chown: cannot verify current ownership of {} for --from; skipping",
                        quoteaf_os(path)
                    );
                }
                return (false, None);
            }
        }
    }

    // Determine whether this call will actually change anything.
    let changed = match &current {
        Some(meta) => {
            let uid_changes = spec.uid.is_some_and(|u| u != meta.uid);
            let gid_changes = spec.gid.is_some_and(|g| g != meta.gid);
            uid_changes || gid_changes
        }
        None => spec.uid.is_some() || spec.gid.is_some(),
    };

    // "No change" sentinel for syscall.
    let uid = spec.uid.unwrap_or(u32::MAX);
    let gid = spec.gid.unwrap_or(u32::MAX);

    match do_chown(path, uid, gid, !follow) {
        Ok(()) => {
            let owner_str = format_owner(spec.uid, spec.gid);
            if opts.json {
                print_chown_json(path, spec.uid, spec.gid, true, "");
            } else if opts.verbose {
                if changed {
                    eprintln!("changed ownership of {} to {owner_str}", quoteaf_os(path));
                } else {
                    eprintln!("ownership of {} retained as {owner_str}", quoteaf_os(path));
                }
            } else if opts.changes && changed {
                eprintln!("changed ownership of {} to {owner_str}", quoteaf_os(path));
            }
            (changed, None)
        }
        Err(e) => {
            if opts.json {
                print_chown_json(path, spec.uid, spec.gid, false, &e);
            } else if !opts.silent {
                eprintln!(
                    "chown: cannot change ownership of {}: {e}",
                    quoteaf_os(path)
                );
            }
            (false, Some(e))
        }
    }
}

fn format_owner(uid: Option<u32>, gid: Option<u32>) -> String {
    match (uid, gid) {
        (Some(u), Some(g)) => format!("{u}:{g}"),
        (Some(u), None) => format!("{u}"),
        (None, Some(g)) => format!(":{g}"),
        (None, None) => "(unchanged)".to_string(),
    }
}

/// Execute chown for all target files.
fn run_chown(opts: &Options, users: &UserDb, groups: &[GroupEntry]) -> bool {
    let spec = if let Some(ref refpath) = opts.reference {
        // --reference: copy owner/group from the reference file's metadata.
        match read_metadata(refpath) {
            Ok(meta) => OwnerSpec {
                uid: Some(meta.uid),
                gid: Some(meta.gid),
            },
            Err(e) => {
                if !opts.silent {
                    eprintln!("chown: cannot read reference {}: {e}", quoteaf_os(refpath));
                }
                return false;
            }
        }
    } else {
        match parse_owner_spec(&opts.spec, users, groups) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("chown: {e}");
                return false;
            }
        }
    };

    let mut any_error = false;
    let operand_follow = follow_operand(opts.recursive, opts.no_deref);

    for file in &opts.files {
        let p = Path::new(file);

        // `p.is_dir()` would follow a symlink and start the walk in whatever
        // tree it names. `symlink_metadata` is lstat: a link is a link, so
        // `chown -R alice link-to-etc` changes the link and stops there.
        let operand_is_walkable_dir =
            opts.recursive && fs::symlink_metadata(p).is_ok_and(|m| m.is_dir());

        if operand_is_walkable_dir {
            for entry in collect_recursive(p) {
                let path_str = entry.path.to_string_lossy();
                // --from filtering is handled inside chown_one, which has
                // access to the file's current metadata.
                let follow = if entry.is_symlink {
                    follow_child()
                } else {
                    // Not a link, so following is a no-op — but ask for
                    // NO_FOLLOW anyway rather than leave a race in which the
                    // name becomes one between the walk and the syscall.
                    false
                };
                let (_, err) = chown_one(&path_str, &spec, opts, follow);
                if err.is_some() {
                    any_error = true;
                }
            }
        } else {
            let (_, err) = chown_one(&p.to_string_lossy(), &spec, opts, operand_follow);
            if err.is_some() {
                any_error = true;
            }
        }
    }

    !any_error
}

// ============================================================================
// chmod execution
// ============================================================================

/// Run chmod on a single file. Returns (changed: bool, error: Option<String>).
///
/// `old_mode` is the file's current permission bits if known (used for change
/// detection); pass `None` when the current mode could not be read.
fn chmod_one(
    path: &str,
    mode_val: u32,
    old_mode: Option<u32>,
    opts: &Options,
) -> (bool, Option<String>) {
    let changed = match old_mode {
        Some(old) => (old & 0o7777) != (mode_val & 0o7777),
        None => true,
    };

    match do_chmod(path, mode_val) {
        Ok(()) => {
            if opts.json {
                print_chmod_json(path, mode_val, true, "");
            } else if opts.verbose {
                if changed {
                    eprintln!(
                        "mode of {} changed to {:04o}",
                        quoteaf_os(path),
                        mode_val & 0o7777
                    );
                } else {
                    eprintln!(
                        "mode of {} retained as {:04o}",
                        quoteaf_os(path),
                        mode_val & 0o7777
                    );
                }
            } else if opts.changes && changed {
                eprintln!(
                    "mode of {} changed to {:04o}",
                    quoteaf_os(path),
                    mode_val & 0o7777
                );
            }
            (changed, None)
        }
        Err(e) => {
            if opts.json {
                print_chmod_json(path, mode_val, false, &e);
            } else if !opts.silent {
                eprintln!("chmod: cannot change mode of {}: {e}", quoteaf_os(path));
            }
            (false, Some(e))
        }
    }
}

/// Execute chmod for all target files.
fn run_chmod(opts: &Options) -> bool {
    // Parse the mode spec. --reference builds the change list from the
    // reference file's bits instead of from a string.
    let mode_spec = if let Some(ref refpath) = opts.reference {
        match read_metadata(refpath) {
            // Umask 0: `--reference` copies bits off a file that already has
            // them, and masking those would filter the answer to a question
            // nobody asked.
            Ok(meta) => ModeSpec {
                changes: from_reference(meta.perms & modechange::CHMOD_MODE_BITS),
                umask_value: 0,
            },
            Err(e) => {
                if !opts.silent {
                    eprintln!("chmod: cannot read reference {}: {e}", quoteaf_os(refpath));
                }
                return false;
            }
        }
    } else {
        match compile(opts.spec.as_bytes()) {
            Some(changes) => ModeSpec {
                changes,
                umask_value: read_umask(),
            },
            // GNU's wording, and deliberately one message for every way the
            // grammar can be broken: the user does not care which rule they
            // tripped, only that the string is not a mode.
            None => {
                eprintln!("chmod: invalid mode: \u{2018}{}\u{2019}", opts.spec);
                return false;
            }
        }
    };

    let mut any_error = false;

    for file in &opts.files {
        // A command-line symlink *is* dereferenced here, matching GNU chmod
        // (`fts` with `FTS_COMFOLLOW`): the operand is a name the caller typed
        // and can see. Links met during the walk below are a different matter
        // and are skipped outright.
        let paths: Vec<WalkEntry> = if opts.recursive {
            let p = Path::new(file);
            if p.is_dir() {
                collect_recursive(p)
            } else {
                vec![WalkEntry {
                    path: p.to_path_buf(),
                    is_symlink: false,
                }]
            }
        } else {
            vec![WalkEntry {
                path: PathBuf::from(file),
                is_symlink: false,
            }]
        };

        for entry in &paths {
            // GNU chmod skips every symbolic link it meets while recursing,
            // and so do we. A symlink has no useful mode bits of its own, so
            // the only thing chmod on one can do is change its target's — and
            // `srv/x -> /etc/shadow` would make `chmod -R 777 srv/` a way to
            // make /etc/shadow world-writable.
            if entry.is_symlink {
                continue;
            }
            let path_str = entry.path.to_string_lossy();

            // Read the current mode (best-effort) for symbolic application and
            // change detection.
            let current_mode = read_metadata(&path_str)
                .ok()
                .map(|m| m.perms & modechange::CHMOD_MODE_BITS);

            // Even an octal mode is resolved against the current one now,
            // because `dir` is not decoration: gnulib protects setuid and
            // setgid on a directory from a change that did not name them, so
            // `chmod -R 755 d` leaves a setgid directory setgid. The old code
            // applied an octal verbatim and stripped it.
            //
            // If the current mode is unknown, 0 is the base: `+` and `=` still
            // land where they should, and `-` on an unset bit is a no-op.
            let base = current_mode.unwrap_or(0);
            let mode_val = mode_spec.resolve(base, entry.path.is_dir());

            let (_, err) = chmod_one(&path_str, mode_val, current_mode, opts);
            if err.is_some() {
                any_error = true;
            }
        }
    }

    !any_error
}

// ============================================================================
// Help text
// ============================================================================

fn print_chown_help() {
    println!("Slate OS chown v0.1.0 -- Change file owner and group");
    println!();
    println!("USAGE:");
    println!("  chown [OPTIONS] OWNER[:GROUP] FILE...");
    println!("  chown [OPTIONS] :GROUP FILE...");
    println!("  chown [OPTIONS] --reference=REF FILE...");
    println!();
    println!("OPTIONS:");
    println!("  -R, --recursive          Operate recursively on directories");
    println!("  -v, --verbose            Report every file processed");
    println!("  -c, --changes            Report only files with actual changes");
    println!("  -f, --silent, --quiet    Suppress error messages");
    println!("  -h, --no-dereference     Change symlink itself, not its target");
    println!("  --from=CUR_OWNER:CUR_GRP Only change if current owner/group match");
    println!("  --reference=FILE         Use owner/group of FILE");
    println!("  --json                   JSON output");
    println!("  --help                   Show this help");
    println!();
    println!("OWNER and GROUP may be names (from /etc/users.yaml) or numeric IDs.");
    println!();
    println!("EXAMPLES:");
    println!("  chown root:admin /etc/config.yaml");
    println!("  chown -R www:www /var/www");
    println!("  chown :users myfile.txt");
    println!("  chown --from=root:root alice:staff /shared/*");
}

fn print_chmod_help() {
    println!("Slate OS chmod v0.1.0 -- Change file permissions");
    println!();
    println!("USAGE:");
    println!("  chmod [OPTIONS] MODE FILE...");
    println!("  chmod [OPTIONS] --reference=REF FILE...");
    println!();
    println!("MODE FORMATS:");
    println!("  Octal:    755, 644, 0777");
    println!("  Symbolic: u+x, g-w, o+r, a+rx, u=rwx,g=rx,o=r");
    println!();
    println!("  Classes: u=user  g=group  o=other  a=all");
    println!("  Ops:     + add   - remove   = set exactly");
    println!("  Perms:   r=read  w=write  x=execute  s=setuid/gid  t=sticky");
    println!();
    println!("OPTIONS:");
    println!("  -R, --recursive          Operate recursively on directories");
    println!("  -v, --verbose            Report every file processed");
    println!("  -c, --changes            Report only files with actual changes");
    println!("  -f, --silent, --quiet    Suppress error messages");
    println!("  --reference=FILE         Use permissions of FILE");
    println!("  --json                   JSON output");
    println!("  --help                   Show this help");
    println!();
    println!("EXAMPLES:");
    println!("  chmod 755 script.sh");
    println!("  chmod u+x,g+x script.sh");
    println!("  chmod -R a+rX /var/www");
    println!("  chmod 4755 /usr/bin/setuid_prog");
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let binary_mode = args.first().map(|a| detect_mode(a)).unwrap_or(Mode::Chown);

    // Load the user database for name resolution (chown needs this; chmod
    // does not, but loading is cheap and keeps the code path simple).
    let users = read_users();
    let groups = build_group_table(&users);

    let opts = match parse_args(&args, &users, &groups) {
        Ok(o) => o,
        Err(msg) => {
            if msg.is_empty() {
                match binary_mode {
                    Mode::Chown => print_chown_help(),
                    Mode::Chmod => print_chmod_help(),
                }
                process::exit(0);
            }
            let name = if binary_mode == Mode::Chown {
                "chown"
            } else {
                "chmod"
            };
            eprintln!("{name}: {msg}");
            eprintln!("Try '{name} --help' for usage information.");
            process::exit(1);
        }
    };

    let success = match opts.mode {
        Mode::Chown => run_chown(&opts, &users, &groups),
        Mode::Chmod => run_chmod(&opts),
    };

    if !success {
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// A database in the form `useradm` writes it.
    ///
    /// Written as text rather than built from setters so that the parser this
    /// crate now shares is exercised on the same bytes the writer produces —
    /// the hand-rolled parser it replaces read `groups:` correctly but was
    /// never fed a file that any writer had actually emitted.
    fn sample_users() -> UserDb {
        UserDb::parse(
            "users:\n\
             \x20 - uid: 0\n\
             \x20   username: \"root\"\n\
             \x20   groups: [\"root\", \"admin\"]\n\
             \x20 - uid: 1000\n\
             \x20   username: \"alice\"\n\
             \x20   groups: [\"users\", \"staff\"]\n",
        )
    }

    // ---- symlink policy ----------------------------------------------------
    //
    // These are the only tests in this file that guard a security boundary, so
    // they are stated as the rule rather than as the code: if one of them goes
    // red, the question to ask is whether the *rule* changed, not whether the
    // assertion needs updating. See known-issues.md →
    // `B-chown-FOLLOWS-SYMLINKS-WHILE-RECURSING`.

    #[test]
    fn plain_chown_of_a_link_follows_it() {
        // POSIX: without -R and without -h, chown acts on the target. The
        // caller named this link and can see what it is.
        assert!(follow_operand(false, false));
    }

    #[test]
    fn dash_h_never_follows() {
        assert!(!follow_operand(false, true));
        assert!(!follow_operand(true, true));
    }

    #[test]
    fn recursive_chown_does_not_follow_its_own_operand() {
        // -P is POSIX's default for `chown -R`. `chown -R alice link-to-etc`
        // must change the link, not walk /etc.
        assert!(!follow_operand(true, false));
    }

    #[test]
    fn nothing_found_during_a_walk_is_ever_followed() {
        // This binary implements neither -H nor -L, so there is no flag that
        // could ask to follow, and following is the escape itself.
        assert!(!follow_child());
    }

    // ---- mode detection ----------------------------------------------------

    #[test]
    fn detect_mode_recognizes_chmod() {
        assert_eq!(detect_mode("chmod"), Mode::Chmod);
        assert_eq!(detect_mode("/usr/bin/chmod"), Mode::Chmod);
        assert_eq!(detect_mode("C:\\bin\\chmod.exe"), Mode::Chmod);
        assert_eq!(detect_mode("chmod.exe"), Mode::Chmod);
    }

    #[test]
    fn detect_mode_defaults_to_chown() {
        assert_eq!(detect_mode("chown"), Mode::Chown);
        assert_eq!(detect_mode("/usr/bin/chown"), Mode::Chown);
        assert_eq!(detect_mode("anything-else"), Mode::Chown);
    }

    // ---- mode parsing -------------------------------------------------------
    //
    // Every row below was measured against GNU coreutils 9.4 under WSL. The
    // grammar itself is `modechange`'s, checked there against 24,480 generated
    // rows; what these tests pin is *this* binary's use of it -- the umask it
    // passes, the `dir` flag it passes, and the base mode it starts from. Those
    // three are the caller's decisions, and all three were wrong before.

    /// Resolve a spec the way `run_chmod` does, with an explicit umask.
    ///
    /// The umask has to be a parameter rather than read from the process: the
    /// build host is Windows and has none, so a test that relied on the real
    /// one would assert nothing here and something different on every
    /// developer's machine.
    fn resolve(spec: &str, old: u32, is_dir: bool, umask_value: u32) -> u32 {
        let changes = compile(spec.as_bytes()).unwrap_or_else(|| panic!("GNU accepts {spec:?}"));
        ModeSpec {
            changes,
            umask_value,
        }
        .resolve(old, is_dir)
    }

    #[test]
    fn an_octal_mode_is_the_mode_it_spells() {
        assert_eq!(resolve("755", 0o000, false, 0), 0o755);
        assert_eq!(resolve("0644", 0o777, false, 0), 0o644);
        assert_eq!(resolve("00755", 0o000, false, 0), 0o755);
        assert_eq!(resolve("4755", 0o000, false, 0), 0o4755);
        // `chmod 0 f` -- the old parser stripped the leading zero, was left with
        // an empty string, fell through to the symbolic branch and died on a
        // missing operator. GNU sets the mode to 0.
        assert_eq!(resolve("0", 0o777, false, 0), 0);
        assert_eq!(resolve("=", 0o777, false, 0), 0);
    }

    /// An octal is never masked, however the umask is set.
    #[test]
    fn an_octal_mode_ignores_the_umask() {
        for umask_value in [0o000, 0o022, 0o077, 0o002] {
            assert_eq!(resolve("755", 0o000, false, umask_value), 0o755);
        }
    }

    /// A clause that names no `who` is masked by the umask; one that names a
    /// `who` is not.
    ///
    /// The old parser never read the umask at all, so `chmod +w f` granted
    /// write to group and other -- a silent broadening of exactly the kind this
    /// utility exists to prevent. Measured, from mode `000`: `+w` is `0222`
    /// under umask 000, `0200` under 022 **and** 077, `0220` under 002, while
    /// `a+w` is `0222` under all four.
    #[test]
    fn a_clause_with_no_who_is_masked_by_the_umask() {
        for (umask_value, want) in [
            (0o000, 0o222),
            (0o022, 0o200),
            (0o077, 0o200),
            (0o002, 0o220),
        ] {
            assert_eq!(resolve("+w", 0o000, false, umask_value), want);
            assert_eq!(resolve("a+w", 0o000, false, umask_value), 0o222);
        }
        // Execute bits are not in any of these umasks, so `+x` from 644 is 755
        // under three of them and 744 under 077.
        assert_eq!(resolve("+x", 0o644, false, 0o022), 0o755);
        assert_eq!(resolve("+x", 0o644, false, 0o077), 0o744);
        // Removal is masked too: `-w` from 666 keeps the bits the umask held.
        assert_eq!(resolve("-w", 0o666, false, 0o022), 0o466);
        assert_eq!(resolve("-w", 0o666, false, 0o002), 0o446);
    }

    /// `X` sets an execute bit only where one is already earned.
    ///
    /// The old parser matched `b'x' | b'X'`, which is what makes
    /// `chmod -R a+rX src/` -- the standard way to make a tree readable --
    /// mark every source file executable. `X` fires on a directory, or on a
    /// file that already has some execute bit, and on nothing else.
    #[test]
    fn capital_x_is_not_x() {
        assert_eq!(resolve("a+rX", 0o644, false, 0), 0o644);
        assert_eq!(resolve("a+rX", 0o700, false, 0), 0o755);
        assert_eq!(resolve("a+rX", 0o700, true, 0), 0o755);
        // A directory earns it whatever its own bits say.
        assert_eq!(resolve("a=,+X", 0o000, true, 0), 0o111);
        assert_eq!(resolve("a=,+X", 0o000, false, 0), 0o000);
    }

    /// A directory keeps setuid and setgid through a change that did not name
    /// them; a file does not.
    ///
    /// This is the one that quietly broke shared group trees. The old code
    /// applied an octal verbatim, so `chmod -R 755 d` on a setgid directory
    /// cleared the bit and every file created there afterwards landed in the
    /// creator's own group instead of the project's. Measured: a `2775`
    /// directory under `chmod -R 755` comes out `2755`, and a `6755` one under
    /// `chmod -R 700` comes out `6700`, while a `4755` *file* under
    /// `chmod 755` comes out `755`.
    #[test]
    fn a_directory_keeps_setgid_through_an_unrelated_change() {
        assert_eq!(resolve("755", 0o2775, true, 0), 0o2755);
        assert_eq!(resolve("700", 0o6755, true, 0), 0o6700);
        assert_eq!(resolve("755", 0o4755, false, 0), 0o755);
        // Naming the bit still changes it, on a directory as much as on a file.
        assert_eq!(resolve("g-s", 0o2775, true, 0), 0o775);
        assert_eq!(resolve("2755", 0o0755, true, 0), 0o2755);
    }

    /// Several operators in one clause, and `=u` copying an existing triad.
    ///
    /// Neither was implemented: the old parser read exactly one operator per
    /// comma-separated part and knew only `rwxstX` as permission letters, so
    /// `u+r-w` and `g=u` were both rejected outright.
    #[test]
    fn several_operators_in_one_clause_and_copying_a_triad() {
        assert_eq!(resolve("u+r-w", 0o000, false, 0), 0o400);
        assert_eq!(resolve("u+r-w", 0o777, false, 0), 0o577);
        assert_eq!(resolve("g=u", 0o750, false, 0), 0o770);
        assert_eq!(resolve("go=u", 0o700, false, 0), 0o777);
    }

    #[test]
    fn the_ordinary_symbolic_forms_still_work() {
        assert_eq!(resolve("u+x", 0o644, false, 0), 0o744);
        assert_eq!(resolve("go-w", 0o666, false, 0), 0o644);
        assert_eq!(resolve("a=rx", 0o777, false, 0), 0o555);
        assert_eq!(resolve("u=rwx,g=rx,o=r", 0o000, false, 0), 0o754);
        assert_eq!(resolve("u+s", 0o755, false, 0), 0o4755);
        assert_eq!(resolve("+t", 0o755, false, 0), 0o1755);
        assert_eq!(resolve("u=r", 0o777, false, 0), 0o477);
    }

    /// The boundary between a mode GNU accepts and one it refuses.
    ///
    /// `+` is valid and means "add nothing"; `=` is valid and clears
    /// everything. The old parser refused `+`, and *accepted* `,` and `u+r,` by
    /// skipping empty parts -- so a typo that dropped a clause silently became
    /// a no-op instead of an error.
    #[test]
    fn the_boundary_between_a_valid_and_an_invalid_mode() {
        for spec in ["+", "=", "0", "0777", "00755", "u+r-w", "g=u"] {
            assert!(compile(spec.as_bytes()).is_some(), "GNU accepts {spec:?}");
        }
        for spec in [",", "u+r,", "", "u", "u+z", "8", "77777", "a", "ugo"] {
            assert!(compile(spec.as_bytes()).is_none(), "GNU refuses {spec:?}");
        }
        // `+` adds nothing, so it leaves the mode alone rather than zeroing it.
        assert_eq!(resolve("+", 0o644, false, 0o022), 0o644);
    }

    /// `--reference` copies the file's bits verbatim, umask and all.
    #[test]
    fn a_reference_mode_is_copied_not_masked() {
        let spec = ModeSpec {
            changes: from_reference(0o4711),
            umask_value: 0,
        };
        assert_eq!(spec.resolve(0o000, false), 0o4711);
        assert_eq!(spec.resolve(0o777, false), 0o4711);
        // Even on a directory: `from_reference` mentions every bit, so setuid
        // and setgid are copied from the reference rather than preserved from
        // the target.
        assert_eq!(spec.resolve(0o2755, true), 0o4711);
    }

    /// GNU's wording, which this binary did not have: one message for every
    /// broken rule, with a colon and curly quotes.
    #[test]
    fn the_invalid_mode_diagnostic_matches_gnu() {
        let rendered = format!("chmod: invalid mode: \u{2018}{}\u{2019}", "u+z");
        assert_eq!(rendered, "chmod: invalid mode: \u{2018}u+z\u{2019}");
    }

    // ---- owner spec parsing ------------------------------------------------

    #[test]
    fn owner_spec_user_only() {
        let users = sample_users();
        let groups = build_group_table(&users);
        let spec = parse_owner_spec("alice", &users, &groups).unwrap();
        assert_eq!(spec.uid, Some(1000));
        assert_eq!(spec.gid, None);
    }

    #[test]
    fn owner_spec_user_and_group() {
        let users = sample_users();
        let groups = build_group_table(&users);
        let spec = parse_owner_spec("root:admin", &users, &groups).unwrap();
        assert_eq!(spec.uid, Some(0));
        assert_eq!(spec.gid, Some(1)); // admin = gid 1
    }

    #[test]
    fn owner_spec_group_only() {
        let users = sample_users();
        let groups = build_group_table(&users);
        let spec = parse_owner_spec(":users", &users, &groups).unwrap();
        assert_eq!(spec.uid, None);
        assert_eq!(spec.gid, Some(100)); // users = gid 100
    }

    #[test]
    fn owner_spec_numeric() {
        let users = sample_users();
        let groups = build_group_table(&users);
        let spec = parse_owner_spec("4242:99", &users, &groups).unwrap();
        assert_eq!(spec.uid, Some(4242));
        assert_eq!(spec.gid, Some(99));
    }

    #[test]
    fn owner_spec_trailing_colon_uses_primary_group() {
        let users = sample_users();
        let groups = build_group_table(&users);
        // alice's primary (first) group is "users" = gid 100.
        let spec = parse_owner_spec("alice:", &users, &groups).unwrap();
        assert_eq!(spec.uid, Some(1000));
        assert_eq!(spec.gid, Some(100));
    }

    #[test]
    fn owner_spec_unknown_user_errors() {
        let users = sample_users();
        let groups = build_group_table(&users);
        assert!(parse_owner_spec("nobody", &users, &groups).is_err());
    }

    #[test]
    fn owner_spec_unknown_group_errors() {
        let users = sample_users();
        let groups = build_group_table(&users);
        assert!(parse_owner_spec(":nogroup", &users, &groups).is_err());
    }

    // ---- group table / resolution -----------------------------------------

    #[test]
    fn group_table_well_known_ids() {
        let users = sample_users();
        let groups = build_group_table(&users);
        assert_eq!(resolve_gid("root", &groups), Some(0));
        assert_eq!(resolve_gid("admin", &groups), Some(1));
        assert_eq!(resolve_gid("users", &groups), Some(100));
    }

    #[test]
    fn group_table_assigns_new_ids_from_101() {
        let users = sample_users();
        let groups = build_group_table(&users);
        // "staff" is the only non-well-known group; gets first free id 101.
        assert_eq!(resolve_gid("staff", &groups), Some(101));
    }

    #[test]
    fn resolve_uid_numeric_and_name() {
        let users = sample_users();
        assert_eq!(resolve_uid("alice", &users), Some(1000));
        assert_eq!(resolve_uid("0", &users), Some(0));
        assert_eq!(resolve_uid("7777", &users), Some(7777));
        assert_eq!(resolve_uid("ghost", &users), None);
    }

    #[test]
    fn an_administrator_is_a_member_of_wheel() {
        // The database records administrator-ness as `is_admin: true` rather
        // than as a group, so `chgrp wheel` would otherwise fail with "unknown
        // group" on a machine that plainly has administrators.
        let users = UserDb::parse(
            "users:\n\
             \x20 - uid: 1000\n\
             \x20   username: \"alice\"\n\
             \x20   is_admin: true\n",
        );
        let groups = build_group_table(&users);
        assert!(resolve_gid("wheel", &groups).is_some());
    }

    #[test]
    fn names_resolve_through_a_database_the_writer_produced() {
        // The migration's whole point: this crate reads what `useradm` writes.
        // Serialising and re-parsing is the only step at which a reader and a
        // writer that disagree can be seen to disagree.
        let mut db = UserDb::new();
        let mut alice = userdb::Record::new();
        alice.set_uid(1000);
        alice.set(userdb::field::USERNAME, "alice");
        alice.set_groups(&["users".to_string(), "staff".to_string()]);
        db.push(alice);

        let reparsed = UserDb::parse(&db.to_text());
        assert_eq!(resolve_uid("alice", &reparsed), Some(1000));
        let groups = build_group_table(&reparsed);
        assert_eq!(resolve_gid("staff", &groups), Some(101));
    }

    // ---- --from filter parsing ---------------------------------------------

    #[test]
    fn from_filter_owner_and_group() {
        let users = sample_users();
        let groups = build_group_table(&users);
        let (u, g) = parse_from_filter("root:admin", &users, &groups).unwrap();
        assert_eq!(u, Some(0));
        assert_eq!(g, Some(1));
    }

    #[test]
    fn from_filter_owner_only() {
        let users = sample_users();
        let groups = build_group_table(&users);
        let (u, g) = parse_from_filter("alice", &users, &groups).unwrap();
        assert_eq!(u, Some(1000));
        assert_eq!(g, None);
    }

    #[test]
    fn from_filter_group_only() {
        let users = sample_users();
        let groups = build_group_table(&users);
        let (u, g) = parse_from_filter(":users", &users, &groups).unwrap();
        assert_eq!(u, None);
        assert_eq!(g, Some(100));
    }

    // ---- metadata buffer parsing -------------------------------------------

    #[test]
    fn metadata_buffer_parses_fields() {
        let mut buf = [0u8; FS_META_SIZE];
        buf[META_OFF_UID..META_OFF_UID + 4].copy_from_slice(&1000u32.to_le_bytes());
        buf[META_OFF_GID..META_OFF_GID + 4].copy_from_slice(&100u32.to_le_bytes());
        buf[META_OFF_PERMS..META_OFF_PERMS + 2].copy_from_slice(&0o755u16.to_le_bytes());

        let meta = parse_metadata_buffer(&buf).unwrap();
        assert_eq!(meta.uid, 1000);
        assert_eq!(meta.gid, 100);
        assert_eq!(meta.perms, 0o755);
    }

    #[test]
    fn metadata_buffer_too_small_returns_none() {
        let buf = [0u8; 8];
        assert!(parse_metadata_buffer(&buf).is_none());
    }

    // ---- error mapping -----------------------------------------------------

    #[test]
    fn kernel_error_known_codes() {
        assert!(kernel_error_to_string(-500).contains("no such file"));
        assert!(kernel_error_to_string(-400).contains("permission denied"));
        assert!(kernel_error_to_string(-2).contains("not supported"));
    }

    #[test]
    fn kernel_error_unknown_code() {
        assert_eq!(kernel_error_to_string(-9999), "error -9999");
    }

    // ---- formatting helpers ------------------------------------------------

    #[test]
    fn format_owner_variants() {
        assert_eq!(format_owner(Some(0), Some(1)), "0:1");
        assert_eq!(format_owner(Some(5), None), "5");
        assert_eq!(format_owner(None, Some(7)), ":7");
        assert_eq!(format_owner(None, None), "(unchanged)");
    }

    #[test]
    fn json_escape_handles_special_chars() {
        assert_eq!(json_escape("a\"b\\c"), "a\\\"b\\\\c");
        assert_eq!(json_escape("line\nbreak"), "line\\nbreak");
        assert_eq!(json_escape("plain"), "plain");
    }
}
