//! Cross-backend `FileMeta` conformance.
//!
//! [`FileMeta`](super::vfs::FileMeta) is a contract with thirteen
//! implementations, and until this module existed there was no test that read
//! the contract. Every backend's own tests assert what *that* backend does, so
//! a field whose meaning drifts between two implementations is invisible to all
//! of them: each one is internally consistent and each one passes.
//!
//! That is not hypothetical. `FileMeta::permissions` documents itself as twelve
//! bits — `setuid setgid sticky rwxrwxrwx`. ext4, iso9660 and memfs delivered
//! twelve; btrfs, zfs and f2fs masked to `0o777` first and delivered nine. The
//! same file, with the same bits on disk, reported a different mode depending on
//! which driver read it, so `cp -a` and `tar` reading off a btrfs/zfs/f2fs mount
//! dropped setuid, setgid and sticky from everything they copied — silently in
//! the strict sense, because "the bit is missing" is indistinguishable from "the
//! bit was never set". Each of the three was internally consistent: it masked on
//! read, nothing wrote, and every assertion about it used a mode inside `0o777`.
//! The divergence existed only *between* implementations. See
//! `known-issues.md` A-NO-CROSS-BACKEND-METADATA-CONFORMANCE-TEST and
//! `design-decisions.md` §663.
//!
//! # What this module checks, and what it deliberately does not
//!
//! Two kinds of claim can be made about a metadata field, and only one of them
//! needs to know what is on the disk:
//!
//! - **Domain and cross-route claims** need no fixture at all. A mode outside
//!   `0o7777` is wrong whatever wrote it; a `readdir` entry whose `ino`
//!   disagrees with a `stat` of the same name is wrong whatever the number is;
//!   a timestamp of `1_756_000_000` is wrong because the field is nanoseconds
//!   and that is a *seconds* value wearing the wrong unit. These run against
//!   any live filesystem the kernel can reach, which is what makes the harness
//!   cheap enough to run on every boot.
//! - **Value claims** — "this file's mode is `0o4755`" — need a fixture that
//!   declares the bits it wrote. Those are the ones that catch a *narrowing*,
//!   and a narrowing is precisely the bug above: masking `& 0o777` violates no
//!   domain rule, because every value it can produce is inside the domain.
//!
//! So the domain checks alone would **not** have caught the btrfs/zfs/f2fs bug.
//! They are here because they catch a different family cheaply and everywhere;
//! the declared-value layer is what closes the family that was actually open.
//!
//! # Why a boot-time harness rather than a host test
//!
//! Backends are `dyn FileSystem` over a block device, and the interesting ones
//! (btrfs, zfs, f2fs, ntfs) already build a synthetic volume in RAM for their
//! own self-tests, on every boot rather than only when a real disk is attached.
//! Running the contract over those same volumes costs one more pass over an
//! image that has already been constructed, and it exercises the driver rather
//! than a re-implementation of it.

use alloc::vec::Vec;

use crate::error::KernelResult;
use crate::fs::path::{Path, PathBuf};
use crate::fs::selftest::{Setup, Skips};
use crate::fs::vfs::{DirEntry, EntryType, FileMeta, FileSystem};
use crate::serial_println;

/// The full permission domain `FileMeta::permissions` promises: `setuid setgid
/// sticky rwxrwxrwx`.
const MODE_DOMAIN: u16 = 0o7777;

/// Floor for a nonzero timestamp, in nanoseconds since the epoch.
///
/// `1e16` ns is 1970-04-26. The point is not the date — no file in any fixture
/// or on any real disk predates it — but that it sits above every wrong *unit*
/// a backend could plausibly return for a present-day time:
///
/// | unit returned | value for 2026 | verdict |
/// |---|---|---|
/// | seconds | ~1.8e9 | caught |
/// | milliseconds | ~1.8e12 | caught |
/// | microseconds | ~1.8e15 | caught |
/// | nanoseconds | ~1.8e18 | passes |
///
/// A lower floor would let the microsecond case through, which is the one most
/// likely to be written by accident: a driver whose on-disk format is `timespec`
/// with a microsecond fraction, converted with one `* 1000` too few.
const TS_NS_FLOOR: u64 = 10_000_000_000_000_000;

