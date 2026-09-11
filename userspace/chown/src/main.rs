//! Slate OS File Ownership Utility
//!
//! Changes a file's owner and group.
//!
//! # It used to answer to `chmod` as well, and nobody could reach it
//!
//! `argv[0]` selected between a chown and a chmod personality. The chmod half
//! could never run: `userspace/coreutils/src/bin/chmod.rs` produces the
//! `chmod` executable, so whichever name this binary was installed under, the
//! real one won. It was 543 lines of finished, tested, unreachable code, and
//! `scripts/multicall-aliases.py` had it pinned as `chown:chmod`.
//!
//! The name went to coreutils rather than here, and not only because §1005
//! makes coreutils the one home. Its front end is better at the part that is
//! not shared: it treats `-r`, `-w` and `-x` as MODE LETTERS, so `chmod -r f`
//! removes read permission, where the parser here had no such rule and would
//! have taken `-r` as a filename and `f` as the mode. It also has
//! `--preserve-root`, and it carries paths as `OsString` rather than `String`.
//! The symbolic-mode grammar itself was never in either of them -- both call
//! `modechange` -- so nothing of the hard part was at stake.
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

use quoting::quoteaf_os;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use pwdb::Db;

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
// Only reached on the real OS — the host arm of the caller returns before it.
// It stays compiled (and unit-tested) on a development host rather than being
// `#[cfg]`-ed away, because the host is where its tests run.
#[cfg_attr(not(target_vendor = "slateos"), allow(dead_code))]
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
// Only reached on the real OS — the host arm of the caller returns before it.
// It stays compiled (and unit-tested) on a development host rather than being
// `#[cfg]`-ed away, because the host is where its tests run.
#[cfg_attr(not(target_vendor = "slateos"), allow(dead_code))]
const SYS_FS_SET_OWNER: u64 = 630;

/// Change file permission mode bits (`SYS_FS_SET_PERMS`).
///
/// arg0 = path pointer, arg1 = path length, arg2 = mode (low 12 bits used:
/// rwx + setuid/setgid/sticky).
// Only reached on the real OS — the host arm of the caller returns before it.
// It stays compiled (and unit-tested) on a development host rather than being
// `#[cfg]`-ed away, because the host is where its tests run.
#[cfg_attr(not(target_vendor = "slateos"), allow(dead_code))]
const SYS_FS_SET_PERMS: u64 = 631;

/// Size of the `SYS_FS_METADATA` output buffer, in bytes.
// Only reached on the real OS — the host arm of the caller returns before it.
// It stays compiled (and unit-tested) on a development host rather than being
// `#[cfg]`-ed away, because the host is where its tests run.
#[cfg_attr(not(target_vendor = "slateos"), allow(dead_code))]
const FS_META_SIZE: usize = 64;

/// Byte offset of the u32 uid field within the metadata buffer.
// Only reached on the real OS — the host arm of the caller returns before it.
// It stays compiled (and unit-tested) on a development host rather than being
// `#[cfg]`-ed away, because the host is where its tests run.
#[cfg_attr(not(target_vendor = "slateos"), allow(dead_code))]
const META_OFF_UID: usize = 48;
/// Byte offset of the u32 gid field within the metadata buffer.
// Only reached on the real OS — the host arm of the caller returns before it.
// It stays compiled (and unit-tested) on a development host rather than being
// `#[cfg]`-ed away, because the host is where its tests run.
#[cfg_attr(not(target_vendor = "slateos"), allow(dead_code))]
const META_OFF_GID: usize = 52;
/// Byte offset of the u16 permission-bits field within the metadata buffer.
// Only reached on the real OS — the host arm of the caller returns before it.
// It stays compiled (and unit-tested) on a development host rather than being
// `#[cfg]`-ed away, because the host is where its tests run.
#[cfg_attr(not(target_vendor = "slateos"), allow(dead_code))]
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
#[cfg(target_vendor = "slateos")]
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
#[cfg(target_vendor = "slateos")]
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
#[cfg(target_vendor = "slateos")]
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
// Only reached on the real OS — the host arm of the caller returns before it.
// It stays compiled (and unit-tested) on a development host rather than being
// `#[cfg]`-ed away, because the host is where its tests run.
#[cfg_attr(not(target_vendor = "slateos"), allow(dead_code))]
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

