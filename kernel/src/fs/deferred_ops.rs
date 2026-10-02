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
//! - A **full volume** — the filesystem is too full to accept a trash rename
//!   and the user prefers to defer rather than permanently delete.
//!
//! An **absent** volume cannot be queued at all: with it gone there is no file
//! to identify the target by (rule 1 below), so the request fails as the path
//! does -- `NotFound` -- before any question of where to keep it.
//!
//! # Where the entries are kept
//!
//! On the volume the operation is about, at `<mount>/.deferred-ops/`, when it
//! can take them: a queue then travels with its volume and is replayed on
//! whatever machine mounts it next.  When it cannot -- read-only or full, two
//! of the three reasons to defer at all -- on the system volume, under
//! `/var/lib/deferred-ops/<the volume's UUID>/`.  Mounting the volume, or
//! remounting it read-write, replays both.  An id is allocated over both
//! places, so it names one entry of a volume wherever that entry is kept.
//!
//! One file per entry, named by its id (`000001`, `000002`, …).  Each is a
//! key=value text record, one key per line, paths octal-escaped via
//! [`super::escape`] with `=` as an extra delimiter so that both the key
//! separator and the line separator are always encoded:
//!
//! ```text
//! version=1
//! op=delete
//! fs_uuid=6f1c0b9e-3c1d-4f0e-9a51-2a7e33c04d11
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
//! For renames an additional `dest_path=…` key is present.  `fs_uuid` is the
//! volume's UUID as `blkid` prints it ([`super::vfs::FileSystem::volume_uuid`]).
//!
//! # Security model
//!
//! 1. **Identity, not path.**  `fs_uuid` + `target_inode` decide what is acted
//!    on; `target_path` says where to look.  A replay acts only when that
//!    name, not followed, still leads to that inode **on the volume being
//!    replayed** -- so an entry reaches nothing beyond its own volume, whatever
//!    it names.
//! 2. **Re-authorise at execution.**  The stored UID/GID/groups are re-checked
//!    against the permission gate (`vfs::path_access_verdict`) and the
//!    `chattr` marks when the operation *runs*.  Refused → the entry is
//!    **dropped**, not retried.
//! 3. **Never escalate a denial.**  The same checks run when the operation is
//!    queued, so a request the user could not perform is refused while they
//!    can still read the answer.  There is no `permission-denied` reason.
//! 4. **Only the kernel writes an entry.**  The kernel enforces no mode bits
//!    and lets anyone `chown` (known-issues
//!    `A-DEFERRED-OPS-ENTRIES-ARE-READABLE-BY-ANYONE`), so neither an entry
//!    file's permissions nor its owner says who wrote it.  What does is the
//!    immutable mark, which only root may set (`attr_policy`): the kernel
//!    seals each entry it writes and checks, once sealed, that the file says
//!    what it wrote.  Replay, the listing and cancellation honour sealed
//!    entries only; an unsealed file in a queue directory is reported and
//!    left alone.  On a removable volume a forger with the disk in another
//!    machine can seal what they like -- and, by rule 1, reach nothing but
//!    that disk.
//!
//! # Doors
//!
//! `SYS_FS_DEFER` (1126) queues, `SYS_FS_DEFER_LIST` (1127) lists a volume's
//! entries -- the caller's own, or every one for root -- and
//! `SYS_FS_DEFER_CANCEL` (1128) cancels one, for whoever queued it or root.
//! See `requests/b-ade-deferred-ops-needs-a-syscall-and-a-queue-that-can-live-off-the-volume.md`
//! and `design-decisions.md` §1529.

use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::escape::{escape_octal, unescape_octal};
use crate::fs::path::{Path, PathBuf};
use crate::fs::vfs::{EntryType, FileAttr, FileId, PathAccess, Vfs, VolumeInfo};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// The directory on each volume that holds its queue, at the volume's root.
/// Relative: `PathBuf::push` replaces the path with an absolute one, which is
/// how every volume's queue once came to be the root volume's.
const QUEUE_DIR: &[u8] = b".deferred-ops";

/// Where the system volume keeps the entries of volumes that could not take
/// their own, one directory per volume UUID.
const SYSTEM_QUEUE_ROOT: &[u8] = b"/var/lib/deferred-ops";

/// The directories above [`SYSTEM_QUEUE_ROOT`], made (0755) if missing.
const SYSTEM_QUEUE_PARENTS: &[&[u8]] = &[b"/var", b"/var/lib"];

/// Entry format version.
const FORMAT_VERSION: u32 = 1;

/// Maximum number of entries per volume, over both places.  Enqueue fails
/// with `ResourceExhausted` at this many: a runaway loop cannot fill a disk
/// with queue files.
const MAX_QUEUE_ENTRIES: usize = 4096;

/// Extra bytes to escape beyond the default set.  `=` is the key/value
/// separator, so it must be encoded inside both keys and values.
const ESCAPE_EXTRA: &[u8] = b"=";

/// The largest entry file read.  The kernel's biggest is two paths of up to
/// `PATH_MAX` (4096) bytes, each byte at most four once escaped, and a few
/// hundred more: a larger file is not one it wrote, and is not read at all --
/// on a removable volume anyone may have made it.
const MAX_ENTRY_BYTES: u64 = 40 * 1024;

/// How many ids storing one entry tries before giving up: each failed try is
/// another writer having taken the id first, or a file altered between its
/// writing and its sealing.
const MAX_STORE_ATTEMPTS: u32 = 8;

/// Filesystems whose inode numbers stay with a file for its life, so that an
/// entry can name its target by number (rule 1).
const STABLE_INODE_FILESYSTEMS: &[&str] = &["ext4", "btrfs", "f2fs", "ntfs", "xfs", "zfs"];

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// The operation to defer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeferredOpKind {
    /// Delete the target (equivalent to `unlink`, or `rmdir` for a directory).
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

    /// The operation as `SYS_FS_DEFER` numbers it: 1 delete, 2 rename.
    #[must_use]
    pub const fn from_code(code: u64) -> Option<Self> {
        match code {
            1 => Some(Self::Delete),
            2 => Some(Self::Rename),
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
    /// The target volume is full and cannot accept a trash rename.
    VolumeFull,
}

impl DeferredReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::DeviceBusy => "device-busy",
            Self::ReadOnly => "read-only",
            Self::VolumeFull => "volume-full",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "device-busy" => Some(Self::DeviceBusy),
            "read-only" => Some(Self::ReadOnly),
            "volume-full" => Some(Self::VolumeFull),
            _ => None,
        }
    }

    /// The reason as `SYS_FS_DEFER` numbers it: 1 busy, 2 read-only, 3 full.
    /// There is no number for an absent volume (see the module doc).
    #[must_use]
    pub const fn from_code(code: u64) -> Option<Self> {
        match code {
            1 => Some(Self::DeviceBusy),
            2 => Some(Self::ReadOnly),
            3 => Some(Self::VolumeFull),
            _ => None,
        }
    }
}