/// Ceiling for a nonzero timestamp, in nanoseconds since the epoch.
///
/// `1e19` ns is the year 2286, and `u64::MAX` ns is 2554 — so this is not an
/// arbitrary "far future" line but the point past which a value is more likely
/// to be an arithmetic accident (a conversion applied twice, a sign-extended
/// negative) than a date. Bounding the top matters because the floor alone
/// cannot catch an over-conversion, and an over-converted timestamp reads as a
/// plausible large number.
const TS_NS_CEILING: u64 = 10_000_000_000_000_000_000;

/// Running tally for one conformance pass.
///
/// Failures carry no payload beyond the line already printed: this runs in
/// kernel context on the serial log, and a caller that wants the detail reads
/// the log. What the caller needs from here is whether to fail the boot.
pub struct Report {
    /// Checks that held.
    pub passed: u32,
    /// Checks that did not.
    pub failed: u32,
    /// Objects the harness could reach and inspect. Reported because a pass
    /// over zero objects is not a pass — it is a fixture that produced nothing,
    /// and without this number the two are indistinguishable in the log.
    pub objects: u32,
    /// Clauses that were *void* rather than held or broken: the object changed
    /// underneath the two readings being compared, so the comparison was
    /// between two different states and proves nothing either way.
    ///
    /// Counted separately from `passed` on purpose. Folding these in would be
    /// claiming a promise was kept when what actually happened is that it could
    /// not be tested, and the whole point of this harness is that a check which
    /// silently declines to run is worse than no check — it reads as coverage.
    pub voided: u32,
}

impl Report {
    /// A fresh, empty tally.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            passed: 0,
            failed: 0,
            objects: 0,
            voided: 0,
        }
    }

    /// Record one clause that could not be judged because the object moved
    /// underneath it. `why` states what was observed to move, so the log can be
    /// read as evidence rather than as an excuse.
    fn void(&mut self, backend: &str, subject: &Path, why: &str) {
        self.voided = self.voided.saturating_add(1);
        serial_println!(
            "[fsconform] VOID {}:{} — {}; the clause is void, not passed",
            backend,
            subject.display(),
            why
        );
    }

    /// Record one check. `what` names the *contract clause*, not the value, so
    /// a failure line says which promise broke.
    fn check(&mut self, ok: bool, backend: &str, subject: &Path, what: &str) {
        if ok {
            self.passed = self.passed.saturating_add(1);
        } else {
            self.failed = self.failed.saturating_add(1);
            serial_println!(
                "[fsconform] FAIL {}:{} — {}",
                backend,
                subject.display(),
                what
            );
        }
    }
}

impl Default for Report {
    fn default() -> Self {
        Self::new()
    }
}

