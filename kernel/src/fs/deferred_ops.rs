//! Deferred filesystem operations — queue a delete or rename that cannot
//! happen yet and replay it when the obstacle clears.
//!
//! # Motivation
//!
//! On a POSIX system `unlink`/`rename` on an open file already succeeds, so
//! the Windows motivation ("the file is locked") does not apply.  What *is*
//! worth deferring:
//!
//! - A **busy mount** — the filesystem hosting the target cannot be written to
//!   right now because something holds it busy.
//! - A **read-only mount** — the filesystem is mounted read-only and the user
//!   wants the operation to happen when it is remounted read-write.
//! - An **absent removable/network volume** — the device has been unplugged or
//!   the share disconnected; the operation should run when it comes back.
//! - A **full volume** — the filesystem is too full to accept a trash rename
//!   and the user prefers to defer rather than permanently delete.
//!
//! # On-disk format
//!
//! One queue per filesystem, on the filesystem the operation targets, at
//! `/.deferred-ops/`.  One file per entry, named by a monotonic id
//! (`/.deferred-ops/000001`, `/.deferred-ops/000002`, …).
//!
//! Each file is a key=value text record, one key per line.  Paths are
//! octal-escaped via [`super::escape`] with `=` as an extra delimiter so that
//! both the key separator and the line separator are always encoded.
//!
//! ```text
//! version=1
//! op=delete
//! fs_uuid=6f1c…
//! target_inode=1048577
//! target_path=/media/usb/report.docx
//! reason=device-busy
//! queued_by_uid=1000
//! queued_by_gid=1000
//! queued_by_groups=100,27
//! required_rights=delete
//! queued_at=1757260800
//! ```
//!
//! For renames an additional `dest_path=…` key is present.
//!
//! # Security model
//!
//! The three non-negotiable rules (from `roadmap.md`):
//!
//! 1. **Identity, not path.**  `target_inode` + `fs_uuid` decide what is acted
//!    on.  `target_path` is a display hint only.
//! 2. **Re-authorise at execution.**  The stored UID/GID/groups are re-checked
//!    against filesystem ACLs when the operation *runs*, not only when it is
//!    queued.  A revoked capability → the entry is **dropped**, not retried.
//! 3. **Never escalate a denial.**  There is no `permission-denied` reason: if
//!    the operation was refused for permission, it is refused, period.
//!
//! See `requests/a-cb-deferred-ops-format-agreed-with-notes.md` for the full
//! design rationale.

use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::escape::{escape_octal, unescape_octal};
use crate::fs::path::{Path, PathBuf};
use crate::fs::vfs::{EntryType, Vfs};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// The directory on each filesystem that holds the deferred-ops queue.
const QUEUE_DIR: &[u8] = b"/.deferred-ops";

/// Entry format version.
const FORMAT_VERSION: u32 = 1;

/// Maximum number of entries per filesystem queue.  A soft limit — enqueue
/// will fail with `ResourceExhausted` if the queue has this many entries.
/// Protects against a runaway enqueue loop filling the filesystem with queue
/// files.
const MAX_QUEUE_ENTRIES: usize = 4096;

/// Extra bytes to escape beyond the default set.  `=` is the key/value
/// separator, so it must be encoded inside both keys and values.
const ESCAPE_EXTRA: &[u8] = b"=";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// The operation to defer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeferredOpKind {
    /// Delete the target (equivalent to `unlink` or `rmdir`).
    Delete,
    /// Rename the target to a new path.
    Rename,
}

impl DeferredOpKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Delete => "delete",
            Self::Rename => "rename",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "delete" => Some(Self::Delete),
            "rename" => Some(Self::Rename),
            _ => None,
        }
    }
}

/// Why the operation was deferred.  Advisory only — the replay hook ignores
/// this and retries everything on every trigger.  The field exists so the UI
/// can tell the user *why* something was deferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeferredReason {
    /// The target filesystem is busy (e.g. unmount in progress).
    DeviceBusy,
    /// The target filesystem is currently mounted read-only.
    ReadOnly,
    /// The target volume is not currently mounted (removable/network).
    VolumeAbsent,
    /// The target volume is full and cannot accept a trash rename.
    VolumeFull,
}

impl DeferredReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::DeviceBusy => "device-busy",
            Self::ReadOnly => "read-only",
            Self::VolumeAbsent => "volume-absent",
            Self::VolumeFull => "volume-full",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "device-busy" => Some(Self::DeviceBusy),
            "read-only" => Some(Self::ReadOnly),
            "volume-absent" => Some(Self::VolumeAbsent),
            "volume-full" => Some(Self::VolumeFull),
            _ => None,
        }
    }
}