/// The required rights for the deferred operation.  Checked against the
/// permission gate when queued and again at replay.
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
    /// UUID of the volume this entry belongs to, in its text form.
    pub fs_uuid: Vec<u8>,
    /// Inode number of the target — the authority for *what* to act on.
    pub target_inode: u64,
    /// Where the target was when queued, as the host spells it: where replay
    /// looks for it, never the authority on what it is.
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

/// An entry as found in a queue: what it says, and the file that holds it.
#[derive(Debug, Clone)]
pub struct StoredEntry {
    /// The entry.
    pub entry: DeferredEntry,
    /// The sealed file it was read from.
    pub file: PathBuf,
}

/// Who asks for a deferred operation: the identity it is checked against when
/// it is queued, and again when it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requester {
    /// User id.
    pub uid: u32,
    /// Primary group id.
    pub gid: u32,
    /// Supplementary group ids.
    pub groups: Vec<u32>,
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
// Entry file names
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Where a volume's entries are
// ---------------------------------------------------------------------------

/// A volume's UUID as `blkid` prints it: 36 bytes of lower-case hex in the
/// 8-4-4-4-12 groups.  The form its entries are filed under and record.
fn uuid_text(uuid: &[u8; 16]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = Vec::with_capacity(36);
    for (i, &b) in uuid.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            out.push(b'-');
        }
        out.push(HEX.get(usize::from(b >> 4)).copied().unwrap_or(b'0'));
        out.push(HEX.get(usize::from(b & 0x0f)).copied().unwrap_or(b'0'));
    }
    out
}

/// Whether operations can be deferred on `volume`: a filesystem whose inode
/// numbers stay with their files (rule 1), with a UUID to file its entries
/// under when it cannot keep them itself.
#[must_use]
pub fn supports_deferred_ops(volume: &VolumeInfo) -> bool {
    volume.uuid.is_some() && STABLE_INODE_FILESYSTEMS.contains(&volume.fs_type.as_str())
}

/// The queue directory on the volume mounted at `mount`.
fn volume_queue_dir(mount: &Path) -> PathBuf {
    mount.join(Path::new(QUEUE_DIR))
}

/// The system volume's directory for the volume whose UUID text is `uuid`.
fn system_queue_dir(uuid: &[u8]) -> PathBuf {
    Path::new(SYSTEM_QUEUE_ROOT).join(Path::new(uuid))
}

/// Where `volume`'s entries may be, in the order a new one is tried: on the
/// volume, then on the system volume -- unless it *is* the system volume,
/// which has the one place.
fn queue_dirs(volume: &VolumeInfo, uuid: &[u8]) -> Vec<PathBuf> {
    let mut dirs = alloc::vec![volume_queue_dir(&volume.mount)];
    if volume.mount.as_bytes() != b"/" {
        dirs.push(system_queue_dir(uuid));
    }
    dirs
}

// ---------------------------------------------------------------------------
// Reading a queue
// ---------------------------------------------------------------------------

/// Add to `out` every entry file the kernel wrote in `dir`: a regular file
/// named by a number, sealed (rule 4), of a size the kernel could have
/// written.  Anything else is reported and left alone.  A missing directory
/// holds none; so does anything that is not a directory of its own -- a
/// symlink would let whoever made it choose what the kernel reads.
///
/// Names and sizes only: an entry is read when it is wanted
/// ([`load_entry`]), so a queue of thousands costs its file names, not its
/// contents, to walk.
fn sealed_entry_files(dir: &Path, out: &mut Vec<(u64, PathBuf)>) -> KernelResult<()> {
    match Vfs::lmetadata(dir) {
        Ok(m) if m.entry_type == EntryType::Directory => {}
        Ok(_) => {
            crate::serial_println!(
                "[deferred-ops] '{}' is not a directory, so it holds no entries",
                dir.display()
            );
            return Ok(());
        }
        Err(KernelError::NotFound) => return Ok(()),
        Err(e) => return Err(e),
    }
    for de in Vfs::readdir(dir)? {
        let Some(id) = parse_entry_name(de.name.as_bytes()) else {
            continue;
        };
        let file = dir.join(&de.name);
        let meta = match Vfs::lmetadata(&file) {
            Ok(m) => m,
            Err(e) => {
                crate::serial_println!(
                    "[deferred-ops] cannot look at '{}': {:?}",
                    file.display(),
                    e
                );
                continue;
            }
        };
        if meta.entry_type != EntryType::File || !meta.attributes.contains(FileAttr::IMMUTABLE) {
            crate::serial_println!(
                "[deferred-ops] ignoring '{}': not sealed, so not written by the kernel",
                file.display()
            );
            continue;
        }
        if meta.size > MAX_ENTRY_BYTES {
            crate::serial_println!(
                "[deferred-ops] ignoring '{}': {} bytes, more than any entry the kernel writes",
                file.display(),
                meta.size
            );
            continue;
        }
        out.push((id, file));
    }
    Ok(())
}

/// The entry files of `volume` -- whose UUID text is `uuid` -- from both its
/// places, oldest first.  A place that cannot be read is reported and passed
/// over: it is no reason to lose sight of the other.
fn entry_files(volume: &VolumeInfo, uuid: &[u8]) -> Vec<(u64, PathBuf)> {
    let mut out = Vec::new();
    for dir in queue_dirs(volume, uuid) {
        if let Err(e) = sealed_entry_files(&dir, &mut out) {
            crate::serial_println!(
                "[deferred-ops] cannot read the queue '{}': {:?}",
                dir.display(),
                e
            );
        }
    }
    out.sort_by_key(|&(id, _)| id);
    out
}

/// The entry in `file`, if it reads, parses and is for the volume whose UUID
/// text is `uuid`; otherwise `None`, after saying why.
fn load_entry(id: u64, file: &Path, uuid: &[u8]) -> Option<DeferredEntry> {
    let data = match Vfs::read_file(file) {
        Ok(d) => d,
        Err(e) => {
            crate::serial_println!("[deferred-ops] cannot read '{}': {:?}", file.display(), e);
            return None;
        }
    };
    match parse_entry(id, &data) {
        Some(entry) if entry.fs_uuid.as_slice() == uuid => Some(entry),
        Some(_) => {
            crate::serial_println!(
                "[deferred-ops] ignoring '{}': it names another volume",
                file.display()
            );
            None
        }
        None => {
            crate::serial_println!("[deferred-ops] ignoring '{}': malformed", file.display());
            None
        }
    }
}