/// Check one object's metadata against every clause of the contract that can be
/// judged without knowing what was written to the disk.
///
/// `entry` is the `DirEntry` the parent's `readdir` produced for this object,
/// when there is one — the root directory of a mount has no parent entry, and
/// the cross-route clauses are skipped for it rather than invented.
fn check_meta(
    backend: &str,
    subject: &Path,
    meta: &FileMeta,
    entry: Option<&DirEntry>,
    r: &mut Report,
) {
    r.objects = r.objects.saturating_add(1);

    // --- Domain ---

    // `permissions` is documented as twelve bits. A backend that reports a bit
    // outside them is either reporting the type bits (`S_IFDIR` and friends,
    // which belong in `entry_type`) or has not masked at all.
    r.check(
        meta.permissions & !MODE_DOMAIN == 0,
        backend,
        subject,
        "permissions carries a bit outside the documented 0o7777 domain \
         (type bits belong in entry_type, not here)",
    );

    // "Number of hard links pointing to the underlying data. Always 1 for
    // filesystems that don't support hard links." So 1 is the floor for
    // anything that exists at all, and 0 means the field was never filled in.
    // Deliberately not `>= 2` for directories: the doc's Unix convention
    // (`2 + subdirectory count`) is what a backend *with* hard links owes, and
    // FAT legitimately reports 1 for everything.
    r.check(
        meta.nlinks >= 1,
        backend,
        subject,
        "nlinks is 0 for an object that exists — the contract's floor is 1 \
         even on filesystems without hard links",
    );

    // --- Timestamp units ---
    //
    // Named individually rather than looped so the failure line says which of
    // the four is wrong; a backend that converts three correctly and one not is
    // the likely shape, and "a timestamp is wrong" would not locate it.
    for (name, ts) in [
        ("created_ns", meta.created_ns),
        ("modified_ns", meta.modified_ns),
        ("accessed_ns", meta.accessed_ns),
        ("changed_ns", meta.changed_ns),
    ] {
        // 0 is the documented "not available", and is not a unit error.
        if ts == 0 {
            continue;
        }
        r.check(
            ts >= TS_NS_FLOOR,
            backend,
            subject,
            match name {
                "created_ns" => {
                    "created_ns is too small to be nanoseconds — \
                                 it is a seconds/millis/micros value in a ns field"
                }
                "modified_ns" => {
                    "modified_ns is too small to be nanoseconds — \
                                  it is a seconds/millis/micros value in a ns field"
                }
                "accessed_ns" => {
                    "accessed_ns is too small to be nanoseconds — \
                                  it is a seconds/millis/micros value in a ns field"
                }
                _ => {
                    "changed_ns is too small to be nanoseconds — \
                      it is a seconds/millis/micros value in a ns field"
                }
            },
        );
        r.check(
            ts <= TS_NS_CEILING,
            backend,
            subject,
            match name {
                "created_ns" => {
                    "created_ns is past the year 2286 — \
                                 an over-conversion, not a date"
                }
                "modified_ns" => {
                    "modified_ns is past the year 2286 — \
                                  an over-conversion, not a date"
                }
                "accessed_ns" => {
                    "accessed_ns is past the year 2286 — \
                                  an over-conversion, not a date"
                }
                _ => "changed_ns is past the year 2286 — an over-conversion, not a date",
            },
        );
    }

    // --- Cross-route agreement ---
    //
    // `DirEntry::ino`'s doc says it is "the **same number** `FileMeta::ino`
    // carries for the same object", and that the two agreeing "by construction"
    // is the point of the field. Every backend has been inspected for it one at
    // a time; this is the first thing that actually asks.
    let Some(entry) = entry else {
        return;
    };

    r.check(
        entry.ino == meta.ino,
        backend,
        subject,
        "readdir's d_ino and stat's st_ino disagree for the same name — \
         the two are documented as the same number",
    );

    r.check(
        entry.entry_type == meta.entry_type,
        backend,
        subject,
        "readdir and stat disagree about the entry type",
    );

    // The regular-file size clause is deliberately *not* here. It is the one
    // cross-route check whose two readings can be separated by enough work for
    // the object to change between them, so it needs a window this function
    // does not have. `check_sizes_bracketed` runs it.
    //
    // (It applies only to regular files in any case: `DirEntry::size` is
    // documented as "0 for directories", while `FileMeta::size` for a directory
    // is whatever the backend charges for the directory's own storage. Those are
    // two different quantities, and asserting they match would be asserting a
    // contract that was never written.)
}