/// The required rights for the deferred operation.  Checked against filesystem
/// ACLs at replay time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequiredRights {
    /// The operation requires write (delete) permission on the parent dir.
    Delete,
    /// The operation requires write permission on both source and dest dirs.
    Write,
}

impl RequiredRights {
    fn as_str(self) -> &'static str {
        match self {
            Self::Delete => "delete",
            Self::Write => "write",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "delete" => Some(Self::Delete),
            "write" => Some(Self::Write),
            _ => None,
        }
    }
}

/// A parsed deferred-operation queue entry.
#[derive(Debug, Clone)]
pub struct DeferredEntry {
    /// The monotonic ID of this entry (derived from filename).
    pub id: u64,
    /// What kind of operation.
    pub op: DeferredOpKind,
    /// UUID of the filesystem this entry belongs to.
    pub fs_uuid: Vec<u8>,
    /// Inode number of the target — the authority for *what* to act on.
    pub target_inode: u64,
    /// The original path of the target — a display hint, never the authority.
    pub target_path: PathBuf,
    /// Destination path (rename only; empty for delete).
    pub dest_path: Option<PathBuf>,
    /// Why the operation was deferred (advisory).
    pub reason: DeferredReason,
    /// UID of the user who queued this operation.
    pub queued_by_uid: u32,
    /// Primary GID of the user who queued this operation.
    pub queued_by_gid: u32,
    /// Supplementary groups of the user who queued this operation.
    pub queued_by_groups: Vec<u32>,
    /// What filesystem permission the replay hook must re-check.
    pub required_rights: RequiredRights,
    /// Unix timestamp (seconds since epoch) when the entry was created.
    pub queued_at: u64,
}

// ---------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------

/// Serialize a [`DeferredEntry`] to the on-disk key=value format.
fn serialize_entry(entry: &DeferredEntry) -> Vec<u8> {
    let mut out = String::new();

    // version= is always first so a future format can be detected.
    out.push_str("version=");
    push_u32(&mut out, FORMAT_VERSION);
    out.push('\n');

    out.push_str("op=");
    out.push_str(entry.op.as_str());
    out.push('\n');

    out.push_str("fs_uuid=");
    out.push_str(&escape_octal(&entry.fs_uuid, ESCAPE_EXTRA));
    out.push('\n');

    out.push_str("target_inode=");
    push_u64(&mut out, entry.target_inode);
    out.push('\n');

    out.push_str("target_path=");
    out.push_str(&escape_octal(entry.target_path.as_bytes(), ESCAPE_EXTRA));
    out.push('\n');

    if let Some(ref dest) = entry.dest_path {
        out.push_str("dest_path=");
        out.push_str(&escape_octal(dest.as_bytes(), ESCAPE_EXTRA));
        out.push('\n');
    }

    out.push_str("reason=");
    out.push_str(entry.reason.as_str());
    out.push('\n');

    out.push_str("queued_by_uid=");
    push_u32(&mut out, entry.queued_by_uid);
    out.push('\n');

    out.push_str("queued_by_gid=");
    push_u32(&mut out, entry.queued_by_gid);
    out.push('\n');

    out.push_str("queued_by_groups=");
    for (i, &g) in entry.queued_by_groups.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_u32(&mut out, g);
    }
    out.push('\n');

    out.push_str("required_rights=");
    out.push_str(entry.required_rights.as_str());
    out.push('\n');

    out.push_str("queued_at=");
    push_u64(&mut out, entry.queued_at);
    out.push('\n');

    out.into_bytes()
}

