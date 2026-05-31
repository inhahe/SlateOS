//! OurOS File Ownership and Permission Utility
//!
//! Dual-mode binary: invoked as `chown` it changes file owner/group; invoked
//! as `chmod` it changes file permission bits. Mode detection is via `argv[0]`.
//!
//! User/group name resolution reads `/etc/users.yaml`, the OurOS user database.
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

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

// ============================================================================
// Syscall numbers (fs zone: 600-799)
// ============================================================================
//
// These map to the real OurOS VFS handlers. The previous version targeted
// Linux numbers 30/31 — which on OurOS are IRQ_REGISTER / IRQ_WAIT, so a chown
// or chmod would have tried to register or block on a hardware interrupt line.

/// Read file metadata (`SYS_FS_METADATA`).
///
/// arg0 = path pointer, arg1 = path length, arg2 = output buffer pointer
/// (`FS_META_SIZE` bytes). On success returns 0 and fills the buffer.
const SYS_FS_METADATA: u64 = 628;

/// Change file owner and group (`SYS_FS_SET_OWNER`).
///
/// arg0 = path pointer, arg1 = path length, arg2 = uid (u32), arg3 = gid (u32).
/// A uid or gid of `u32::MAX` means "leave that field unchanged"; the kernel
/// resolves the sentinel against the file's current owner.
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
/// Register mapping follows the OurOS syscall ABI:
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

/// Map a negative OurOS kernel error code to a human-readable string.
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

const USER_DB_PATH: &str = "/etc/users.yaml";

/// A resolved user entry from the OurOS user database.
struct UserEntry {
    uid: u32,
    username: String,
    groups: Vec<String>,
}

/// A resolved group with a numeric GID.
///
/// OurOS assigns GIDs by order of appearance in the groups collected across
/// all user entries. Group 0 = "root", group 1 = "admin", etc. The exact
/// mapping is built at runtime from `/etc/users.yaml`.
struct GroupEntry {
    gid: u32,
    name: String,
}

/// Read all users from /etc/users.yaml (same format as useradm).
fn read_users() -> Vec<UserEntry> {
    let content = match fs::read_to_string(USER_DB_PATH) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut users = Vec::new();
    let mut uid: u32 = 0;
    let mut username = String::new();
    let mut groups: Vec<String> = Vec::new();
    let mut in_entry = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("- uid:") || trimmed.starts_with("-  uid:") {
            // Flush previous entry.
            if in_entry && !username.is_empty() {
                users.push(UserEntry {
                    uid,
                    username: username.clone(),
                    groups: groups.clone(),
                });
            }
            uid = trimmed
                .split(':')
                .nth(1)
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0);
            username.clear();
            groups.clear();
            in_entry = true;
        } else if in_entry {
            if let Some(val) = trimmed.strip_prefix("username:") {
                username = val.trim().trim_matches('"').to_string();
            } else if let Some(val) = trimmed.strip_prefix("groups:") {
                let val = val.trim().trim_matches(|c: char| c == '[' || c == ']');
                groups = val
                    .split(',')
                    .map(|g| g.trim().trim_matches('"').to_string())
                    .filter(|g| !g.is_empty())
                    .collect();
            }
        }
    }

    // Flush the last entry.
    if in_entry && !username.is_empty() {
        users.push(UserEntry {
            uid,
            username,
            groups,
        });
    }

    users
}

/// Build the group table by collecting every unique group name from all users
/// and assigning GIDs in order. Well-known groups get fixed IDs:
///   root=0, admin=1, users=100.
fn build_group_table(users: &[UserEntry]) -> Vec<GroupEntry> {
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
    for user in users {
        for g in &user.groups {
            if !seen.contains(g) {
                groups.push(GroupEntry {
                    gid: next_gid,
                    name: g.clone(),
                });
                seen.insert(g.clone());
                next_gid = next_gid.saturating_add(1);
            }
        }
    }

    groups
}