/// Cross-route size agreement for regular files — *established*, not assumed.
///
/// `DirEntry::size` and `FileMeta::size` are the same quantity by two routes, so
/// they must agree — but only for an object that held still between the two
/// readings, and this harness cannot freeze one. On a filesystem that synthesises
/// content per call they legitimately differ: `/proc/heapinfo` prints
/// `slab_allocs` and `total_allocs`, which the harness's *own* allocations
/// increment between the `readdir` and the `stat`, so a decimal digit rolls over
/// and the byte length changes. procfs is not wrong there — each size it returned
/// was true when it was taken. The clause was wrong, because it asserted an
/// invariant that holds only for a stable object without ever establishing that
/// the object was stable.
///
/// So bracket the stat between two listings: `d1` from the walk's `readdir`, then
/// the stat, then a second `readdir` giving `d2`.
///
/// - **`d1 == d2`** — the size did not move across a window that *contains* the
///   stat, so the stat must have seen that same value. A difference is then a
///   genuine cross-route disagreement, and is failed.
/// - **`d1 != d2`** — the object changed size while we were looking. The two
///   readings are of different states, so the comparison proves nothing; the
///   clause is voided and named in the log rather than silently skipped.
///
/// This is *exact* for a size that only ever grows, which is the `heapinfo`
/// case. It is not exact for one that can return to an earlier value —
/// `slab_active` is allocs-minus-frees and can fall back — so a `d1 == d2`
/// mismatch is confirmed by one tight re-reading of that single name before it
/// is failed. That costs nothing on the common path, because it runs only where
/// a failure was about to be reported, and it is the difference between a
/// harness that occasionally cries wolf and one whose failures are evidence.
fn check_sizes_bracketed(
    backend: &'static str,
    fs: &mut dyn FileSystem,
    dir: &Path,
    entries: &[DirEntry],
    stat_sizes: &[(usize, u64)],
    r: &mut Report,
) {
    if stat_sizes.is_empty() {
        return;
    }
    let after = match fs.readdir(dir) {
        Ok(e) => e,
        Err(e) => {
            // The first listing succeeded; a second that refuses is the same
            // shape of self-contradiction the rest of this harness fails on.
            // Treating it as "no second reading, so skip the clause" would let a
            // backend switch the check off by breaking.
            r.failed = r.failed.saturating_add(1);
            serial_println!(
                "[fsconform] FAIL {}:{} — readdir succeeded once and then returned {:?}; \
                 the second listing is what establishes whether sizes held still",
                backend,
                dir.display(),
                e
            );
            return;
        }
    };

    for &(idx, stat_size) in stat_sizes {
        let Some(entry) = entries.get(idx) else {
            continue;
        };
        let child = dir.join(&entry.name);
        let subject: &Path = &child;

        // A name that listed, statted, then stopped listing has changed more
        // than its size; the clause is void for it either way.
        let Some(again) = after.iter().find(|e| e.name == entry.name) else {
            r.void(backend, subject, "listed before the stat and gone after it");
            continue;
        };

        if entry.size != again.size {
            r.void(backend, subject, "size moved across the walk");
            continue;
        }

        if stat_size == entry.size {
            r.check(true, backend, subject, "");
            continue;
        }

        // The bracket says the size held, but the stat disagrees. Either that is
        // a real cross-route defect, or the size left and came back inside the
        // window. Ask again, tightly, before accusing anyone.
        match tight_reread(fs, dir, &entry.name) {
            Some((d3, m2, d4)) if d3 == d4 && m2 != d3 => {
                r.check(
                    false,
                    backend,
                    subject,
                    "readdir and stat disagree about a regular file's size, and a \
                     second, tightly-taken pair of readings disagrees the same way",
                );
            }
            Some(_) => {
                r.void(
                    backend,
                    subject,
                    "readdir and stat disagreed once, but a tight re-reading did not \
                     reproduce it, so the size returned to an earlier value inside the \
                     window",
                );
            }
            None => {
                // The object answered three times and then would not. That is
                // not a size disagreement, so do not report one; it is still a
                // backend contradicting itself, so do not stay silent either.
                r.failed = r.failed.saturating_add(1);
                serial_println!(
                    "[fsconform] FAIL {}:{} — could not be re-read to confirm a size \
                     disagreement (readdir said {}, stat said {})",
                    backend,
                    subject.display(),
                    entry.size,
                    stat_size
                );
            }
        }
    }
}

/// Re-read one name's size by both routes, as close together as the interface
/// allows: `readdir`, `metadata`, `readdir`.
///
/// Returns `None` if any of the three refused — a different failure from a size
/// disagreement, and reported as one by the caller.
fn tight_reread(fs: &mut dyn FileSystem, dir: &Path, name: &PathBuf) -> Option<(u64, u64, u64)> {
    let d3 = fs.readdir(dir).ok()?.iter().find(|e| e.name == *name)?.size;
    let m2 = fs.metadata(&dir.join(name)).ok()?.size;
    let d4 = fs.readdir(dir).ok()?.iter().find(|e| e.name == *name)?.size;
    Some((d3, m2, d4))
}