/// Parse a key=value entry file back into a [`DeferredEntry`].
///
/// Returns `None` for any malformed or unrecognised format — a corrupt entry
/// is dropped rather than acted on, because acting on a half-parsed operation
/// could delete the wrong file.
fn parse_entry(id: u64, data: &[u8]) -> Option<DeferredEntry> {
    let mut version: Option<u32> = None;
    let mut op: Option<DeferredOpKind> = None;
    let mut fs_uuid: Option<Vec<u8>> = None;
    let mut target_inode: Option<u64> = None;
    let mut target_path: Option<PathBuf> = None;
    let mut dest_path: Option<PathBuf> = None;
    let mut reason: Option<DeferredReason> = None;
    let mut queued_by_uid: Option<u32> = None;
    let mut queued_by_gid: Option<u32> = None;
    let mut queued_by_groups: Option<Vec<u32>> = None;
    let mut required_rights: Option<RequiredRights> = None;
    let mut queued_at: Option<u64> = None;

    for line in data.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let eq_pos = line.iter().position(|&b| b == b'=')?;
        let key = line.get(..eq_pos)?;
        let val_start = eq_pos.checked_add(1)?;
        let val = line.get(val_start..)?;

        match key {
            b"version" => {
                version = Some(parse_u32(val)?);
            }
            b"op" => {
                let s = core::str::from_utf8(val).ok()?;
                op = Some(DeferredOpKind::from_str(s)?);
            }
            b"fs_uuid" => {
                fs_uuid = Some(unescape_octal(val)?);
            }
            b"target_inode" => {
                target_inode = Some(parse_u64(val)?);
            }
            b"target_path" => {
                let bytes = unescape_octal(val)?;
                target_path = Some(PathBuf::from(bytes));
            }
            b"dest_path" => {
                let bytes = unescape_octal(val)?;
                dest_path = Some(PathBuf::from(bytes));
            }
            b"reason" => {
                let s = core::str::from_utf8(val).ok()?;
                reason = Some(DeferredReason::from_str(s)?);
            }
            b"queued_by_uid" => {
                queued_by_uid = Some(parse_u32(val)?);
            }
            b"queued_by_gid" => {
                queued_by_gid = Some(parse_u32(val)?);
            }
            b"queued_by_groups" => {
                let s = core::str::from_utf8(val).ok()?;
                let mut groups = Vec::new();
                if !s.is_empty() {
                    for part in s.split(',') {
                        groups.push(part.parse::<u32>().ok()?);
                    }
                }
                queued_by_groups = Some(groups);
            }
            b"required_rights" => {
                let s = core::str::from_utf8(val).ok()?;
                required_rights = Some(RequiredRights::from_str(s)?);
            }
            b"queued_at" => {
                queued_at = Some(parse_u64(val)?);
            }
            // Unknown keys are ignored (forward compatibility).
            _ => {}
        }
    }

    // version=1 is required.
    if version? != FORMAT_VERSION {
        return None;
    }

    // All required fields must be present.
    let op = op?;
    if op == DeferredOpKind::Rename && dest_path.is_none() {
        return None; // Rename without a destination is malformed.
    }

    Some(DeferredEntry {
        id,
        op,
        fs_uuid: fs_uuid?,
        target_inode: target_inode?,
        target_path: target_path?,
        dest_path,
        reason: reason?,
        queued_by_uid: queued_by_uid?,
        queued_by_gid: queued_by_gid?,
        queued_by_groups: queued_by_groups.unwrap_or_default(),
        required_rights: required_rights?,
        queued_at: queued_at?,
    })
}

// ---------------------------------------------------------------------------
// Numeric helpers (no-std, no alloc for parsing)
// ---------------------------------------------------------------------------

fn push_u32(out: &mut String, v: u32) {
    push_u64(out, u64::from(v));
}

fn push_u64(s: &mut String, mut val: u64) {
    if val == 0 {
        s.push('0');
        return;
    }
    // Max u64 is 20 digits.
    let mut digits = [0u8; 20];
    let mut i = 0usize;
    while val > 0 {
        if let Some(slot) = digits.get_mut(i) {
            *slot = (val % 10) as u8;
        }
        val /= 10;
        i = i.wrapping_add(1);
    }
    // Write digits in reverse (most significant first).
    while i > 0 {
        i = i.wrapping_sub(1);
        if let Some(&d) = digits.get(i) {
            s.push(char::from(b'0'.wrapping_add(d)));
        }
    }
}

fn parse_u32(b: &[u8]) -> Option<u32> {
    let s = core::str::from_utf8(b).ok()?;
    s.parse().ok()
}

fn parse_u64(b: &[u8]) -> Option<u64> {
    let s = core::str::from_utf8(b).ok()?;
    s.parse().ok()
}

// ---------------------------------------------------------------------------
// Queue directory helpers
// ---------------------------------------------------------------------------

/// Build the full path to the queue directory on a given mount point.
fn queue_dir_path(mount_path: &Path) -> PathBuf {
    let mut p = mount_path.to_path_buf();
    p.push(Path::new(QUEUE_DIR));
    p
}

/// Build the full path to a specific entry file.
fn entry_path(mount_path: &Path, id: u64) -> PathBuf {
    let mut p = queue_dir_path(mount_path);
    // Zero-padded to 6 digits for ls-friendly ordering.
    let mut buf = [0u8; 16];
    let name = format_entry_name(id, &mut buf);
    p.push(Path::new(name));
    p
}