/// Resolve a username to a UID.
fn resolve_uid(name: &str, users: &[UserEntry]) -> Option<u32> {
    // Try numeric first.
    if let Ok(n) = name.parse::<u32>() {
        return Some(n);
    }
    users.iter().find(|u| u.username == name).map(|u| u.uid)
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
#[cfg(target_arch = "x86_64")]
fn do_chown(path: &str, uid: u32, gid: u32) -> Result<(), String> {
    // SAFETY: SYS_FS_SET_OWNER reads `path.len()` bytes from `path.as_ptr()`
    // and takes uid in arg2 and gid in arg3. The path slice outlives the call.
    let ret = unsafe {
        syscall4(
            SYS_FS_SET_OWNER,
            path.as_ptr() as u64,
            path.len() as u64,
            uid as u64,
            gid as u64,
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
fn do_chown(_path: &str, _uid: u32, _gid: u32) -> Result<(), String> {
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

/// Recursively collect all paths under a directory (depth-first).
///
/// The directory itself is included as the last entry so that ownership/mode
/// changes propagate from leaves to root (allowing the directory to remain
/// readable during traversal).
fn collect_recursive(base: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    collect_recursive_inner(base, &mut results);
    results
}

fn collect_recursive_inner(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => {
            // Cannot read this directory -- include it anyway so the caller
            // can report the error during the actual chown/chmod call.
            out.push(dir.to_path_buf());
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => {
                out.push(path);
                continue;
            }
        };

        if ft.is_dir() {
            collect_recursive_inner(&path, out);
        } else {
            out.push(path);
        }
    }

    // Directory itself comes last (leaf-first order).
    out.push(dir.to_path_buf());
}

// ============================================================================
// Symbolic mode parsing (chmod)
// ============================================================================

/// Permission bits: standard POSIX layout.
const S_ISUID: u32 = 0o4000;
const S_ISGID: u32 = 0o2000;
const S_ISVTX: u32 = 0o1000;
const S_IRUSR: u32 = 0o0400;
const S_IWUSR: u32 = 0o0200;
const S_IXUSR: u32 = 0o0100;
const S_IRGRP: u32 = 0o0040;
const S_IWGRP: u32 = 0o0020;
const S_IXGRP: u32 = 0o0010;
const S_IROTH: u32 = 0o0004;
const S_IWOTH: u32 = 0o0002;
const S_IXOTH: u32 = 0o0001;

/// A single symbolic mode clause, e.g. `u+rx` or `go-w`.
struct ModeClause {
    /// Which classes: user, group, other. If none are set, treat as "all".
    who_user: bool,
    who_group: bool,
    who_other: bool,
    /// Operation: '+' (add), '-' (remove), '=' (set exactly).
    op: char,
    /// Permission bits being affected.
    read: bool,
    write: bool,
    execute: bool,
    setuid: bool,
    setgid: bool,
    sticky: bool,
}

/// Parse a symbolic mode string like `u+x`, `go-w`, `a=rwx`, or `u+rwx,g=rx,o=r`.
///
/// Returns a list of clauses to apply in order.
fn parse_symbolic_mode(mode_str: &str) -> Result<Vec<ModeClause>, String> {
    let mut clauses = Vec::new();

    for part in mode_str.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        let bytes = part.as_bytes();
        let len = bytes.len();
        let mut pos = 0;

        // Parse the "who" portion: [ugoa]*
        let mut who_u = false;
        let mut who_g = false;
        let mut who_o = false;
        let mut who_any = false;

        while pos < len {
            match bytes[pos] {
                b'u' => { who_u = true; who_any = true; }
                b'g' => { who_g = true; who_any = true; }
                b'o' => { who_o = true; who_any = true; }
                b'a' => { who_u = true; who_g = true; who_o = true; who_any = true; }
                _ => break,
            }
            pos += 1;
        }

        // If no "who" was specified, default to "all".
        if !who_any {
            who_u = true;
            who_g = true;
            who_o = true;
        }

        // Parse the operator: +, -, =
        if pos >= len {
            return Err(format!("invalid mode: '{part}' (missing operator)"));
        }

        let op = bytes[pos] as char;
        if op != '+' && op != '-' && op != '=' {
            return Err(format!(
                "invalid mode: '{part}' (expected +, -, or = at position {pos})"
            ));
        }
        pos += 1;

        // Parse the permission letters: [rwxstXugo]*
        let mut r = false;
        let mut w = false;
        let mut x = false;
        let mut suid = false;
        let mut sgid = false;
        let mut sticky = false;

        while pos < len {
            match bytes[pos] {
                b'r' => r = true,
                b'w' => w = true,
                b'x' | b'X' => x = true,
                b's' => {
                    // setuid if 'u' is in who, setgid if 'g' is in who
                    if who_u { suid = true; }
                    if who_g { sgid = true; }
                    // If neither u nor g was explicit, default both
                    if !who_u && !who_g { suid = true; sgid = true; }
                }
                b't' => sticky = true,
                _ => {
                    return Err(format!(
                        "invalid permission character '{}' in '{part}'",
                        bytes[pos] as char
                    ));
                }
            }
            pos += 1;
        }

        clauses.push(ModeClause {
            who_user: who_u,
            who_group: who_g,
            who_other: who_o,
            op,
            read: r,
            write: w,
            execute: x,
            setuid: suid,
            setgid: sgid,
            sticky,
        });
    }

    if clauses.is_empty() {
        return Err("empty mode string".to_string());
    }

    Ok(clauses)
}

/// Build a bitmask of the permissions described by a clause for a given "who".
fn clause_bits(clause: &ModeClause) -> u32 {
    let mut bits: u32 = 0;

    if clause.who_user {
        if clause.read { bits |= S_IRUSR; }
        if clause.write { bits |= S_IWUSR; }
        if clause.execute { bits |= S_IXUSR; }
    }
    if clause.who_group {
        if clause.read { bits |= S_IRGRP; }
        if clause.write { bits |= S_IWGRP; }
        if clause.execute { bits |= S_IXGRP; }
    }
    if clause.who_other {
        if clause.read { bits |= S_IROTH; }
        if clause.write { bits |= S_IWOTH; }
        if clause.execute { bits |= S_IXOTH; }
    }
    if clause.setuid { bits |= S_ISUID; }
    if clause.setgid { bits |= S_ISGID; }
    if clause.sticky { bits |= S_ISVTX; }

    bits
}

/// Build a mask of all bits that a clause affects, for use with '=' to clear
/// unmentioned bits in the relevant classes.
fn clause_who_mask(clause: &ModeClause) -> u32 {
    let mut mask: u32 = 0;
    if clause.who_user { mask |= S_IRUSR | S_IWUSR | S_IXUSR; }
    if clause.who_group { mask |= S_IRGRP | S_IWGRP | S_IXGRP; }
    if clause.who_other { mask |= S_IROTH | S_IWOTH | S_IXOTH; }
    // '=' on user also clears setuid, on group clears setgid, on any clears sticky
    if clause.who_user { mask |= S_ISUID; }
    if clause.who_group { mask |= S_ISGID; }
    mask |= S_ISVTX;
    mask
}

/// Apply a list of symbolic mode clauses to an existing mode value.
fn apply_symbolic_mode(mut current: u32, clauses: &[ModeClause]) -> u32 {
    for clause in clauses {
        let bits = clause_bits(clause);

        match clause.op {
            '+' => current |= bits,
            '-' => current &= !bits,
            '=' => {
                let mask = clause_who_mask(clause);
                current = (current & !mask) | bits;
            }
            _ => {} // unreachable: parse_symbolic_mode validates op
        }
    }
    current
}

/// Parse a mode string which may be octal or symbolic.
///
/// Returns `Ok(Left(octal))` for absolute modes or `Ok(Right(clauses))` for
/// symbolic modes that need to be applied to the current mode.
fn parse_mode(mode_str: &str) -> Result<ModeSpec, String> {
    // Try octal first: must be all digits 0-7, optionally prefixed with '0'.
    let trimmed = mode_str.strip_prefix('0').unwrap_or(mode_str);
    if !trimmed.is_empty() && trimmed.bytes().all(|b| b.is_ascii_digit() && b <= b'7') {
        let val = u32::from_str_radix(trimmed, 8)
            .map_err(|e| format!("invalid octal mode '{mode_str}': {e}"))?;
        if val > 0o7777 {
            return Err(format!("mode value {val:#o} exceeds maximum 7777"));
        }
        return Ok(ModeSpec::Absolute(val));
    }

    // Fall back to symbolic parsing.
    let clauses = parse_symbolic_mode(mode_str)?;
    Ok(ModeSpec::Symbolic(clauses))
}

enum ModeSpec {
    /// An absolute octal mode (e.g. 0755).
    Absolute(u32),
    /// A list of symbolic clauses to apply to the current mode.
    Symbolic(Vec<ModeClause>),
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
    users: &[UserEntry],
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

        let uid = resolve_uid(owner_str, users)
            .ok_or_else(|| format!("unknown user: '{owner_str}'"))?;

        let gid = if group_str.is_empty() {
            // `OWNER:` -- set group to the owner's primary group
            users
                .iter()
                .find(|u| u.uid == uid)
                .and_then(|u| u.groups.first())
                .and_then(|g| resolve_gid(g, groups))
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
    let uid = resolve_uid(spec, users)
        .ok_or_else(|| format!("unknown user: '{spec}'"))?;
    Ok(OwnerSpec {
        uid: Some(uid),
        gid: None,
    })
}

/// Parse a `--from=CURRENT_OWNER:CURRENT_GROUP` filter. Either side may be
/// empty to mean "don't check".
fn parse_from_filter(
    spec: &str,
    users: &[UserEntry],
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
        let uid = resolve_uid(spec, users)
            .ok_or_else(|| format!("unknown user in --from: '{spec}'"))?;
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

fn parse_args(
    args: &[String],
    users: &[UserEntry],
    groups: &[GroupEntry],
) -> Result<Options, String> {
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
fn chown_one(
    path: &str,
    spec: &OwnerSpec,
    opts: &Options,
) -> (bool, Option<String>) {
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
                        "chown: cannot verify current ownership of '{path}' for --from; skipping"
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

    match do_chown(path, uid, gid) {
        Ok(()) => {
            let owner_str = format_owner(spec.uid, spec.gid);
            if opts.json {
                print_chown_json(path, spec.uid, spec.gid, true, "");
            } else if opts.verbose {
                if changed {
                    eprintln!("changed ownership of '{path}' to {owner_str}");
                } else {
                    eprintln!("ownership of '{path}' retained as {owner_str}");
                }
            } else if opts.changes && changed {
                eprintln!("changed ownership of '{path}' to {owner_str}");
            }
            (changed, None)
        }
        Err(e) => {
            if opts.json {
                print_chown_json(path, spec.uid, spec.gid, false, &e);
            } else if !opts.silent {
                eprintln!("chown: cannot change ownership of '{path}': {e}");
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
fn run_chown(
    opts: &Options,
    users: &[UserEntry],
    groups: &[GroupEntry],
) -> bool {
    // -h / --no-dereference asks us to operate on the symlink itself. The
    // OurOS VFS set_owner path resolves symlinks (resolve_follow) and there is
    // no lchown-equivalent syscall yet, so we cannot honor this. Warn rather
    // than silently chown the target. (Tracked in todo.txt.)
    if opts.no_deref && !opts.silent {
        eprintln!(
            "chown: warning: -h/--no-dereference is not supported on OurOS; symlink targets will be affected"
        );
    }

    let spec = if let Some(ref refpath) = opts.reference {
        // --reference: copy owner/group from the reference file's metadata.
        match read_metadata(refpath) {
            Ok(meta) => OwnerSpec {
                uid: Some(meta.uid),
                gid: Some(meta.gid),
            },
            Err(e) => {
                if !opts.silent {
                    eprintln!("chown: cannot read reference '{refpath}': {e}");
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

    for file in &opts.files {
        let paths: Vec<PathBuf> = if opts.recursive {
            let p = Path::new(file);
            if p.is_dir() {
                collect_recursive(p)
            } else {
                vec![p.to_path_buf()]
            }
        } else {
            vec![PathBuf::from(file)]
        };

        for path in &paths {
            let path_str = path.to_string_lossy();
            // --from filtering is handled inside chown_one, which has access
            // to the file's current metadata.
            let (_, err) = chown_one(&path_str, &spec, opts);
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
                    eprintln!("mode of '{path}' changed to {:04o}", mode_val & 0o7777);
                } else {
                    eprintln!("mode of '{path}' retained as {:04o}", mode_val & 0o7777);
                }
            } else if opts.changes && changed {
                eprintln!("mode of '{path}' changed to {:04o}", mode_val & 0o7777);
            }
            (changed, None)
        }
        Err(e) => {
            if opts.json {
                print_chmod_json(path, mode_val, false, &e);
            } else if !opts.silent {
                eprintln!("chmod: cannot change mode of '{path}': {e}");
            }
            (false, Some(e))
        }
    }
}

/// Execute chmod for all target files.
fn run_chmod(opts: &Options) -> bool {
    // Parse the mode spec. --reference copies the reference file's mode as an
    // absolute value.
    let mode_spec = if let Some(ref refpath) = opts.reference {
        match read_metadata(refpath) {
            Ok(meta) => ModeSpec::Absolute(meta.perms & 0o7777),
            Err(e) => {
                if !opts.silent {
                    eprintln!("chmod: cannot read reference '{refpath}': {e}");
                }
                return false;
            }
        }
    } else {
        match parse_mode(&opts.spec) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("chmod: {e}");
                return false;
            }
        }
    };

    let mut any_error = false;

    for file in &opts.files {
        let paths: Vec<PathBuf> = if opts.recursive {
            let p = Path::new(file);
            if p.is_dir() {
                collect_recursive(p)
            } else {
                vec![p.to_path_buf()]
            }
        } else {
            vec![PathBuf::from(file)]
        };

        for path in &paths {
            let path_str = path.to_string_lossy();

            // Read the current mode (best-effort) for symbolic application and
            // change detection.
            let current_mode = read_metadata(&path_str).ok().map(|m| m.perms & 0o7777);

            let mode_val = match &mode_spec {
                ModeSpec::Absolute(m) => *m,
                ModeSpec::Symbolic(clauses) => {
                    // Symbolic modes apply deltas to the current mode. If the
                    // current mode is unknown, fall back to 0o000 as the base:
                    // '+' and '=' still behave correctly, and '-' on unset bits
                    // is a harmless no-op.
                    let base = current_mode.unwrap_or(0o000);
                    apply_symbolic_mode(base, clauses)
                }
            };

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
    println!("OurOS chown v0.1.0 -- Change file owner and group");
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
    println!("OurOS chmod v0.1.0 -- Change file permissions");
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

    let binary_mode = args
        .first()
        .map(|a| detect_mode(a))
        .unwrap_or(Mode::Chown);

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
            let name = if binary_mode == Mode::Chown { "chown" } else { "chmod" };
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

    fn sample_users() -> Vec<UserEntry> {
        vec![
            UserEntry {
                uid: 0,
                username: "root".to_string(),
                groups: vec!["root".to_string(), "admin".to_string()],
            },
            UserEntry {
                uid: 1000,
                username: "alice".to_string(),
                groups: vec!["users".to_string(), "staff".to_string()],
            },
        ]
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

    // ---- octal mode parsing ------------------------------------------------

    #[test]
    fn parse_mode_octal_basic() {
        match parse_mode("755").unwrap() {
            ModeSpec::Absolute(m) => assert_eq!(m, 0o755),
            ModeSpec::Symbolic(_) => panic!("expected absolute"),
        }
    }

    #[test]
    fn parse_mode_octal_with_leading_zero() {
        match parse_mode("0644").unwrap() {
            ModeSpec::Absolute(m) => assert_eq!(m, 0o644),
            ModeSpec::Symbolic(_) => panic!("expected absolute"),
        }
    }

    #[test]
    fn parse_mode_octal_with_setuid() {
        match parse_mode("4755").unwrap() {
            ModeSpec::Absolute(m) => assert_eq!(m, 0o4755),
            ModeSpec::Symbolic(_) => panic!("expected absolute"),
        }
    }

    #[test]
    fn parse_mode_rejects_too_large() {
        assert!(parse_mode("77777").is_err());
    }

    #[test]
    fn parse_mode_digit_8_is_symbolic_not_octal() {
        // "8" is not a valid octal digit, so it falls through to symbolic
        // parsing, which then fails (no operator).
        assert!(parse_mode("8").is_err());
    }

    // ---- symbolic mode parsing ---------------------------------------------

    #[test]
    fn symbolic_add_user_execute() {
        let clauses = parse_symbolic_mode("u+x").unwrap();
        let result = apply_symbolic_mode(0o644, &clauses);
        assert_eq!(result, 0o744);
    }

    #[test]
    fn symbolic_remove_group_other_write() {
        let clauses = parse_symbolic_mode("go-w").unwrap();
        let result = apply_symbolic_mode(0o666, &clauses);
        assert_eq!(result, 0o644);
    }

    #[test]
    fn symbolic_set_exact_all() {
        let clauses = parse_symbolic_mode("a=rx").unwrap();
        let result = apply_symbolic_mode(0o777, &clauses);
        assert_eq!(result, 0o555);
    }

    #[test]
    fn symbolic_no_who_defaults_to_all() {
        let clauses = parse_symbolic_mode("+x").unwrap();
        let result = apply_symbolic_mode(0o644, &clauses);
        assert_eq!(result, 0o755);
    }

    #[test]
    fn symbolic_multiple_clauses() {
        let clauses = parse_symbolic_mode("u=rwx,g=rx,o=r").unwrap();
        let result = apply_symbolic_mode(0o000, &clauses);
        assert_eq!(result, 0o754);
    }

    #[test]
    fn symbolic_setuid() {
        let clauses = parse_symbolic_mode("u+s").unwrap();
        let result = apply_symbolic_mode(0o755, &clauses);
        assert_eq!(result, 0o4755);
    }

    #[test]
    fn symbolic_sticky() {
        let clauses = parse_symbolic_mode("+t").unwrap();
        let result = apply_symbolic_mode(0o755, &clauses);
        assert_eq!(result, 0o1755);
    }

    #[test]
    fn symbolic_set_clears_unmentioned_bits() {
        // u=r should clear the existing write/execute bits for the user.
        let clauses = parse_symbolic_mode("u=r").unwrap();
        let result = apply_symbolic_mode(0o777, &clauses);
        assert_eq!(result, 0o477);
    }

    #[test]
    fn symbolic_missing_operator_errors() {
        assert!(parse_symbolic_mode("u").is_err());
    }

    #[test]
    fn symbolic_invalid_perm_char_errors() {
        assert!(parse_symbolic_mode("u+z").is_err());
    }

    #[test]
    fn symbolic_empty_errors() {
        assert!(parse_symbolic_mode("").is_err());
    }

    // ---- clause bit helpers ------------------------------------------------

    #[test]
    fn clause_bits_user_rwx() {
        let clauses = parse_symbolic_mode("u+rwx").unwrap();
        assert_eq!(clause_bits(&clauses[0]), S_IRUSR | S_IWUSR | S_IXUSR);
    }

    #[test]
    fn clause_who_mask_user_includes_setuid() {
        let clauses = parse_symbolic_mode("u=r").unwrap();
        let mask = clause_who_mask(&clauses[0]);
        assert!(mask & S_ISUID != 0);
        assert!(mask & S_IRUSR != 0);
        assert!(mask & S_IRGRP == 0);
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