/// Run the contract over one live filesystem, starting at `dir`.
///
/// Walks `dir`'s entries and checks each one, plus `dir` itself. One level
/// deep on purpose: the clauses here are per-object and hold uniformly, so a
/// recursive walk would multiply the same evidence rather than add to it, and
/// on procfs it would wander into a tree that changes underneath the walk.
///
/// A backend that cannot list `dir` is skipped **only** when it said it does not
/// implement the operation. Any other error is counted as a failure: a driver
/// that is asked to list its own root and refuses has answered the question this
/// harness exists to ask, and treating that as "nothing to check here" would let
/// the harness disable itself on exactly the boot where it had found something.
/// See `crate::fs::selftest::classify` for the three errors that mean "this
/// system cannot" as against "this system was asked and refused".
pub fn check_tree(
    backend: &'static str,
    fs: &mut dyn FileSystem,
    dir: &Path,
    r: &mut Report,
    skips: &mut Skips,
) {
    let entries = match fs.readdir(dir) {
        Ok(e) => e,
        Err(e) => {
            match crate::fs::selftest::classify::<()>(Err(e)) {
                Setup::Unsupported(_) => {
                    skips.record(backend, "backend does not implement readdir");
                }
                Setup::Ready | Setup::Failed(_) => {
                    r.failed = r.failed.saturating_add(1);
                    serial_println!(
                        "[fsconform] FAIL {}:{} — readdir refused with {:?}, which is not \
                         a missing feature; a backend that cannot list its own root has \
                         answered this harness rather than excused itself from it",
                        backend,
                        dir.display(),
                        e
                    );
                }
            }
            return;
        }
    };

    // The directory itself, with no parent entry to cross-check against.
    match fs.metadata(dir) {
        Ok(meta) => check_meta(backend, dir, &meta, None, r),
        Err(e) => {
            // Counted, not merely printed: `readdir` has already answered that
            // this object exists, so a `metadata` that disagrees is the same
            // cross-route contradiction the per-entry loop below fails on, and
            // reporting it only as a log line would let a backend contradict
            // itself about its own root while the harness still says "passed".
            r.failed = r.failed.saturating_add(1);
            serial_println!(
                "[fsconform] FAIL {}:{} — readdir succeeded but metadata() returned \
                 {:?}; the two routes disagree about whether the object exists",
                backend,
                dir.display(),
                e
            );
        }
    }

    // Sizes the stats reported, for regular files only, kept until the second
    // listing below can say whether the object held still while we read it.
    // See `check_sizes_bracketed`.
    let mut stat_sizes: Vec<(usize, u64)> = Vec::new();

    for (idx, entry) in entries.iter().enumerate() {
        let child: PathBuf = dir.join(&entry.name);
        match fs.metadata(&child) {
            Ok(meta) => {
                if meta.entry_type == EntryType::File && entry.entry_type == EntryType::File {
                    stat_sizes.push((idx, meta.size));
                }
                check_meta(backend, &child, &meta, Some(entry), r);
            }
            Err(e) => {
                // A name that lists but does not stat is itself a cross-route
                // disagreement, and a real one: it is what a caller sees as a
                // file that exists in `ls` and vanishes under `stat`. Counted
                // as a failure rather than a skip, unlike a fixture that would
                // not mount.
                r.failed = r.failed.saturating_add(1);
                serial_println!(
                    "[fsconform] FAIL {}:{} — listed by readdir but metadata() \
                     returned {:?}",
                    backend,
                    child.display(),
                    e
                );
            }
        }
    }

    check_sizes_bracketed(backend, fs, dir, &entries, &stat_sizes, r);
}