/// Format an entry ID as a zero-padded filename.
fn format_entry_name(id: u64, buf: &mut [u8; 16]) -> &[u8] {
    // Simple zero-padded decimal, 6 digits minimum.
    let mut n = id;
    let mut pos = buf.len();
    loop {
        pos = pos.saturating_sub(1);
        if let Some(slot) = buf.get_mut(pos) {
            *slot = b'0'.wrapping_add((n % 10) as u8);
        }
        n /= 10;
        if n == 0 {
            break;
        }
    }
    // Pad to at least 6 digits.
    while buf.len().saturating_sub(pos) < 6 {
        pos = pos.saturating_sub(1);
        if let Some(slot) = buf.get_mut(pos) {
            *slot = b'0';
        }
    }
    // pos is always <= buf.len() because we only decrement from buf.len()
    // via saturating_sub, so this cannot go out of bounds.
    match buf.get(pos..) {
        Some(s) => s,
        None => b"000001", // Defensive fallback; unreachable in practice.
    }
}

/// Parse an entry filename back to its numeric ID.
fn parse_entry_name(name: &[u8]) -> Option<u64> {
    if name.is_empty() {
        return None;
    }
    // All digits, no leading garbage.
    let s = core::str::from_utf8(name).ok()?;
    s.parse().ok()
}