/// Call `f` with each entry of `volume`, oldest first, until it fails.
fn for_each_entry(
    volume: &VolumeInfo,
    mut f: impl FnMut(StoredEntry) -> KernelResult<()>,
) -> KernelResult<()> {
    let Some(uuid) = volume.uuid.as_ref() else {
        return Ok(());
    };
    let uuid = uuid_text(uuid);
    for (id, file) in entry_files(volume, &uuid) {
        if let Some(entry) = load_entry(id, &file, &uuid) {
            f(StoredEntry { entry, file })?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Writing an entry
// ---------------------------------------------------------------------------

/// `mkdir`, taking a directory already there -- a racing writer's -- as
/// success.  With `own`, it must be a directory itself, not a symlink to one.
fn make_dir(dir: &Path, mode: u16, own: bool) -> KernelResult<()> {
    match Vfs::mkdir_mode(dir, mode) {
        Ok(()) => Ok(()),
        Err(KernelError::AlreadyExists) => {
            let meta = if own {
                Vfs::lmetadata(dir)?
            } else {
                Vfs::metadata(dir)?
            };
            if meta.entry_type == EntryType::Directory {
                Ok(())
            } else {
                Err(KernelError::NotADirectory)
            }
        }
        Err(e) => Err(e),
    }
}

/// Make the queue directory `dir` if it is missing -- with, for the system
/// volume's, the directories above it -- and refuse it if it is anything but
/// a directory of its own: a symlink would choose where the kernel writes.
fn ensure_queue_dir(dir: &Path) -> KernelResult<()> {
    if dir.starts_with(Path::new(SYSTEM_QUEUE_ROOT)) {
        for parent in SYSTEM_QUEUE_PARENTS {
            make_dir(Path::new(parent), 0o755, false)?;
        }
        make_dir(Path::new(SYSTEM_QUEUE_ROOT), 0o700, true)?;
    }
    make_dir(dir, 0o700, true)
}

/// One past the highest id in any of `dirs`, so an id names one entry of a
/// volume wherever it is kept.
fn next_id(dirs: &[PathBuf]) -> KernelResult<u64> {
    let mut max_id = 0u64;
    let mut count = 0usize;
    for dir in dirs {
        let entries = match Vfs::readdir(dir) {
            Ok(e) => e,
            Err(KernelError::NotFound) => continue,
            Err(e) => return Err(e),
        };
        for de in &entries {
            if let Some(id) = parse_entry_name(de.name.as_bytes()) {
                max_id = max_id.max(id);
                count = count.saturating_add(1);
            }
        }
    }
    if count >= MAX_QUEUE_ENTRIES {
        return Err(KernelError::ResourceExhausted);
    }
    max_id.checked_add(1).ok_or(KernelError::ResourceExhausted)
}

/// Write all of `data` through the handle `h`.
fn write_all(h: u64, data: &[u8]) -> KernelResult<()> {
    let mut rest = data;
    while !rest.is_empty() {
        let n = crate::fs::handle::write(h, rest)?;
        if n == 0 {
            return Err(KernelError::IoError);
        }
        rest = rest.get(n..).unwrap_or(&[]);
    }
    Ok(())
}

/// Clear an entry file's seal and remove it.
fn unseal_and_remove(file: &Path) -> KernelResult<()> {
    Vfs::set_attributes(file, FileAttr::NONE)?;
    Vfs::remove(file)
}

/// Write `data` as the entry file `dir/<id>`, created here and nowhere else
/// (`O_CREAT|O_EXCL`), then seal it and check that the sealed file says what
/// was written (rule 4).
///
/// `Ok(None)` when the id was taken, or the file was altered before it was
/// sealed: the caller tries another.
fn write_sealed(dir: &Path, id: u64, data: &[u8]) -> KernelResult<Option<PathBuf>> {
    use crate::fs::handle::{self, OpenFlags};
    let mut name = [0u8; 16];
    let file = dir.join(Path::new(format_entry_name(id, &mut name)));
    let flags = OpenFlags::WRITE
        .union(OpenFlags::CREATE)
        .union(OpenFlags::EXCL);
    let h = match handle::open_with_mode(&file, flags, 0o600) {
        Ok(h) => h,
        Err(KernelError::AlreadyExists) => return Ok(None),
        Err(e) => return Err(e),
    };
    let written = write_all(h, data);
    // Closed whatever the write did: an open handle would keep the file held.
    let closed = handle::close(h);
    let sealed = written
        .and(closed)
        .and_then(|()| Vfs::set_attributes(&file, FileAttr::IMMUTABLE));
    if let Err(e) = sealed {
        // Whatever failed, the file must not stay half-made. If even this
        // fails, what stays is an unsealed file, which every reader ignores
        // (rule 4) -- so the error worth returning is the first.
        let _ = Vfs::set_attributes(&file, FileAttr::NONE);
        let _ = Vfs::remove(&file);
        return Err(e);
    }
    match Vfs::read_file(&file) {
        Ok(back) if back == data => Ok(Some(file)),
        _ => {
            crate::serial_println!(
                "[deferred-ops] '{}' changed before it was sealed; discarded",
                file.display()
            );
            // As above: left behind, it would be an entry nobody wrote, and
            // its id is taken -- the caller moves on to the next either way.
            let _ = unseal_and_remove(&file);
            Ok(None)
        }
    }
}

/// Keep `entry` in `dir`, one of `all` (the volume's places, over which the
/// id is allocated).
fn store_in(dir: &Path, all: &[PathBuf], entry: &mut DeferredEntry) -> KernelResult<u64> {
    ensure_queue_dir(dir)?;
    for _ in 0..MAX_STORE_ATTEMPTS {
        let id = next_id(all)?;
        entry.id = id;
        if write_sealed(dir, id, &serialize_entry(entry))?.is_some() {
            return Ok(id);
        }
    }
    Err(KernelError::ResourceExhausted)
}

/// Keep `entry` for `volume` in the first of its places that can take it,
/// going on to the next when one refuses for a reason deferring exists for:
/// read-only, full, busy.
///
/// As the kernel (`proc::thread::as_kernel`): every path here is the
/// kernel's own choosing.
fn store(volume: &VolumeInfo, uuid: &[u8], entry: &mut DeferredEntry) -> KernelResult<u64> {
    let dirs = queue_dirs(volume, uuid);
    let mut last = KernelError::NotSupported;
    for dir in &dirs {
        match store_in(dir, &dirs, entry) {
            Ok(id) => return Ok(id),
            Err(
                e @ (KernelError::ReadOnlyFilesystem
                | KernelError::DiskFull
                | KernelError::DeviceBusy),
            ) => last = e,
            Err(e) => return Err(e),
        }
    }
    Err(last)
}

// ---------------------------------------------------------------------------
// Rights (rules 2 and 3)
// ---------------------------------------------------------------------------

/// Whether `who` may do now what `found` and `dest` describe, by what a real
/// `unlink`, `rmdir` or `rename` meets: the permission gate (`Write`) on the
/// file and on each directory whose names change, and the `chattr` marks.
///
/// The gate is asked through `path_access_verdict`, with `who`'s identity
/// given: whoever is running this -- the caller queueing, or the kernel
/// replaying -- is not who the operation is for.
fn may_run(
    found: &crate::fs::vfs::DeferralTarget,
    dest: Option<&Path>,
    who: &Requester,
) -> KernelResult<()> {
    use crate::fs::vfs::path_access_verdict;
    let verdict =
        |path: &Path| path_access_verdict(path, who.uid, who.gid, &who.groups, PathAccess::Write);
    verdict(&found.path)?;
    verdict(&found.parent)?;
    crate::fs::attr_policy::may_delete(found.parent_attributes, found.attributes)?;
    if let Some(dest) = dest {
        let dest_dir = dest.parent().unwrap_or(Path::new("/"));
        verdict(dest)?;
        verdict(dest_dir)?;
        // The destination is a host path: read as the kernel, so no
        // namespace is applied to it a second time. A directory that cannot
        // be read takes no names, and the rename will say so itself.
        if let Ok(meta) = crate::proc::thread::as_kernel(|| Vfs::metadata(dest_dir)) {
            crate::fs::attr_policy::may_create(meta.attributes)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Queue `op` on the file `target` names, for `who` -- `SYS_FS_DEFER`'s work,
/// in its caller's context: the paths are resolved as the caller resolves
/// them, and the caller's gate decides whether it may look at them.
///
/// `target`'s final component is not followed (a deferred delete removes the
/// name, as `unlink` does).  `now_secs` is the time to record.
///
/// # Errors
///
/// - `InvalidArgument`: a rename with no destination, or a delete with one;
/// - `NotFound`: no such name -- an absent volume's paths among them;
/// - `NotSupported`: a filesystem without stable inode numbers or a UUID;
/// - `DeviceBusy`: the name is a mount point;
/// - `CrossDevice`: a rename to another volume;
/// - `PermissionDenied` / `NotPermitted`: `who` could not do it even with
///   nothing in the way (rule 3);
/// - `ResourceExhausted`: the volume already has [`MAX_QUEUE_ENTRIES`];
/// - whatever kept both places from taking the entry.
pub fn queue(
    target: &Path,
    op: DeferredOpKind,
    dest: Option<&Path>,
    reason: DeferredReason,
    who: &Requester,
    now_secs: u64,
) -> KernelResult<u64> {
    if matches!(
        (op, dest),
        (DeferredOpKind::Rename, None) | (DeferredOpKind::Delete, Some(_))
    ) {
        return Err(KernelError::InvalidArgument);
    }
    let found = Vfs::deferral_target(target)?;
    let uuid = match found.volume.uuid {
        Some(ref u) if supports_deferred_ops(&found.volume) => uuid_text(u),
        _ => return Err(KernelError::NotSupported),
    };
    let dest = match dest {
        Some(d) => {
            let (host, fs_id) = Vfs::deferral_destination(d)?;
            if fs_id != found.id.fs_id {
                return Err(KernelError::CrossDevice);
            }
            Some(host)
        }
        None => None,
    };
    may_run(&found, dest.as_deref(), who)?;

    let mut entry = DeferredEntry {
        id: 0,
        op,
        fs_uuid: uuid.clone(),
        target_inode: found.id.ino,
        target_path: found.path.clone(),
        dest_path: dest,
        reason,
        queued_by_uid: who.uid,
        queued_by_gid: who.gid,
        queued_by_groups: who.groups.clone(),
        required_rights: match op {
            DeferredOpKind::Delete => RequiredRights::Delete,
            DeferredOpKind::Rename => RequiredRights::Write,
        },
        queued_at: now_secs,
    };
    let id = crate::proc::thread::as_kernel(|| store(&found.volume, &uuid, &mut entry))?;
    crate::serial_println!(
        "[deferred-ops] queued entry {} for '{}': {} '{}' (inode {}, uid {}, {})",
        id,
        found.volume.mount.display(),
        op.as_str(),
        found.path.display(),
        found.id.ino,
        who.uid,
        reason.as_str(),
    );
    Ok(id)
}

/// The entries of the volume `path` is on that `who_uid` may see: their own,
/// or every one for root -- `SYS_FS_DEFER_LIST`'s work.  A target path is a
/// name its queuer may not want shown to others.
///
/// # Errors
///
/// As `path` fails to resolve, or the caller's gate refuses to let it look.
pub fn list(path: &Path, who_uid: u32) -> KernelResult<Vec<StoredEntry>> {
    let volume = Vfs::volume_named_by(path)?;
    let mut entries = Vec::new();
    crate::proc::thread::as_kernel(|| {
        for_each_entry(&volume, |s| {
            if who_uid == 0 || s.entry.queued_by_uid == who_uid {
                entries.push(s);
            }
            Ok(())
        })
    })?;
    Ok(entries)
}

/// [`list`] as `SYS_FS_DEFER_LIST` hands it back: for each entry an `id=`
/// line, its record, and a blank line -- built only as far as `limit` bytes,
/// past which the answer is `BufferTooSmall` and nothing.
///
/// # Errors
///
/// As [`list`]; `BufferTooSmall` past `limit`.
pub fn listing(path: &Path, who_uid: u32, limit: usize) -> KernelResult<Vec<u8>> {
    let volume = Vfs::volume_named_by(path)?;
    let mut out = Vec::new();
    crate::proc::thread::as_kernel(|| {
        for_each_entry(&volume, |s| {
            if who_uid != 0 && s.entry.queued_by_uid != who_uid {
                return Ok(());
            }
            append_listed(&mut out, &s.entry);
            if out.len() > limit {
                return Err(KernelError::BufferTooSmall);
            }
            Ok(())
        })
    })?;
    Ok(out)
}

/// One entry of a listing: its `id=` line, its record, a blank line.
fn append_listed(out: &mut Vec<u8>, entry: &DeferredEntry) {
    let mut head = String::from("id=");
    push_u64(&mut head, entry.id);
    head.push('\n');
    out.extend_from_slice(head.as_bytes());
    out.extend_from_slice(&serialize_entry(entry));
    out.push(b'\n');
}

/// Cancel entry `id` of the volume `path` is on, for `who_uid` -- whoever
/// queued it, or root -- `SYS_FS_DEFER_CANCEL`'s work.
///
/// # Errors
///
/// `NotFound` for no such entry; `NotPermitted` for someone else's; as
/// [`list`] for `path`; whatever stopped the entry's removal.
pub fn cancel(path: &Path, id: u64, who_uid: u32) -> KernelResult<()> {
    let volume = Vfs::volume_named_by(path)?;
    let uuid = volume
        .uuid
        .as_ref()
        .map(uuid_text)
        .ok_or(KernelError::NotFound)?;
    crate::proc::thread::as_kernel(|| {
        let file = entry_files(&volume, &uuid)
            .into_iter()
            .find(|&(found, _)| found == id)
            .map(|(_, file)| file)
            .ok_or(KernelError::NotFound)?;
        let entry = load_entry(id, &file, &uuid).ok_or(KernelError::NotFound)?;
        if who_uid != 0 && entry.queued_by_uid != who_uid {
            return Err(KernelError::NotPermitted);
        }
        unseal_and_remove(&file)?;
        crate::serial_println!(
            "[deferred-ops] cancelled entry {} of '{}'",
            id,
            volume.mount.display()
        );
        Ok(())
    })
}

/// Replay every entry of the volume mounted at `mount_path`, oldest first:
/// run it if the file is still the one queued and its queuer may still do
/// it; drop it for good if not (rules 1 and 2); keep it if the volume still
/// refuses.  Nothing for a mount that cannot carry a queue.
///
/// Returns how many ran and how many were dropped.
///
/// # Errors
///
/// `NotFound` when nothing is mounted over `mount_path`.
pub fn replay(mount_path: &Path) -> KernelResult<(usize, usize)> {
    let volume = Vfs::volume_of(mount_path)?;
    if volume.mount.as_path() != mount_path || !supports_deferred_ops(&volume) {
        return Ok((0, 0));
    }
    Ok(crate::proc::thread::as_kernel(|| replay_volume(&volume)))
}

/// [`replay`]'s work, as the kernel.
fn replay_volume(volume: &VolumeInfo) -> (usize, usize) {
    let Some(uuid) = volume.uuid.as_ref().map(uuid_text) else {
        return (0, 0);
    };
    let mut executed = 0usize;
    let mut dropped = 0usize;
    for (id, file) in entry_files(volume, &uuid) {
        let Some(entry) = load_entry(id, &file, &uuid) else {
            continue;
        };
        let stored = StoredEntry { entry, file };
        let entry = &stored.entry;
        let finished = match replay_one(volume, entry) {
            ReplayResult::Executed => {
                executed = executed.saturating_add(1);
                true
            }
            ReplayResult::Dropped(why) => {
                crate::serial_println!(
                    "[deferred-ops] dropping entry {} ({} '{}'): {}",
                    entry.id,
                    entry.op.as_str(),
                    entry.target_path.display(),
                    why,
                );
                dropped = dropped.saturating_add(1);
                true
            }
            ReplayResult::Deferred(why) => {
                crate::serial_println!(
                    "[deferred-ops] entry {} waits again ({} '{}'): {}",
                    entry.id,
                    entry.op.as_str(),
                    entry.target_path.display(),
                    why,
                );
                false
            }
        };
        if finished && let Err(e) = unseal_and_remove(&stored.file) {
            // The operation is done (or never will be) and its entry stays:
            // the next replay finds the name gone or changed and drops it.
            crate::serial_println!(
                "[deferred-ops] cannot remove finished entry '{}': {:?}",
                stored.file.display(),
                e
            );
        }
    }
    (executed, dropped)
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

/// Run one entry of `volume`, as the kernel.
fn replay_one(volume: &VolumeInfo, entry: &DeferredEntry) -> ReplayResult {
    // Rule 1: the name, not followed, still leads to the queued inode -- on
    // this volume, whatever the entry says.
    let found = match Vfs::deferral_target(&entry.target_path) {
        Ok(f) => f,
        Err(KernelError::NotFound) => return ReplayResult::Dropped("the target no longer exists"),
        Err(KernelError::DeviceBusy) => {
            return ReplayResult::Deferred("something is mounted on the target");
        }
        Err(_) => return ReplayResult::Deferred("cannot look at the target"),
    };
    let queued = FileId {
        fs_id: volume.fs_id,
        ino: entry.target_inode,
    };
    if found.id != queued {
        return ReplayResult::Dropped("the name no longer leads to the queued file on this volume");
    }
    let dest = match (entry.op, entry.dest_path.as_ref()) {
        (DeferredOpKind::Delete, _) => None,
        (DeferredOpKind::Rename, None) => {
            return ReplayResult::Dropped("a rename with no destination");
        }
        (DeferredOpKind::Rename, Some(d)) => match Vfs::deferral_destination(d) {
            Ok((host, fs_id)) if fs_id == volume.fs_id => Some(host),
            Ok(_) => return ReplayResult::Dropped("the destination is not on this volume"),
            Err(KernelError::NotFound) => {
                return ReplayResult::Dropped("the destination's directory no longer exists");
            }
            Err(_) => return ReplayResult::Deferred("cannot look at the destination"),
        },
    };
    // Rule 2: the queuer may still do it.
    let who = Requester {
        uid: entry.queued_by_uid,
        gid: entry.queued_by_gid,
        groups: entry.queued_by_groups.clone(),
    };
    if may_run(&found, dest.as_deref(), &who).is_err() {
        return ReplayResult::Dropped("refused for the user who queued it");
    }
    let result = match dest {
        Some(dest) => Vfs::rename(&found.path, &dest),
        None if found.entry_type == EntryType::Directory => Vfs::rmdir(&found.path),
        None => Vfs::remove(&found.path),
    };
    match result {
        Ok(()) => ReplayResult::Executed,
        Err(KernelError::ReadOnlyFilesystem) => ReplayResult::Deferred("still read-only"),
        Err(KernelError::DeviceBusy) => ReplayResult::Deferred("still busy"),
        Err(KernelError::DiskFull) => ReplayResult::Deferred("still full"),
        Err(KernelError::NotFound) => {
            ReplayResult::Dropped("the target went while it was being acted on")
        }
        Err(KernelError::NotEmpty) => ReplayResult::Dropped("the directory is not empty"),
        Err(KernelError::CrossDevice) => ReplayResult::Dropped("a rename across volumes"),
        Err(KernelError::PermissionDenied | KernelError::NotPermitted) => {
            ReplayResult::Dropped("refused")
        }
        Err(_) => ReplayResult::Deferred("failed; it is tried again next time"),
    }
}

/// Replay the queue of the volume just mounted at `mount_path` -- or just
/// remounted read-write, which is what clears a read-only deferral.  Called by
/// `Vfs::mount_with_options` and `Vfs::remount`; best-effort, as neither may
/// fail for it.
pub fn replay_on_mount(mount_path: &Path) {
    match replay(mount_path) {
        Ok((0, 0)) => {}
        Ok((executed, dropped)) => crate::serial_println!(
            "[deferred-ops] replay on '{}': {} ran, {} dropped",
            mount_path.display(),
            executed,
            dropped,
        ),
        Err(e) => crate::serial_println!(
            "[deferred-ops] replay on '{}' failed: {:?}",
            mount_path.display(),
            e,
        ),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Report a failed check and fail the test.
fn check(ok: bool, what: &str) -> KernelResult<()> {
    if ok {
        Ok(())
    } else {
        crate::serial_println!("[deferred-ops]   FAIL: {}", what);
        Err(KernelError::InternalError)
    }
}

/// Exercise the deferred-ops module.  Run at boot with the system image's
/// ext4 mounted at `/mnt`, which the boot test attaches with `snapshot=on`, so
/// nothing written there outlives the boot.
///
/// # Errors
///
/// `InternalError` (after a `FAIL:` line) for a check that does not hold, or
/// the error that stopped a step.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[deferred-ops] Running self-test...");
    let mut skips = crate::fs::selftest::Skips::new();
    test_format()?;
    match Vfs::volume_of(Path::new("/mnt")) {
        Ok(v) if v.mount.as_bytes() == b"/mnt" && supports_deferred_ops(&v) && !v.read_only => {
            let result = test_on_volume(&v);
            cleanup_volume_test(&v);
            result?;
        }
        _ => skips.record(
            "queue, list, cancel and replay on a volume",
            "no writable ext4 with a UUID at /mnt",
        ),
    }
    skips.report("[deferred-ops]");
    crate::serial_println!("[deferred-ops] Self-test PASSED{}", skips.suffix());
    Ok(())
}

/// The record format, the names, the codes and the places -- no volume
/// needed.
fn test_format() -> KernelResult<()> {
    crate::serial_println!("  deferred_ops::self_test 1: serialize/parse round-trip (delete)");
    let entry = DeferredEntry {
        id: 42,
        op: DeferredOpKind::Delete,
        fs_uuid: b"test-uuid-1234".to_vec(),
        target_inode: 1_048_577,
        target_path: PathBuf::from(b"/media/usb/report.docx".to_vec()),
        dest_path: None,
        reason: DeferredReason::DeviceBusy,
        queued_by_uid: 1000,
        queued_by_gid: 1000,
        queued_by_groups: alloc::vec![100, 27],
        required_rights: RequiredRights::Delete,
        queued_at: 1_757_260_800,
    };
    let parsed = parse_entry(42, &serialize_entry(&entry));
    check(
        parsed.as_ref().is_some_and(|p| {
            p.id == 42
                && p.op == DeferredOpKind::Delete
                && p.fs_uuid == b"test-uuid-1234"
                && p.target_inode == 1_048_577
                && p.target_path.as_bytes() == b"/media/usb/report.docx"
                && p.dest_path.is_none()
                && p.reason == DeferredReason::DeviceBusy
                && p.queued_by_uid == 1000
                && p.queued_by_gid == 1000
                && p.queued_by_groups == [100, 27]
                && p.required_rights == RequiredRights::Delete
                && p.queued_at == 1_757_260_800
        }),
        "a delete entry did not survive serialize/parse",
    )?;

    crate::serial_println!("  deferred_ops::self_test 2: serialize/parse round-trip (rename)");
    let entry2 = DeferredEntry {
        id: 43,
        op: DeferredOpKind::Rename,
        fs_uuid: b"uuid-5678".to_vec(),
        target_inode: 2_097_153,
        target_path: PathBuf::from(b"/home/user/old name.txt".to_vec()),
        dest_path: Some(PathBuf::from(b"/home/user/new=name.txt".to_vec())),
        reason: DeferredReason::VolumeFull,
        queued_by_uid: 500,
        queued_by_gid: 500,
        queued_by_groups: Vec::new(),
        required_rights: RequiredRights::Write,
        queued_at: 1_757_261_000,
    };
    let parsed2 = parse_entry(43, &serialize_entry(&entry2));
    check(
        parsed2.as_ref().is_some_and(|p| {
            p.op == DeferredOpKind::Rename
                && p.target_path.as_bytes() == b"/home/user/old name.txt"
                // The `=` in the dest path must survive the escaping.
                && p.dest_path.as_ref().map(|d| d.as_bytes()) == Some(&b"/home/user/new=name.txt"[..])
                && p.reason == DeferredReason::VolumeFull
                && p.required_rights == RequiredRights::Write
        }),
        "a rename entry did not survive serialize/parse",
    )?;

    crate::serial_println!("  deferred_ops::self_test 3: non-UTF-8 path round-trip");
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
    let parsed3 = parse_entry(44, &serialize_entry(&entry3));
    check(
        parsed3.as_ref().is_some_and(|p| {
            p.target_path.as_bytes() == b"/mnt/re\xffport\xfe" && p.fs_uuid == b"\xff\xfe\xfd"
        }),
        "a non-UTF-8 entry did not survive serialize/parse",
    )?;

    crate::serial_println!("  deferred_ops::self_test 4: malformed entries are rejected");
    let rest = b"target_inode=1\ntarget_path=/x\nfs_uuid=a\nreason=device-busy\nqueued_by_uid=0\nqueued_by_gid=0\nqueued_by_groups=\nqueued_at=0\n";
    let with = |head: &[u8], tail: &[u8]| -> Vec<u8> {
        let mut v = head.to_vec();
        v.extend_from_slice(rest);
        v.extend_from_slice(tail);
        v
    };
    check(
        parse_entry(1, b"op=delete\ntarget_inode=1\n").is_none()
            && parse_entry(1, b"version=99\nop=delete\n").is_none()
            && parse_entry(1, &with(b"version=1\nop=truncate\n", b"required_rights=delete\n")).is_none()
            && parse_entry(1, &with(b"version=1\nop=rename\n", b"required_rights=write\n")).is_none()
            // A reason with no place in the closed set -- the old
            // `volume-absent` among them -- is malformed.
            && parse_entry(
                1,
                b"version=1\nop=delete\ntarget_inode=1\ntarget_path=/x\nfs_uuid=a\nreason=volume-absent\nqueued_by_uid=0\nqueued_by_gid=0\nqueued_by_groups=\nrequired_rights=delete\nqueued_at=0\n",
            )
            .is_none()
            && parse_entry(1, &with(b"version=1\nop=delete\n", b"required_rights=delete\n")).is_some(),
        "the parser took a malformed entry, or refused a sound one",
    )?;

    crate::serial_println!("  deferred_ops::self_test 5: entry file names");
    let (mut a, mut b, mut c) = ([0u8; 16], [0u8; 16], [0u8; 16]);
    check(
        format_entry_name(1, &mut a) == b"000001"
            && format_entry_name(999_999, &mut b) == b"999999"
            && format_entry_name(1_000_000, &mut c) == b"1000000"
            && parse_entry_name(b"000001") == Some(1)
            && parse_entry_name(b"999999") == Some(999_999)
            && parse_entry_name(b"").is_none()
            && parse_entry_name(b"abc").is_none(),
        "an entry's file name did not round-trip",
    )?;

    crate::serial_println!("  deferred_ops::self_test 6: names, and the system call's codes");
    let reasons = [
        DeferredReason::DeviceBusy,
        DeferredReason::ReadOnly,
        DeferredReason::VolumeFull,
    ];
    let ops = [DeferredOpKind::Delete, DeferredOpKind::Rename];
    check(
        reasons
            .iter()
            .all(|&r| DeferredReason::from_str(r.as_str()) == Some(r))
            && ops
                .iter()
                .all(|&o| DeferredOpKind::from_str(o.as_str()) == Some(o))
            && DeferredReason::from_code(1) == Some(DeferredReason::DeviceBusy)
            && DeferredReason::from_code(2) == Some(DeferredReason::ReadOnly)
            && DeferredReason::from_code(3) == Some(DeferredReason::VolumeFull)
            && DeferredReason::from_code(0).is_none()
            && DeferredReason::from_code(4).is_none()
            && DeferredOpKind::from_code(1) == Some(DeferredOpKind::Delete)
            && DeferredOpKind::from_code(2) == Some(DeferredOpKind::Rename)
            && DeferredOpKind::from_code(3).is_none(),
        "a reason or an operation did not round-trip through its name or code",
    )?;

    crate::serial_println!("  deferred_ops::self_test 7: a volume's UUID, its places, and support");
    let uuid = [
        0x6f, 0x1c, 0x0b, 0x9e, 0x3c, 0x1d, 0x4f, 0x0e, 0x9a, 0x51, 0x2a, 0x7e, 0x33, 0xc0, 0x4d,
        0x11,
    ];
    let text = uuid_text(&uuid);
    let usb = VolumeInfo {
        mount: PathBuf::from(b"/media/usb".to_vec()),
        fs_type: alloc::string::String::from("ext4"),
        fs_id: 7,
        read_only: false,
        uuid: Some(uuid),
    };
    let root = VolumeInfo {
        mount: PathBuf::from(b"/".to_vec()),
        ..usb.clone()
    };
    let dirs = queue_dirs(&usb, &text);
    check(
        text == b"6f1c0b9e-3c1d-4f0e-9a51-2a7e33c04d11",
        "a UUID's text is not blkid's",
    )?;
    // The bug this replaced: `push` of an absolute name replaced the mount,
    // so every volume's queue was the root volume's.
    check(
        dirs.len() == 2
            && dirs.first().map(|d| d.as_bytes()) == Some(&b"/media/usb/.deferred-ops"[..])
            && dirs.get(1).map(|d| d.as_bytes())
                == Some(&b"/var/lib/deferred-ops/6f1c0b9e-3c1d-4f0e-9a51-2a7e33c04d11"[..]),
        "a volume's two places are not where they should be",
    )?;
    check(
        queue_dirs(&root, &text).len() == 1,
        "the system volume has a second place, on itself",
    )?;
    let memfs = VolumeInfo {
        fs_type: alloc::string::String::from("memfs"),
        ..usb.clone()
    };
    let no_uuid = VolumeInfo {
        uuid: None,
        ..usb.clone()
    };
    check(
        supports_deferred_ops(&usb)
            && !supports_deferred_ops(&memfs)
            && !supports_deferred_ops(&no_uuid),
        "support is not ext4-with-a-UUID",
    )?;
    Ok(())
}

/// The scratch names [`test_on_volume`] uses.
const T_VICTIM: &str = "/mnt/_dfo_victim";
const T_FROM: &str = "/mnt/_dfo_from";
const T_TO: &str = "/mnt/_dfo_to";
const T_KEEP: &str = "/mnt/_dfo_keep";
const T_MOVED: &str = "/mnt/_dfo_moved";
const T_DIR: &str = "/mnt/_dfo_dir";
const T_GUARDED: &str = "/mnt/_dfo_dir/f";
const T_OTHER: &str = "/tmp/_dfo_other";

/// Take down what [`test_on_volume`] made, whatever it got to: the scratch
/// files, the ACL it set, and every entry naming one of them.
fn cleanup_volume_test(v: &VolumeInfo) {
    // Each step is best-effort: what is not there was never made, and a
    // failure here must not hide the test's own result.
    let _ = crate::fs::acl::remove_acl(T_DIR);
    for p in [T_VICTIM, T_FROM, T_TO, T_KEEP, T_MOVED, T_GUARDED, T_OTHER] {
        let _ = Vfs::remove(p);
    }
    let _ = Vfs::rmdir(T_DIR);
    if let Some(uuid) = v.uuid.as_ref() {
        let text = uuid_text(uuid);
        for dir in queue_dirs(v, &text) {
            let Ok(names) = Vfs::readdir(&dir) else {
                continue;
            };
            for de in names {
                let file = dir.join(&de.name);
                let ours = Vfs::read_file(&file).is_ok_and(|d| {
                    d.windows(10)
                        .any(|w| w == b"/mnt/_dfo_" || w == b"/tmp/_dfo_")
                });
                if ours {
                    let _ = unseal_and_remove(&file);
                }
            }
        }
    }
}

/// Queue, list, cancel and replay on the ext4 volume `v` at `/mnt`.
fn test_on_volume(v: &VolumeInfo) -> KernelResult<()> {
    use crate::fs::acl::{self, AclPerm};
    use crate::fs::vfs::MountOptions;
    let mnt = Path::new("/mnt");
    let u1000 = Requester {
        uid: 1000,
        gid: 1000,
        groups: Vec::new(),
    };
    let uuid = v.uuid.as_ref().map(uuid_text).unwrap_or_default();
    let on_volume = volume_queue_dir(&v.mount);
    let entry_file = |dir: &Path, id: u64| {
        let mut name = [0u8; 16];
        dir.join(Path::new(format_entry_name(id, &mut name)))
    };
    let gone = |p: &Path| matches!(Vfs::lmetadata(p), Err(KernelError::NotFound));
    cleanup_volume_test(v);

    crate::serial_println!("  deferred_ops::self_test 8: queue, list, cancel");
    Vfs::write_file(T_VICTIM, b"x")?;
    let id = queue(
        Path::new(T_VICTIM),
        DeferredOpKind::Delete,
        None,
        DeferredReason::DeviceBusy,
        &u1000,
        1,
    )?;
    let file = entry_file(&on_volume, id);
    check(
        Vfs::lmetadata(&file).is_ok_and(|m| m.attributes.contains(FileAttr::IMMUTABLE)),
        "an entry is not kept, sealed, on its volume",
    )?;
    let seen = |uid: u32| list(mnt, uid).is_ok_and(|l| l.iter().any(|s| s.entry.id == id));
    check(seen(1000), "the queuer does not see their entry")?;
    check(!seen(2000), "another user sees someone else's entry")?;
    check(seen(0), "root does not see every entry")?;
    check(
        cancel(mnt, id, 2000) == Err(KernelError::NotPermitted),
        "another user cancelled someone else's entry",
    )?;
    cancel(mnt, id, 1000)?;
    check(gone(&file), "a cancelled entry is still there")?;
    check(Vfs::stat(T_VICTIM).is_ok(), "cancelling acted on the file")?;
    check(
        queue(
            Path::new(T_VICTIM),
            DeferredOpKind::Rename,
            None,
            DeferredReason::DeviceBusy,
            &u1000,
            1,
        ) == Err(KernelError::InvalidArgument),
        "a rename with no destination was queued",
    )?;

    crate::serial_println!("  deferred_ops::self_test 9: replay deletes and renames");
    let id = queue(
        Path::new(T_VICTIM),
        DeferredOpKind::Delete,
        None,
        DeferredReason::DeviceBusy,
        &u1000,
        2,
    )?;
    Vfs::write_file(T_FROM, b"r")?;
    let id2 = queue(
        Path::new(T_FROM),
        DeferredOpKind::Rename,
        Some(Path::new(T_TO)),
        DeferredReason::ReadOnly,
        &u1000,
        3,
    )?;
    let (ran, _) = replay(mnt)?;
    check(
        ran >= 2 && gone(Path::new(T_VICTIM)),
        "a replay did not delete the queued file",
    )?;
    check(
        gone(Path::new(T_FROM)) && Vfs::read_file(T_TO).is_ok_and(|d| d == b"r"),
        "a replay did not rename the queued file",
    )?;
    check(
        gone(&entry_file(&on_volume, id)) && gone(&entry_file(&on_volume, id2)),
        "a finished entry is still there",
    )?;

    crate::serial_println!("  deferred_ops::self_test 10: a name that now leads elsewhere");
    Vfs::write_file(T_KEEP, b"k")?;
    let id = queue(
        Path::new(T_KEEP),
        DeferredOpKind::Delete,
        None,
        DeferredReason::DeviceBusy,
        &u1000,
        4,
    )?;
    // The queued file moves away and another takes its name.
    Vfs::rename(T_KEEP, T_MOVED)?;
    Vfs::write_file(T_KEEP, b"new")?;
    replay(mnt)?;
    check(
        Vfs::stat(T_KEEP).is_ok() && Vfs::stat(T_MOVED).is_ok(),
        "a replay acted on a file that is not the one queued",
    )?;
    check(
        gone(&entry_file(&on_volume, id)),
        "an entry whose file is gone from its name was not dropped",
    )?;

    crate::serial_println!("  deferred_ops::self_test 11: an unsealed entry is nobody's");
    // Written straight into the queue, as any process can (rule 4): root's,
    // naming a file that exists, and not sealed.
    let keep_ino = Vfs::lmetadata(T_KEEP)?.ino;
    let forged = DeferredEntry {
        id: 0,
        op: DeferredOpKind::Delete,
        fs_uuid: uuid.clone(),
        target_inode: keep_ino,
        target_path: PathBuf::from(T_KEEP.as_bytes().to_vec()),
        dest_path: None,
        reason: DeferredReason::DeviceBusy,
        queued_by_uid: 0,
        queued_by_gid: 0,
        queued_by_groups: Vec::new(),
        required_rights: RequiredRights::Delete,
        queued_at: 5,
    };
    let forged_file = entry_file(&on_volume, 900_000);
    Vfs::write_file(&forged_file, &serialize_entry(&forged))?;
    let listed = list(mnt, 0).is_ok_and(|l| l.iter().any(|s| s.entry.id == 900_000));
    replay(mnt)?;
    let kept = Vfs::stat(T_KEEP).is_ok() && Vfs::stat(&forged_file).is_ok();
    Vfs::remove(&forged_file)?;
    check(!listed, "an unsealed entry was listed")?;
    check(kept, "an unsealed entry was acted on, or removed")?;

    crate::serial_println!("  deferred_ops::self_test 12: an entry reaches nothing off its volume");
    // Sealed, as the kernel seals -- which a forger with this volume in
    // another machine could do as well -- naming a file on /tmp by its own
    // inode number.
    Vfs::write_file(T_OTHER, b"o")?;
    let off = DeferredEntry {
        target_inode: Vfs::lmetadata(T_OTHER)?.ino,
        target_path: PathBuf::from(T_OTHER.as_bytes().to_vec()),
        ..forged
    };
    let sealed = write_sealed(&on_volume, 900_001, &serialize_entry(&off))?;
    check(
        sealed.is_some(),
        "the sealed test entry could not be written",
    )?;
    replay(mnt)?;
    check(
        Vfs::stat(T_OTHER).is_ok(),
        "an entry acted on a file on another volume",
    )?;
    check(
        gone(&entry_file(&on_volume, 900_001)),
        "an entry naming another volume's file was not dropped",
    )?;

    crate::serial_println!("  deferred_ops::self_test 13: rights are checked when queued");
    Vfs::mkdir(T_DIR)?;
    Vfs::write_file(T_GUARDED, b"g")?;
    // Others may read and search the directory but not change its names.
    acl::set_acl(
        T_DIR,
        acl::build_acl(AclPerm::ALL, AclPerm(5), AclPerm(5), &[], &[]),
    )?;
    let u3000 = Requester {
        uid: 3000,
        gid: 3000,
        groups: Vec::new(),
    };
    let refused = queue(
        Path::new(T_GUARDED),
        DeferredOpKind::Delete,
        None,
        DeferredReason::DeviceBusy,
        &u3000,
        6,
    );
    acl::remove_acl(T_DIR);
    check(
        refused == Err(KernelError::PermissionDenied),
        "a delete the gate refuses was queued",
    )?;

    crate::serial_println!(
        "  deferred_ops::self_test 14: a read-only volume's entry waits on the system volume"
    );
    Vfs::write_file(T_VICTIM, b"x")?;
    let opts = Vfs::mount_options(mnt)?;
    Vfs::remount(
        mnt,
        MountOptions {
            read_only: true,
            ..opts
        },
    )?;
    let waiting = (|| -> KernelResult<PathBuf> {
        let id = queue(
            Path::new(T_VICTIM),
            DeferredOpKind::Delete,
            None,
            DeferredReason::ReadOnly,
            &u1000,
            7,
        )?;
        let file = entry_file(&system_queue_dir(&uuid), id);
        check(
            Vfs::lmetadata(&file).is_ok_and(|m| m.attributes.contains(FileAttr::IMMUTABLE)),
            "a read-only volume's entry is not kept, sealed, on the system volume",
        )?;
        check(Vfs::stat(T_VICTIM).is_ok(), "queueing acted on the file")?;
        Ok(file)
    })();
    // Read-write again whatever happened: the replay this triggers is the
    // point of the step.
    Vfs::remount(mnt, opts)?;
    let file = waiting?;
    check(
        gone(Path::new(T_VICTIM)),
        "remounting read-write did not run the waiting entry",
    )?;
    check(gone(&file), "the system volume's entry is still there")?;
    Ok(())
}