/// Run the contract over every backend the boot can reach.
///
/// Returns `Err` when a *clause* failed. A backend that could not be reached is
/// not an error: this must run on a diskless memfs boot as readily as on one
/// with disks attached, and refusing to pass because no btrfs volume exists
/// would make the harness noise on the common path and train the log's reader
/// to ignore it.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[fsconform] Running cross-backend FileMeta conformance...");

    let mut r = Report::new();
    let mut skips = Skips::new();
    let mut heap = HeapWatch::new();

    // A backend the harness fully controls, driven directly rather than through
    // the VFS. It is here for two reasons beyond checking memfs: it is the one
    // pass guaranteed to inspect a nonzero number of objects on any boot, so
    // "0 objects" in the report below can only mean the harness itself is
    // broken; and it exercises the direct-`dyn FileSystem` route that the
    // synthetic btrfs/zfs/f2fs/ntfs volumes will use.
    //
    // A fixture that will not build is a *failure*, not a skip. It is
    // `MemFs::new()`, two writes and a mkdir against an allocator the kernel has
    // already proven it has; there is no environment in which it legitimately
    // cannot be made. Skipping here would be deciding not to run the harness
    // based on the outcome of a call into the very code the harness checks.
    match build_memfs_fixture() {
        Ok(mut fs) => check_tree("memfs", &mut fs, Path::new("/"), &mut r, &mut skips),
        Err(e) => {
            r.failed = r.failed.saturating_add(1);
            serial_println!(
                "[fsconform] FAIL memfs:/ — the fixture could not be built ({e:?}); it is \
                 `MemFs::new()` plus two writes and a mkdir and depends on nothing about \
                 this boot, \
                 so this is memfs failing, not an austere environment"
            );
        }
    }
    heap.mark(&"memfs fixture", &mut r);

    // The live VFS mounts. Every boot has at least the root; a boot with
    // pseudo-filesystems mounted gets those too, and those are exactly the
    // backends whose `ino` and `nlinks` are documented as placeholders.
    //
    // The mount check asks the *mount table*, not the filesystem: whether
    // `/proc` is mounted is a fact about this boot's configuration, and a
    // question the code under test does not get a vote on.
    for path in ["/", "/proc", "/sys", "/dev", "/tmp"] {
        let p = Path::new(path);
        if path != "/" && !crate::fs::selftest::is_mounted(path) {
            skips.record(path, "not mounted on this boot");
            continue;
        }
        check_vfs_path(p, &mut r, &mut skips, &mut heap);
        heap.mark(&path, &mut r);
    }

    // A pass over zero objects is not a pass, and this is the difference
    // between a harness and a harness-shaped no-op. The memfs fixture is built
    // from nothing but `MemFs::new()` and two writes, so it cannot fail for any
    // environmental reason — if nothing was inspected, the harness is broken
    // rather than the boot being austere, and saying "passed" would be the
    // exact failure mode `A-GATES-SILENTLY-STOPPED-CHECKING` records.
    if r.objects == 0 {
        serial_println!(
            "[fsconform] Conformance VACUOUS: {} clause(s) recorded but no object was \
             inspected — the memfs fixture is unconditional, so this is the harness \
             failing to reach anything, not a boot with nothing mounted.",
            r.passed
        );
        return Err(crate::error::KernelError::InternalError);
    }

    skips.report("[fsconform]");

    // Voided clauses belong on the closing line for the same reason the skips
    // do: a clause that could not be judged is not a clause that held, and a
    // reader who scrolled to the bottom must not be told otherwise. The count
    // is also the thing that would make a *regression* visible — a backend that
    // started churning every file's size would void its way to a clean run, and
    // this number is what shows that happening.
    let voided = if r.voided == 0 {
        ""
    } else {
        " — see the VOID lines above"
    };

    if r.failed == 0 {
        // The suffix, not just the `report` above: the closing line is the one a
        // reader believes, so a run that reached four of five mounts must not
        // read as a full pass to someone who scrolled to the bottom.
        serial_println!(
            "[fsconform] Conformance passed ({} clause(s) over {} object(s), {} voided{}){}.",
            r.passed,
            r.objects,
            r.voided,
            voided,
            skips.suffix()
        );
        return Ok(());
    }

    serial_println!(
        "[fsconform] Conformance FAILED: {} clause(s) held, {} broke, {} voided{}, over {} \
         object(s){}.",
        r.passed,
        r.failed,
        r.voided,
        voided,
        r.objects,
        skips.suffix()
    );
    Err(crate::error::KernelError::InternalError)
}