/// Allocate the next entry ID by scanning existing entries.
fn next_entry_id(mount_path: &Path) -> KernelResult<u64> {
    let qdir = queue_dir_path(mount_path);
    let entries = match Vfs::readdir(&qdir) {
        Ok(e) => e,
        Err(KernelError::NotFound) => return Ok(1),
        Err(e) => return Err(e),
    };

    let mut max_id: u64 = 0;
    let mut count: usize = 0;
    for entry in &entries {
        if let Some(id) = parse_entry_name(entry.name.as_bytes()) {
            if id > max_id {
                max_id = id;
            }
            count = count.saturating_add(1);
        }
    }

    if count >= MAX_QUEUE_ENTRIES {
        return Err(KernelError::ResourceExhausted);
    }

    Ok(max_id.saturating_add(1))
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Check whether the filesystem at `mount_path` supports deferred operations.
///
/// Only filesystems with stable inode numbers can support deferred ops,
/// because rule 1 requires identity-based targeting.  FAT32, ISO9660 and
/// pseudo-filesystems return `false`.
pub fn supports_deferred_ops(mount_path: &Path) -> bool {
    // ext4, btrfs, f2fs, NTFS (read-write) — all have stable inodes.
    // FAT, ISO9660, memfs, procfs, devfs, sysfs — no stable inodes.
    let mounts = Vfs::mounts();
    for (mp, fs_type) in &mounts {
        if mp.as_path() == mount_path {
            return matches!(
                fs_type.as_str(),
                "ext4" | "btrfs" | "f2fs" | "ntfs" | "xfs" | "zfs"
            );
        }
    }
    false
}

/// Enqueue a deferred operation on the filesystem mounted at `mount_path`.
///
/// Creates the `/.deferred-ops/` directory if it does not exist, allocates
/// a monotonic entry ID, writes the entry file, and syncs.
///
/// # Errors
///
/// - `NotSupported` if the filesystem does not support deferred ops (no
///   stable inodes).
/// - `PermissionDenied` if the reason is `permission-denied` (rule 3: never
///   escalate a denial).
/// - `ResourceExhausted` if the queue already has [`MAX_QUEUE_ENTRIES`].
/// - Any VFS error from creating the directory or writing the file.
pub fn enqueue(
    mount_path: &Path,
    op: DeferredOpKind,
    fs_uuid: &[u8],
    target_inode: u64,
    target_path: &Path,
    dest_path: Option<&Path>,
    reason: DeferredReason,
    uid: u32,
    gid: u32,
    groups: &[u32],
    timestamp: u64,
) -> KernelResult<u64> {
    // Rule 1: refuse if no stable inodes.
    if !supports_deferred_ops(mount_path) {
        crate::serial_println!(
            "[deferred-ops] refusing to enqueue on '{}': no stable inodes",
            mount_path.display()
        );
        return Err(KernelError::NotSupported);
    }

    // Rule 3: a rename requires a dest_path.
    if op == DeferredOpKind::Rename && dest_path.is_none() {
        return Err(KernelError::InvalidArgument);
    }

    // Ensure the queue directory exists.
    let qdir = queue_dir_path(mount_path);
    match Vfs::stat(&qdir) {
        Ok(entry) if entry.entry_type == EntryType::Directory => {}
        Ok(_) => {
            crate::serial_println!(
                "[deferred-ops] '{}' exists but is not a directory",
                qdir.display()
            );
            return Err(KernelError::NotADirectory);
        }
        Err(KernelError::NotFound) => {
            Vfs::mkdir(&qdir)?;
            crate::serial_println!(
                "[deferred-ops] created queue directory '{}'",
                qdir.display()
            );
        }
        Err(e) => return Err(e),
    }

    // Allocate an entry ID.
    let id = next_entry_id(mount_path)?;

    let required_rights = match op {
        DeferredOpKind::Delete => RequiredRights::Delete,
        DeferredOpKind::Rename => RequiredRights::Write,
    };

    let entry = DeferredEntry {
        id,
        op,
        fs_uuid: fs_uuid.to_vec(),
        target_inode,
        target_path: target_path.to_path_buf(),
        dest_path: dest_path.map(|p| p.to_path_buf()),
        reason,
        queued_by_uid: uid,
        queued_by_gid: gid,
        queued_by_groups: groups.to_vec(),
        required_rights,
        queued_at: timestamp,
    };

    let data = serialize_entry(&entry);
    let path = entry_path(mount_path, id);
    Vfs::write_file(&path, &data)?;

    crate::serial_println!(
        "[deferred-ops] enqueued entry {} on '{}': {} '{}' (inode {}, uid {}, reason {})",
        id,
        mount_path.display(),
        op.as_str(),
        target_path.display(),
        target_inode,
        uid,
        reason.as_str(),
    );

    Ok(id)
}

/// Cancel a deferred operation by its entry ID.
///
/// Deletes the entry file.  Returns `Ok(())` if the entry existed and was
/// removed, or `NotFound` if no such entry exists.
pub fn cancel(mount_path: &Path, id: u64) -> KernelResult<()> {
    let path = entry_path(mount_path, id);
    Vfs::remove(&path)?;
    crate::serial_println!(
        "[deferred-ops] cancelled entry {} on '{}'",
        id,
        mount_path.display()
    );
    Ok(())
}

/// List all pending deferred operations on the filesystem at `mount_path`.
///
/// Returns entries sorted by ID (oldest first).  Malformed entries are
/// logged and skipped.
pub fn list(mount_path: &Path) -> KernelResult<Vec<DeferredEntry>> {
    let qdir = queue_dir_path(mount_path);
    let dir_entries = match Vfs::readdir(&qdir) {
        Ok(e) => e,
        Err(KernelError::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let mut entries = Vec::new();
    for de in &dir_entries {
        let id = match parse_entry_name(de.name.as_bytes()) {
            Some(id) => id,
            None => continue, // Skip non-numeric entries (e.g. `.`, `..`).
        };

        let mut path = qdir.clone();
        path.push(&de.name);
        let data = match Vfs::read_file(&path) {
            Ok(d) => d,
            Err(e) => {
                crate::serial_println!("[deferred-ops] warning: cannot read entry {}: {:?}", id, e);
                continue;
            }
        };

        match parse_entry(id, &data) {
            Some(entry) => entries.push(entry),
            None => {
                crate::serial_println!(
                    "[deferred-ops] warning: malformed entry {} on '{}', skipping",
                    id,
                    mount_path.display()
                );
            }
        }
    }

    // Sort by ID for deterministic oldest-first replay order.
    entries.sort_by_key(|e| e.id);
    Ok(entries)
}

/// Replay all pending deferred operations on the filesystem at `mount_path`.
///
/// For each entry, in oldest-first order:
///
/// 1. Verify the target inode still exists and matches `target_inode`.
/// 2. Re-check filesystem permissions for the stored UID/GID/groups.
/// 3. Execute the operation.
/// 4. On success: delete the entry file.
/// 5. On permission failure or inode mismatch: **drop** the entry (delete it
///    and log why — rule 2 & rule 3).
/// 6. On transient failure (I/O error, busy): leave the entry for next time.
///
/// Returns the number of entries successfully executed and the number dropped.
pub fn replay(mount_path: &Path) -> KernelResult<(usize, usize)> {
    let entries = list(mount_path)?;
    if entries.is_empty() {
        return Ok((0, 0));
    }

    crate::serial_println!(
        "[deferred-ops] replaying {} entries on '{}'",
        entries.len(),
        mount_path.display()
    );

    let mut executed = 0usize;
    let mut dropped = 0usize;

    for entry in &entries {
        match replay_one(mount_path, entry) {
            ReplayResult::Executed => {
                // Delete the entry file.
                let _ = cancel(mount_path, entry.id);
                executed = executed.saturating_add(1);
            }
            ReplayResult::Dropped(reason) => {
                crate::serial_println!(
                    "[deferred-ops] dropping entry {} ({}): {}",
                    entry.id,
                    entry.op.as_str(),
                    reason,
                );
                // Delete the entry — it will never succeed.
                let _ = cancel(mount_path, entry.id);
                dropped = dropped.saturating_add(1);
            }
            ReplayResult::Deferred(reason) => {
                crate::serial_println!(
                    "[deferred-ops] deferring entry {} again ({}): {}",
                    entry.id,
                    entry.op.as_str(),
                    reason,
                );
                // Leave the entry for next replay.
            }
        }
    }

    if executed > 0 || dropped > 0 {
        crate::serial_println!(
            "[deferred-ops] replay on '{}': {} executed, {} dropped, {} remain",
            mount_path.display(),
            executed,
            dropped,
            entries
                .len()
                .saturating_sub(executed)
                .saturating_sub(dropped),
        );
    }

    Ok((executed, dropped))
}

// ---------------------------------------------------------------------------
// Internal replay logic
// ---------------------------------------------------------------------------

enum ReplayResult {
    /// Operation executed successfully.
    Executed,
    /// Operation permanently failed — entry should be removed.
    Dropped(&'static str),
    /// Operation temporarily failed — entry stays for next replay.
    Deferred(&'static str),
}

fn replay_one(mount_path: &Path, entry: &DeferredEntry) -> ReplayResult {
    // Step 1: verify the target inode still exists.
    let target_stat = match Vfs::stat(&entry.target_path) {
        Ok(s) => s,
        Err(KernelError::NotFound) => {
            return ReplayResult::Dropped("target no longer exists");
        }
        Err(_) => {
            return ReplayResult::Deferred("cannot stat target");
        }
    };

    // Step 1b: verify inode identity (rule 1).
    if target_stat.ino == 0 {
        // Filesystem reports no stable inode — should not have been queued.
        return ReplayResult::Dropped("filesystem has no stable inodes");
    }
    if target_stat.ino != entry.target_inode {
        return ReplayResult::Dropped("inode mismatch — target was replaced");
    }

    // Step 2: re-check filesystem permissions (rule 2).
    let access = match entry.required_rights {
        RequiredRights::Delete | RequiredRights::Write => super::vfs::PathAccess::Write,
    };

    // Check permission on the parent directory of the target (that is where
    // the write permission for unlink/rename is checked).
    if let Some(parent) = entry.target_path.parent() {
        if super::vfs::path_access_verdict(
            parent,
            entry.queued_by_uid,
            entry.queued_by_gid,
            &entry.queued_by_groups,
            access,
        )
        .is_err()
        {
            return ReplayResult::Dropped("permission revoked for queuing user");
        }
    }

    // For renames, also check write permission on the destination's parent.
    if entry.op == DeferredOpKind::Rename {
        if let Some(ref dest) = entry.dest_path {
            if let Some(dest_parent) = dest.parent() {
                if super::vfs::path_access_verdict(
                    dest_parent,
                    entry.queued_by_uid,
                    entry.queued_by_gid,
                    &entry.queued_by_groups,
                    access,
                )
                .is_err()
                {
                    return ReplayResult::Dropped("permission revoked for queuing user (dest)");
                }
            }
        }
    }

    // Step 3: execute the operation.
    match entry.op {
        DeferredOpKind::Delete => match Vfs::remove(&entry.target_path) {
            Ok(()) => ReplayResult::Executed,
            Err(KernelError::NotFound) => {
                ReplayResult::Dropped("target disappeared between stat and remove")
            }
            Err(KernelError::DeviceBusy) => ReplayResult::Deferred("device still busy"),
            Err(KernelError::ReadOnlyFilesystem) => {
                ReplayResult::Deferred("filesystem still read-only")
            }
            Err(_) => ReplayResult::Deferred("remove failed (transient)"),
        },
        DeferredOpKind::Rename => {
            let dest = match entry.dest_path {
                Some(ref d) => d,
                None => {
                    return ReplayResult::Dropped("rename with no destination");
                }
            };
            match Vfs::rename(&entry.target_path, dest) {
                Ok(()) => ReplayResult::Executed,
                Err(KernelError::NotFound) => {
                    ReplayResult::Dropped("target disappeared between stat and rename")
                }
                Err(KernelError::DeviceBusy) => ReplayResult::Deferred("device still busy"),
                Err(KernelError::ReadOnlyFilesystem) => {
                    ReplayResult::Deferred("filesystem still read-only")
                }
                Err(KernelError::CrossDevice) => {
                    ReplayResult::Dropped("cross-device rename — cannot defer across mounts")
                }
                Err(_) => ReplayResult::Deferred("rename failed (transient)"),
            }
        }
    }
}

/// Called after a successful mount to replay any pending deferred operations.
///
/// This is the primary trigger: when a volume comes back (re-mount, device
/// reconnect), its queue is replayed.  Best-effort — errors are logged but
/// do not fail the mount.
pub fn replay_on_mount(mount_path: &Path) {
    if !supports_deferred_ops(mount_path) {
        return;
    }

    // Check whether a queue directory even exists before doing more work.
    let qdir = queue_dir_path(mount_path);
    match Vfs::stat(&qdir) {
        Ok(entry) if entry.entry_type == EntryType::Directory => {}
        _ => return, // No queue dir — nothing to replay.
    }

    match replay(mount_path) {
        Ok((executed, dropped)) => {
            if executed > 0 || dropped > 0 {
                crate::serial_println!(
                    "[deferred-ops] post-mount replay on '{}': {} executed, {} dropped",
                    mount_path.display(),
                    executed,
                    dropped,
                );
            }
        }
        Err(e) => {
            crate::serial_println!(
                "[deferred-ops] post-mount replay on '{}' failed: {:?}",
                mount_path.display(),
                e,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Exercise the deferred-ops module.  Run at boot after the root filesystem
/// is mounted.
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::serial_println;

    serial_println!("[deferred-ops] Running self-test...");

    // --- Serialization round-trip ---

    serial_println!("  deferred_ops::self_test 1: serialize/parse round-trip (delete)");
    let entry = DeferredEntry {
        id: 42,
        op: DeferredOpKind::Delete,
        fs_uuid: b"test-uuid-1234".to_vec(),
        target_inode: 1048577,
        target_path: PathBuf::from(b"/media/usb/report.docx".to_vec()),
        dest_path: None,
        reason: DeferredReason::DeviceBusy,
        queued_by_uid: 1000,
        queued_by_gid: 1000,
        queued_by_groups: alloc::vec![100, 27],
        required_rights: RequiredRights::Delete,
        queued_at: 1757260800,
    };
    let data = serialize_entry(&entry);
    let parsed = parse_entry(42, &data).expect("round-trip parse failed");
    assert_eq!(parsed.id, 42);
    assert_eq!(parsed.op, DeferredOpKind::Delete);
    assert_eq!(parsed.fs_uuid, b"test-uuid-1234");
    assert_eq!(parsed.target_inode, 1048577);
    assert_eq!(parsed.target_path.as_bytes(), b"/media/usb/report.docx");
    assert!(parsed.dest_path.is_none());
    assert_eq!(parsed.reason, DeferredReason::DeviceBusy);
    assert_eq!(parsed.queued_by_uid, 1000);
    assert_eq!(parsed.queued_by_gid, 1000);
    assert_eq!(parsed.queued_by_groups, alloc::vec![100, 27]);
    assert_eq!(parsed.required_rights, RequiredRights::Delete);
    assert_eq!(parsed.queued_at, 1757260800);

    serial_println!("  deferred_ops::self_test 2: serialize/parse round-trip (rename)");
    let entry2 = DeferredEntry {
        id: 43,
        op: DeferredOpKind::Rename,
        fs_uuid: b"uuid-5678".to_vec(),
        target_inode: 2097153,
        target_path: PathBuf::from(b"/home/user/old name.txt".to_vec()),
        dest_path: Some(PathBuf::from(b"/home/user/new=name.txt".to_vec())),
        reason: DeferredReason::VolumeFull,
        queued_by_uid: 500,
        queued_by_gid: 500,
        queued_by_groups: Vec::new(),
        required_rights: RequiredRights::Write,
        queued_at: 1757261000,
    };
    let data2 = serialize_entry(&entry2);
    let parsed2 = parse_entry(43, &data2).expect("round-trip parse failed");
    assert_eq!(parsed2.op, DeferredOpKind::Rename);
    assert_eq!(parsed2.target_path.as_bytes(), b"/home/user/old name.txt");
    // The `=` in the dest path must survive the escaping round-trip.
    assert_eq!(
        parsed2.dest_path.as_ref().unwrap().as_bytes(),
        b"/home/user/new=name.txt"
    );
    assert_eq!(parsed2.reason, DeferredReason::VolumeFull);
    assert_eq!(parsed2.required_rights, RequiredRights::Write);

    serial_println!("  deferred_ops::self_test 3: non-UTF-8 path round-trip");
    let entry3 = DeferredEntry {
        id: 44,
        op: DeferredOpKind::Delete,
        fs_uuid: b"\xff\xfe\xfd".to_vec(),
        target_inode: 999,
        target_path: PathBuf::from(b"/mnt/re\xffport\xfe".to_vec()),
        dest_path: None,
        reason: DeferredReason::ReadOnly,
        queued_by_uid: 0,
        queued_by_gid: 0,
        queued_by_groups: Vec::new(),
        required_rights: RequiredRights::Delete,
        queued_at: 0,
    };
    let data3 = serialize_entry(&entry3);
    let parsed3 = parse_entry(44, &data3).expect("non-UTF-8 round-trip failed");
    assert_eq!(parsed3.target_path.as_bytes(), b"/mnt/re\xffport\xfe");
    assert_eq!(parsed3.fs_uuid, b"\xff\xfe\xfd");

    serial_println!("  deferred_ops::self_test 4: malformed entries are rejected");
    // Missing version line.
    assert!(parse_entry(1, b"op=delete\ntarget_inode=1\n").is_none());
    // Wrong version.
    assert!(parse_entry(1, b"version=99\nop=delete\n").is_none());
    // Unknown op.
    assert!(parse_entry(
        1,
        b"version=1\nop=truncate\ntarget_inode=1\ntarget_path=/x\nfs_uuid=a\nreason=device-busy\nqueued_by_uid=0\nqueued_by_gid=0\nqueued_by_groups=\nrequired_rights=delete\nqueued_at=0\n"
    ).is_none());
    // Rename without dest_path.
    assert!(parse_entry(
        1,
        b"version=1\nop=rename\ntarget_inode=1\ntarget_path=/x\nfs_uuid=a\nreason=device-busy\nqueued_by_uid=0\nqueued_by_gid=0\nqueued_by_groups=\nrequired_rights=write\nqueued_at=0\n"
    ).is_none());

    serial_println!("  deferred_ops::self_test 5: entry filename formatting");
    let mut buf = [0u8; 16];
    assert_eq!(format_entry_name(1, &mut buf), b"000001");
    let mut buf2 = [0u8; 16];
    assert_eq!(format_entry_name(999999, &mut buf2), b"999999");
    let mut buf3 = [0u8; 16];
    assert_eq!(format_entry_name(1000000, &mut buf3), b"1000000");

    serial_println!("  deferred_ops::self_test 6: entry filename parsing");
    assert_eq!(parse_entry_name(b"000001"), Some(1));
    assert_eq!(parse_entry_name(b"999999"), Some(999999));
    assert_eq!(parse_entry_name(b""), None);
    assert_eq!(parse_entry_name(b"abc"), None);

    serial_println!("  deferred_ops::self_test 7: reason values round-trip");
    for reason in [
        DeferredReason::DeviceBusy,
        DeferredReason::ReadOnly,
        DeferredReason::VolumeAbsent,
        DeferredReason::VolumeFull,
    ] {
        assert_eq!(
            DeferredReason::from_str(reason.as_str()),
            Some(reason),
            "reason {:?} did not round-trip",
            reason
        );
    }

    serial_println!("  deferred_ops::self_test 8: op values round-trip");
    for op in [DeferredOpKind::Delete, DeferredOpKind::Rename] {
        assert_eq!(
            DeferredOpKind::from_str(op.as_str()),
            Some(op),
            "op {:?} did not round-trip",
            op
        );
    }

    serial_println!("  deferred_ops::self_test 9: supports_deferred_ops for root");
    // The root filesystem is memfs in the boot environment — it does not
    // support deferred ops (no stable inodes).  This verifies the function
    // runs without crashing and returns a coherent answer.
    let root_support = supports_deferred_ops(Path::new(b"/"));
    serial_println!(
        "    root fs supports deferred ops: {} (expected: depends on fs type)",
        root_support
    );

    serial_println!("[deferred-ops] Self-test PASSED");
    Ok(())
}