/// Read `/etc/passwd` and `/etc/group`.
///
/// An unreadable or absent file is an empty table, which is `pwdb`'s rule and
/// glibc's: names then cannot be resolved, so `chown alice f` fails with
/// "unknown user" rather than silently doing something else. That is the right
/// failure -- `chown`'s whole job is to name an owner, and guessing one would
/// change the file to an owner nobody asked for.
///
/// This used to read `/etc/users.yaml`, and to *invent* the group table:
/// `root`, `admin` and `users` were given fixed ids and every other group name
/// mentioned by any account was numbered from 101 in order of appearance. Those
/// numbers were not the system's. A `chgrp audio` would set a file's group to
/// whatever position `audio` happened to occupy, which no other program agreed
/// with and which changed when an account was added. `/etc/group` has the real
/// numbers, and since §353 it is generated from the same database, so there is
/// no longer a second answer to disagree with.
fn read_db() -> Db {
    Db::load()
}

/// Resolve a username to a UID.
fn resolve_uid(name: &str, db: &Db) -> Option<u32> {
    // Try numeric first.
    if let Ok(n) = name.parse::<u32>() {
        return Some(n);
    }
    db.user_by_name(name.as_bytes()).map(|u| u.uid)
}

/// Resolve a group name to a GID.
fn resolve_gid(name: &str, db: &Db) -> Option<u32> {
    // Try numeric first.
    if let Ok(n) = name.parse::<u32>() {
        return Some(n);
    }
    db.group_by_name(name.as_bytes()).map(|g| g.gid)
}