/// Check one path through the VFS rather than against a `dyn FileSystem`.
///
/// The VFS route is what userspace actually gets — `Vfs::readdir` and
/// `Vfs::metadata_resolved` are the two calls behind `getdents64` and `stat`,
/// and it is *their* disagreement that reaches a program. Driving the backend
/// directly would check the driver and miss anything the VFS layer adds on top,
/// which is where `finish_listing` synthesises submount entries.
///
/// `dir`'s presence has already been established from the mount table by the
/// caller, so a `readdir` that then refuses is not evidence that the path is
/// absent — it is the mounted filesystem declining. Only the three errors
/// `crate::fs::selftest::classify` reads as "this system cannot" are skipped;
/// everything else is a failure, because a skip decided from the outcome of the
/// call under test disables the check on exactly the boot where it would have
/// fired.
fn check_vfs_path(dir: &Path, r: &mut Report, skips: &mut Skips, heap: &mut HeapWatch) {
    use crate::fs::vfs::Vfs;

    let backend = "vfs";
    let entries = match Vfs::readdir(dir) {
        Ok(e) => e,
        Err(e) => {
            match crate::fs::selftest::classify::<()>(Err(e)) {
                Setup::Unsupported(_) => {
                    skips.record("vfs readdir", "filesystem does not implement readdir");
                }
                Setup::Ready | Setup::Failed(_) => {
                    r.failed = r.failed.saturating_add(1);
                    serial_println!(
                        "[fsconform] FAIL {}:{} — path is mounted but readdir returned \
                         {:?}, which is not a missing feature",
                        backend,
                        dir.display(),
                        e
                    );
                }
            }
            return;
        }
    };

    for entry in &entries {
        let child: PathBuf = dir.join(&entry.name);
        match Vfs::metadata_resolved(&child) {
            Ok(meta) => check_meta(backend, &child, &meta, Some(entry), r),
            Err(_) => {
                // Deliberately not a failure on the VFS route, unlike the
                // direct-backend one: a name listed in /proc can legitimately
                // belong to a process that exited between the listing and the
                // stat, and that race is the filesystem behaving correctly.
                // The direct route has no such race because nothing else holds
                // the fixture.
            }
        }
        // Per entry, not per directory: on a pseudo-filesystem `stat` runs the
        // file's *generator*, so a counter that moves here names the one
        // generator that moved it. Attributing it to the directory would leave
        // 200 candidates.
        heap.mark_path(&child, r);
    }
}

/// Watches the heap's corruption counters across the pass and attributes any
/// movement to the phase that caused it.
///
/// # Why this belongs in a metadata harness
///
/// It is here because of how the harness's own first live boot failed. Two
/// `[heap] DOUBLE-FREE detected!` lines appeared during the pass; nothing in
/// the log said which of ~200 `stat` calls produced them. They incremented a
/// *global* counter, which `syshealth` later read as `[FAIL] Heap safety: 2
/// violation(s)`, which made `kshell::self_test` rung 21 — "a checker that
/// passed reports success" — panic the kernel. So the boot died in a self-test
/// three subsystems away from the code that actually broke, with a message that
/// named none of it.
///
/// That chain is the general hazard, not a one-off: a self-test that perturbs
/// global state hands a later checker a failure it cannot explain, and the
/// further apart the two run, the more expensive the diagnosis. The fix is for
/// the perturbing pass to notice at the moment it happens, while it still knows
/// which object it was touching.
///
/// Reading a `stat` on a pseudo-filesystem runs arbitrary kernel code — the
/// file's generator — so this harness is unusually likely to be the thing that
/// trips a latent allocator bug, and unusually well placed to say which call
/// did it.
struct HeapWatch {
    double_free: u32,
    use_after_free: u32,
    redzone: u32,
}

impl HeapWatch {
    /// Sample the counters as they stand before the pass touches anything.
    ///
    /// Baselined rather than assumed zero: an earlier self-test may legitimately
    /// have exercised the detector (`mm::heap`'s own self-test deliberately
    /// performs a double-free to prove detection works), and a harness that
    /// reported those as its own would be crying wolf on every boot.
    fn new() -> Self {
        let s = crate::mm::heap::stats();
        Self {
            double_free: s.double_free_violations,
            use_after_free: s.poison_violations,
            redzone: s.redzone_violations,
        }
    }

    /// Compare against the baseline, report and re-baseline. `what` names the
    /// phase that ran since the last call.
    ///
    /// Counted as a conformance failure: the counters are global, so leaving
    /// them moved is not a private matter for this harness. Something that
    /// corrupts the allocator while a directory is being listed is a worse
    /// finding than any metadata disagreement this module was built to look for,
    /// and it must not be reported more quietly than one.
    fn mark(&mut self, what: &dyn core::fmt::Display, r: &mut Report) {
        let s = crate::mm::heap::stats();
        let df = s.double_free_violations.saturating_sub(self.double_free);
        let uaf = s.poison_violations.saturating_sub(self.use_after_free);
        let rz = s.redzone_violations.saturating_sub(self.redzone);

        if df != 0 || uaf != 0 || rz != 0 {
            r.failed = r.failed.saturating_add(1);
            serial_println!(
                "[fsconform] FAIL heap:{} — the allocator's corruption counters moved while \
                 this was being inspected: {} double-free(s), {} use-after-free(s), {} \
                 redzone violation(s). The counters are global, so this will also surface \
                 later as a syshealth failure with nothing to point at; the culprit is here.",
                what,
                df,
                uaf,
                rz
            );
        }

        // Re-baseline unconditionally, including after a report: each phase is
        // charged only with what it did, so one bad generator cannot make every
        // later phase look guilty too.
        self.double_free = s.double_free_violations;
        self.use_after_free = s.poison_violations;
        self.redzone = s.redzone_violations;
    }

    /// [`mark`](Self::mark) for a path, which needs `display()` rather than
    /// `Display` because `Path` deliberately does not implement it — a path is
    /// bytes, and rendering it is a lossy choice the caller has to make.
    fn mark_path(&mut self, p: &Path, r: &mut Report) {
        self.mark(&p.display(), r);
    }
}

/// Build a small in-memory filesystem carrying one object of each kind the
/// contract distinguishes.
///
/// Deliberately not "one file": the clauses branch on `entry_type` — the
/// size cross-check is `File`-only — so a fixture of files alone would leave
/// the directory arm of every clause unexecuted and the harness would report a
/// pass it had not earned.
fn build_memfs_fixture() -> KernelResult<crate::fs::memfs::MemFs> {
    use crate::fs::memfs::MemFs;

    let mut fs = MemFs::new();
    fs.write_file(Path::new("/plain"), b"conformance")?;
    fs.mkdir(Path::new("/adir"))?;
    fs.write_file(Path::new("/adir/nested"), b"nested")?;
    Ok(fs)
}

// ---------------------------------------------------------------------------
// Constant self-checks
// ---------------------------------------------------------------------------
//
// These are `const` assertions and not `#[cfg(test)]` unit tests on purpose:
// the kernel crate sets `test = false` (`kernel/Cargo.toml`), because a host
// test binary cannot link a crate that supplies its own `panic_impl`. A
// `#[cfg(test)] mod tests` here would therefore compile for nobody and run on
// no boot, while looking in the source exactly like coverage. A const assertion
// is checked on every build instead, which for a claim about two integer
// constants is strictly more than a unit test would have given.

/// The domain mask must be exactly the twelve documented bits, and must contain
/// the three special ones — those three being the whole subject of the bug this
/// module was written for.
const _: () = assert!(MODE_DOMAIN == 0o7777);
const _: () = assert!(MODE_DOMAIN.count_ones() == 12);
const _: () = assert!(
    0o4000u16 & !MODE_DOMAIN == 0,
    "setuid is outside the domain"
);
const _: () = assert!(
    0o2000u16 & !MODE_DOMAIN == 0,
    "setgid is outside the domain"
);
const _: () = assert!(
    0o1000u16 & !MODE_DOMAIN == 0,
    "sticky is outside the domain"
);

/// The timestamp floor's justification is the gap between these rows, so they
/// are asserted rather than left in a comment: a later edit that lowers the
/// floor to admit some backend's odd value would silently reopen the
/// microsecond case, which is the one most likely to be written by accident.
///
/// `SAMPLE_SECS` is 2026-09-01T00:00:00Z, expressed four ways below.
const SAMPLE_SECS: u64 = 1_787_184_000;
const _: () = assert!(
    SAMPLE_SECS < TS_NS_FLOOR,
    "a seconds value must be rejected"
);
const _: () = assert!(
    SAMPLE_SECS * 1_000 < TS_NS_FLOOR,
    "a milliseconds value must be rejected"
);
const _: () = assert!(
    SAMPLE_SECS * 1_000_000 < TS_NS_FLOOR,
    "a microseconds value must be rejected"
);
const _: () = assert!(
    SAMPLE_SECS * 1_000_000_000 >= TS_NS_FLOOR,
    "a nanoseconds value must be accepted"
);
const _: () = assert!(
    SAMPLE_SECS * 1_000_000_000 <= TS_NS_CEILING,
    "a nanoseconds value must be under the ceiling"
);