/// The group an account's files belong to.
///
/// `/etc/passwd`'s fourth column, which is what "the user's group" means to
/// every other program that reads the file. A record with no `gid` in the
/// database is generated into that column with its uid, so the
/// user-private-group convention is already applied by the time it is read
/// here and needs no second implementation.
fn primary_gid(uid: u32, db: &Db) -> Option<u32> {
    db.user_by_uid(uid).map(|u| u.gid)
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
// Only reached on the real OS — the host arm of the caller returns before it.
// It stays compiled (and unit-tested) on a development host rather than being
// `#[cfg]`-ed away, because the host is where its tests run.
#[cfg_attr(not(target_vendor = "slateos"), allow(dead_code))]
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
#[cfg(target_vendor = "slateos")]
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
#[cfg(not(target_vendor = "slateos"))]
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
#[cfg(target_vendor = "slateos")]
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

/// Host fallback so the crate compiles for tests on development hosts.
#[cfg(not(target_vendor = "slateos"))]
fn do_chown(_path: &str, _uid: u32, _gid: u32, _no_follow: bool) -> Result<(), String> {
    Err("chown syscall unavailable on this platform".to_string())
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
fn parse_owner_spec(spec: &str, db: &Db) -> Result<OwnerSpec, String> {
    if let Some(group_name) = spec.strip_prefix(':') {
        // `:GROUP` -- change group only
        let gid = resolve_gid(group_name, db)
            .ok_or_else(|| format!("unknown group: {}", quoteaf_os(group_name)))?;
        return Ok(OwnerSpec {
            uid: None,
            gid: Some(gid),
        });
    }

    if let Some(colon_pos) = spec.find(':') {
        // `OWNER:GROUP`
        let owner_str = &spec[..colon_pos];
        let group_str = &spec[colon_pos + 1..];

        let uid = resolve_uid(owner_str, db)
            .ok_or_else(|| format!("unknown user: {}", quoteaf_os(owner_str)))?;

        let gid = if group_str.is_empty() {
            // `OWNER:` -- set group to the owner's *primary* group, which is
            // the record's `gid`, or its uid when it has none: that is the
            // user-private-group convention, and it is what the generated
            // `/etc/passwd` puts in the gid column for such a record, so the
            // two cannot disagree.
            //
            // This used to be the first entry of the record's `groups` list,
            // which is the *supplementary* list. On an account `useradd` put
            // in `audio` and `video`, `chown alice:` would have set the group
            // to `audio` -- a group alice is merely a member of, not the one
            // her files belong to. It went unnoticed because nothing wrote
            // that list for accounts made by `useradd`; now that `useradd`
            // maintains it, the wrong answer would be the usual one.
            primary_gid(uid, db)
        } else {
            Some(
                resolve_gid(group_str, db)
                    .ok_or_else(|| format!("unknown group: {}", quoteaf_os(group_str)))?,
            )
        };

        return Ok(OwnerSpec {
            uid: Some(uid),
            gid,
        });
    }

    // Plain `OWNER` -- change owner only
    let uid = resolve_uid(spec, db).ok_or_else(|| format!("unknown user: {}", quoteaf_os(spec)))?;
    Ok(OwnerSpec {
        uid: Some(uid),
        gid: None,
    })
}

/// Parse a `--from=CURRENT_OWNER:CURRENT_GROUP` filter. Either side may be
/// empty to mean "don't check".
fn parse_from_filter(spec: &str, db: &Db) -> Result<(Option<u32>, Option<u32>), String> {
    if let Some(colon_pos) = spec.find(':') {
        let owner_str = &spec[..colon_pos];
        let group_str = &spec[colon_pos + 1..];

        let uid = if owner_str.is_empty() {
            None
        } else {
            Some(
                resolve_uid(owner_str, db)
                    .ok_or_else(|| format!("unknown user in --from: {}", quoteaf_os(owner_str)))?,
            )
        };

        let gid = if group_str.is_empty() {
            None
        } else {
            Some(
                resolve_gid(group_str, db)
                    .ok_or_else(|| format!("unknown group in --from: {}", quoteaf_os(group_str)))?,
            )
        };

        Ok((uid, gid))
    } else {
        // Just an owner, no group filter.
        let uid = resolve_uid(spec, db)
            .ok_or_else(|| format!("unknown user in --from: {}", quoteaf_os(spec)))?;
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

// ============================================================================
// Argument parsing
// ============================================================================

/// Parsed command-line options.
///
/// THIS CRATE ANSWERED TO `chmod` TOO, and could not be reached that way:
/// `userspace/coreutils/src/bin/chmod.rs` produces the `chmod` executable, so
/// the personality here was finished code nobody could run. It is deleted
/// rather than left as dead weight, per design-decisions.md §1006.
struct Options {
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

fn parse_args(args: &[String], db: &Db) -> Result<Options, String> {
    if args.is_empty() {
        return Err("no arguments provided".to_string());
    }

    let mut opts = Options {
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

        if arg == "-h" || arg == "--no-dereference" {
            opts.no_deref = true;
            i += 1;
            continue;
        }

        // --from=OWNER:GROUP
        if let Some(from_val) = arg.strip_prefix("--from=") {
            let (fuid, fgid) = parse_from_filter(from_val, db)?;
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
        return Err("missing owner operand".to_string());
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
fn run_chown(opts: &Options, db: &Db) -> bool {
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
        match parse_owner_spec(&opts.spec, db) {
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
    println!("OWNER and GROUP may be names (from /etc/passwd and /etc/group) or numeric IDs.");
    println!();
    println!("EXAMPLES:");
    println!("  chown root:admin /etc/config.yaml");
    println!("  chown -R www:www /var/www");
    println!("  chown :users myfile.txt");
    println!("  chown --from=root:root alice:staff /shared/*");
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    // Load the user database for name resolution.
    let db = read_db();

    let opts = match parse_args(&args, &db) {
        Ok(o) => o,
        Err(msg) => {
            if msg.is_empty() {
                print_chown_help();
                process::exit(0);
            }
            eprintln!("chown: {msg}");
            eprintln!("Try 'chown --help' for usage information.");
            process::exit(1);
        }
    };

    let success = run_chown(&opts, &db);

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

    /// The two files a POSIX system turns a name into a number with.
    ///
    /// Written as text because that is what they are: `pwdb` reads bytes, and
    /// since `design-decisions.md` §353 these files are *generated* from
    /// `/etc/users.yaml` by `userdb::UserDb::save`, so a fixture written by
    /// hand and one written by the generator have the same shape. The gids are
    /// the system's, not this crate's: it used to number groups itself, from
    /// 101, in the order it happened to meet them.
    fn sample_db() -> Db {
        Db::from_bytes(
            b"root:x:0:0:root:/root:/bin/sh\n\
              alice:x:1000:100:Alice:/home/alice:/bin/sh\n",
            b"root:x:0:\n\
              admin:x:1:\n\
              users:x:100:alice\n\
              staff:x:101:alice\n\
              wheel:x:10:alice\n",
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

    // ---- owner spec parsing ------------------------------------------------

    #[test]
    fn owner_spec_user_only() {
        let db = sample_db();
        let spec = parse_owner_spec("alice", &db).unwrap();
        assert_eq!(spec.uid, Some(1000));
        assert_eq!(spec.gid, None);
    }

    #[test]
    fn owner_spec_user_and_group() {
        let db = sample_db();
        let spec = parse_owner_spec("root:admin", &db).unwrap();
        assert_eq!(spec.uid, Some(0));
        assert_eq!(spec.gid, Some(1)); // admin = gid 1
    }

    #[test]
    fn owner_spec_group_only() {
        let db = sample_db();
        let spec = parse_owner_spec(":users", &db).unwrap();
        assert_eq!(spec.uid, None);
        assert_eq!(spec.gid, Some(100)); // users = gid 100
    }

    #[test]
    fn owner_spec_numeric() {
        let db = sample_db();
        let spec = parse_owner_spec("4242:99", &db).unwrap();
        assert_eq!(spec.uid, Some(4242));
        assert_eq!(spec.gid, Some(99));
    }

    #[test]
    fn owner_spec_trailing_colon_uses_primary_group() {
        let db = sample_db();
        // alice's primary group is her `gid`, 100 -- not the first entry of
        // her supplementary list, which happens to have the same name here and
        // would not on an account whose own group is its private one.
        let spec = parse_owner_spec("alice:", &db).unwrap();
        assert_eq!(spec.uid, Some(1000));
        assert_eq!(spec.gid, Some(100));
    }

    /// `chown alice:` follows the primary group, not the first supplementary
    /// one. An account in `audio` and `video` whose own group is its private
    /// one used to have its files handed to `audio`.
    #[test]
    fn owner_spec_trailing_colon_ignores_the_supplementary_groups() {
        let db = Db::from_bytes(
            b"alice:x:1000:1000:Alice:/home/alice:/bin/sh\n",
            b"audio:x:29:alice\nvideo:x:44:alice\nalice:x:1000:\n",
        );
        let spec = parse_owner_spec("alice:", &db).unwrap();
        assert_eq!(spec.gid, Some(1000));
    }

    /// A record with no `gid` in the database is generated into `/etc/passwd`
    /// with its uid in the gid column -- the user-private-group convention --
    /// so by the time it is read here the fallback has already been applied,
    /// and this crate does not apply a second one of its own.
    #[test]
    fn an_account_generated_with_no_gid_of_its_own_is_in_the_group_named_by_its_uid() {
        let db = Db::from_bytes(
            b"alice:x:1000:1000:Alice:/home/alice:/bin/sh\n",
            b"audio:x:29:alice\n",
        );
        let spec = parse_owner_spec("alice:", &db).unwrap();
        assert_eq!(spec.gid, Some(1000));
    }

    #[test]
    fn owner_spec_unknown_user_errors() {
        let db = sample_db();
        assert!(parse_owner_spec("nobody", &db).is_err());
    }

    #[test]
    fn owner_spec_unknown_group_errors() {
        let db = sample_db();
        assert!(parse_owner_spec(":nogroup", &db).is_err());
    }

    // ---- group table / resolution -----------------------------------------

    /// A group's id is the one `/etc/group` gives it, not a position.
    ///
    /// This crate used to build its own table: `root`, `admin` and `users` got
    /// fixed ids and everything else was numbered from 101 in order of first
    /// appearance. `staff` was 101 here because it was the first such group
    /// mentioned -- not because anything on the system said so, and adding an
    /// account could change it. `chgrp staff` therefore set a gid no other
    /// program agreed with.
    #[test]
    fn a_group_resolves_to_the_id_the_group_file_gives_it() {
        let db = sample_db();
        assert_eq!(resolve_gid("root", &db), Some(0));
        assert_eq!(resolve_gid("admin", &db), Some(1));
        assert_eq!(resolve_gid("users", &db), Some(100));
        assert_eq!(resolve_gid("staff", &db), Some(101));
        assert_eq!(resolve_gid("wheel", &db), Some(10));
        assert_eq!(resolve_gid("nosuchgroup", &db), None);
    }

    /// A numeric argument is taken as the id itself, for a group that is not
    /// in the file at all -- a file may be owned by a gid no name maps to.
    #[test]
    fn a_numeric_group_needs_no_entry() {
        let db = sample_db();
        assert_eq!(resolve_gid("4242", &db), Some(4242));
    }

    #[test]
    fn resolve_uid_numeric_and_name() {
        let db = sample_db();
        assert_eq!(resolve_uid("alice", &db), Some(1000));
        assert_eq!(resolve_uid("0", &db), Some(0));
        assert_eq!(resolve_uid("7777", &db), Some(7777));
        assert_eq!(resolve_uid("ghost", &db), None);
    }

    /// Names resolve through the file the writer produced.
    ///
    /// The point the `useradm`-round-trip test used to make, made one step
    /// further along: `userdb::UserDb::save` *generates* `/etc/passwd` and
    /// `/etc/group`'s companion, and this crate reads the generated bytes. A
    /// reader and a writer that disagree can only be seen to disagree at the
    /// step where one consumes the other's output.
    #[test]
    fn names_resolve_through_a_passwd_file_the_generator_produced() {
        let scratch = scratchdir::ScratchDir::new("chown-generated");
        let path = scratch.path("users.yaml");
        let mut db = userdb::UserDb::new();
        let mut alice = userdb::Record::new();
        alice.set_uid(1000);
        alice.set_gid(100);
        alice.set(userdb::field::USERNAME, "alice");
        db.push(alice);
        db.save(&path).expect("save");

        let passwd = std::fs::read(scratch.path(userdb::PASSWD_NAME)).expect("generated passwd");
        let read = Db::from_bytes(&passwd, b"users:x:100:\n");
        assert_eq!(resolve_uid("alice", &read), Some(1000));
        assert_eq!(primary_gid(1000, &read), Some(100));
    }

    // ---- --from filter parsing ---------------------------------------------

    #[test]
    fn from_filter_owner_and_group() {
        let db = sample_db();
        let (u, g) = parse_from_filter("root:admin", &db).unwrap();
        assert_eq!(u, Some(0));
        assert_eq!(g, Some(1));
    }

    #[test]
    fn from_filter_owner_only() {
        let db = sample_db();
        let (u, g) = parse_from_filter("alice", &db).unwrap();
        assert_eq!(u, Some(1000));
        assert_eq!(g, None);
    }

    #[test]
    fn from_filter_group_only() {
        let db = sample_db();
        let (u, g) = parse_from_filter(":users", &db).unwrap();
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
