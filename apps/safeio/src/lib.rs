//! Crash-safe file writes for applications that persist user data.
//!
//! # Why this crate exists
//!
//! `std::fs::write` opens its target with `O_TRUNC`. The file is emptied
//! *first*, and only then refilled. Every application that saved a document
//! with it therefore had a window — short, but real — in which the user's file
//! existed on disk as a truncated fragment or as nothing at all. Anything that
//! interrupts the write inside that window leaves it that way permanently:
//! a full disk, a removed USB stick, a killed process, a power loss.
//!
//! That is not a hypothetical failure for a text editor. "Save, then the
//! machine died, and now the file is empty" is the single worst thing a
//! document editor can do, because the user's own copy *was* the file.
//!
//! The fix is the standard one: write the new contents to a temporary file
//! beside the target, flush it to the disk, and then `rename` it over the
//! target. `rename` within a directory is atomic — every reader sees either
//! the whole old file or the whole new one, never a partial one — so an
//! interruption at any point leaves the original intact and costs at most a
//! stray temporary.
//!
//! # What this does *not* promise
//!
//! [`write_atomically`] guarantees the target is never observed partially
//! written. It does not guarantee the *new* contents survive a power loss:
//! that additionally requires flushing the containing directory, which is not
//! portable (Windows cannot open a directory as a file). The distinction
//! matters and is the right trade: losing an edit is recoverable by redoing
//! it, whereas losing the file that existed before the save is not.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// How many temporary names to try before giving up.
///
/// A collision needs two saves of the same file in the same process to draw
/// the same counter value, which cannot happen, or another process to have
/// left a temporary with the same PID — possible only after a PID has been
/// recycled. The bound exists so that a directory in a pathological state
/// fails the save instead of spinning forever.
const MAX_TEMP_ATTEMPTS: u32 = 1024;

/// Distinguishes concurrent saves within one process.
///
/// The PID alone is not enough: two documents saved at once, or one document
/// saved twice in quick succession, would otherwise pick the same temporary
/// name and the second would clobber the first's half-written file.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Successful [`write_atomically`] calls, counted only under the `audit`
/// feature. See [`writes_performed`].
#[cfg(feature = "audit")]
static WRITES: AtomicU64 = AtomicU64::new(0);

/// Successful [`copy_atomically`] calls, counted only under the `audit`
/// feature. See [`copies_performed`].
#[cfg(feature = "audit")]
static COPIES: AtomicU64 = AtomicU64::new(0);

/// How many [`write_atomically`] and [`write_new_atomically`] calls have
/// succeeded in this process.
///
/// This exists so a caller can *test* that its saves go through this crate.
/// It cannot be tested any other way: a successful atomic write and a
/// successful `std::fs::write` leave identical bytes at identical paths, and
/// the difference between them — that `fs::write` truncates the target before
/// it writes, so an interrupted save destroys the file it was saving — only
/// shows up in an interruption a portable test cannot stage. Without this
/// counter, swapping an adopter back to `fs::write` leaves all of its tests
/// green, which is exactly what happened.
///
/// Compare a reading taken before an operation with one taken after, rather
/// than against an absolute: tests in a crate share a process and run in
/// parallel, so the absolute value is not reproducible.
///
/// Only available under the `audit` feature, which callers enable in their
/// `[dev-dependencies]`.
#[cfg(feature = "audit")]
#[must_use]
pub fn writes_performed() -> u64 {
    WRITES.load(Ordering::Relaxed)
}

/// How many [`copy_atomically`] calls have succeeded in this process.
///
/// As [`writes_performed`], for copies. Kept separate so a test can assert
/// which of the two an operation used, rather than only that it used one.
///
/// Only available under the `audit` feature.
#[cfg(feature = "audit")]
#[must_use]
pub fn copies_performed() -> u64 {
    COPIES.load(Ordering::Relaxed)
}

/// What a bounded read actually returned.
///
/// The three fields exist because a caller needs all three to say something
/// true, and eight apps had each worked that out separately.
#[derive(Clone, Debug)]
pub struct CappedRead {
    /// The text that was read, cut at a character boundary.
    pub text: String,
    /// The file's full length in bytes, whether or not all of it was read.
    pub whole: usize,
    /// Whether `text` is shorter than the file.
    pub truncated: bool,
}

impl CappedRead {
    /// A prefix to put in front of whatever the caller was going to say, or
    /// the empty string when nothing was cut.
    ///
    /// **Front-loaded, deliberately.** A truncated document usually fails to
    /// parse -- its closing tags or its last record were in the part that was
    /// dropped -- so a caller that reports the parse error first tells the
    /// user their file is malformed when it is merely long.
    #[must_use]
    pub fn note(&self, max: usize) -> String {
        if self.truncated {
            format!("INCOMPLETE ({max} of {} bytes read): ", self.whole)
        } else {
            String::new()
        }
    }

    /// Drop any final partial line.
    ///
    /// For line-oriented formats, where a half-record is worse than a missing
    /// one: `apps/dbviewer` imports CSV, and a cut in the middle of a row
    /// yields a record with half its columns that the parser accepts.
    #[must_use]
    pub fn to_last_line(mut self) -> Self {
        if self.truncated {
            let keep = self.text.rfind('\n').map_or(0, |nl| nl);
            self.text.truncate(keep);
        }
        self
    }
}

/// What a bounded read of a BINARY file returned.
///
/// Separate from [`CappedRead`] rather than generic over the two, because the
/// interesting part of the text version -- backing the cut up to a character
/// boundary -- has no meaning here, and a type that carried the field without
/// the behaviour would invite someone to rely on it.
#[derive(Clone, Debug)]
pub struct CappedBytes {
    /// The bytes that were read.
    pub bytes: Vec<u8>,
    /// The file's full length, whether or not all of it was read.
    pub whole: usize,
    /// Whether `bytes` is shorter than the file.
    pub truncated: bool,
}

impl CappedBytes {
    /// A prefix to put in front of whatever the caller was going to say, or
    /// the empty string when nothing was cut.
    ///
    /// **Front-loaded, and it matters more here than for text.** A truncated
    /// binary file almost never parses -- a length prefix will point past the
    /// end, a checksum will not match -- so a caller that reports the parse
    /// error first tells the user their file is corrupt when it is merely
    /// larger than this program will read.
    #[must_use]
    pub fn note(&self, max: usize) -> String {
        if self.truncated {
            format!("INCOMPLETE ({max} of {} bytes read): ", self.whole)
        } else {
            String::new()
        }
    }
}

/// Read `path` as bytes, stopping after `max`.
///
/// For formats that are not text: a `.torrent` is bencode, an image is an
/// image, and [`read_to_string_capped`] would refuse both at the first byte
/// that is not UTF-8 -- reporting "stream did not contain valid UTF-8" about a
/// file that is perfectly valid and simply not text.
///
/// # What `max` bounds
///
/// The *allocation*, not merely the result. At most `max + 1` bytes are ever
/// read or held, so asking for 64 KiB of a 40 GB file costs 64 KiB.
///
/// This function used to call `std::fs::read` and truncate the result. That
/// bounded what the caller was handed and nothing else: the whole file was
/// already in memory by the time the cap was applied, so the cap could not
/// prevent the single failure a cap is for. It is the mistake
/// `imagecodec::Limits` names in its own documentation -- "a limit applied
/// afterwards is not a limit, it is a post-mortem" -- and every caller of this
/// function had inherited it, including the picture viewers whose comments
/// said they were protected.
///
/// The one byte past the cap is what makes "the file ended" distinguishable
/// from "we stopped" without a second trip to the filesystem.
///
/// # Errors
///
/// Whatever opening and reading the file returns: it is missing, or is not
/// readable.
pub fn read_capped(path: &Path, max: usize) -> io::Result<CappedBytes> {
    let mut file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(ceiling(max))
        .read_to_end(&mut bytes)?;
    if bytes.len() <= max {
        // The read stopped short of its own limit, so the file is exhausted
        // and this is all of it.
        let whole = bytes.len();
        return Ok(CappedBytes {
            bytes,
            whole,
            truncated: false,
        });
    }
    bytes.truncate(max);
    Ok(CappedBytes {
        bytes,
        whole: whole_len(&file, max),
        truncated: true,
    })
}

/// One byte past `max`, as a count for [`Read::take`].
///
/// Saturating at both steps: a `max` of `usize::MAX` has no successor, and on
/// a 32-bit target the cast is the narrowing one.
fn ceiling(max: usize) -> u64 {
    u64::try_from(max).unwrap_or(u64::MAX).saturating_add(1)
}

/// The file's full length, for reporting how much was left unread.
///
/// Only called once the read has already stopped at the cap, so a failure here
/// cannot cost data -- it costs the exact figure in a message. When the
/// filesystem will not say, the answer falls back to `max + 1`, which is a
/// *lower bound* rather than a guess: that many bytes were just read, so the
/// file is at least that long. The same floor is applied to the reported
/// length, because a size that contradicts what was already read would be
/// worse than an imprecise one.
fn whole_len(file: &fs::File, max: usize) -> usize {
    let known = max.saturating_add(1);
    file.metadata()
        .ok()
        .and_then(|m| usize::try_from(m.len()).ok())
        .map_or(known, |len| len.max(known))
}

/// Read `path` as text, stopping after `max` bytes.
///
/// # Why a bounded read is its own function
///
/// Eight applications had written this loop, and they agree -- I checked all
/// eight before replacing them. That is the moment to collect it, not after
/// one of them drifts: the same file held twelve hand-rolled Rust maskers and
/// three of them were wrong about raw strings.
///
/// The cut is moved back to a character boundary because slicing a `String`
/// anywhere else panics, and the loop terminates because offset 0 is always a
/// boundary.
///
/// # What this does NOT do
///
/// It does not decide what to say. A truncation means different things to
/// different formats -- a cut calendar silently loses appointments, a cut CSV
/// silently loses rows, a cut XML document usually fails to parse outright --
/// and only the caller knows which. [`CappedRead::note`] offers the wording
/// that suits most of them.
///
/// # What `max` bounds
///
/// The *allocation*, for the reasons given on [`read_capped`]: at most
/// `max + 1` bytes are read. This too used to read the whole file first.
///
/// # Validity is judged on what was read, not on what was skipped
///
/// A consequence of stopping at the cap, and an improvement: a 2 GB log whose
/// last megabyte is binary junk now yields its first 64 KiB instead of failing
/// with "stream did not contain valid UTF-8" about bytes the caller was never
/// going to see. Only the returned prefix has to be text.
///
/// # Errors
///
/// The file is missing, is not readable, or *the part that was read* is not
/// UTF-8.
pub fn read_to_string_capped(path: &Path, max: usize) -> io::Result<CappedRead> {
    let mut file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(ceiling(max))
        .read_to_end(&mut bytes)?;
    if bytes.len() <= max {
        let text = into_text(bytes)?;
        let whole = text.len();
        return Ok(CappedRead {
            text,
            whole,
            truncated: false,
        });
    }
    bytes.truncate(max);
    // Back the cut off a partial character. `error_len() == None` is precisely
    // "the input ended in the middle of one", which is this cut rather than a
    // defect in the file -- so keep what was whole before it. `Some` is a byte
    // sequence that is invalid however much follows it, and that is the file's
    // problem, reported as such.
    let end = match std::str::from_utf8(&bytes) {
        Ok(_) => bytes.len(),
        Err(e) if e.error_len().is_none() => e.valid_up_to(),
        Err(_) => return Err(not_utf8()),
    };
    bytes.truncate(end);
    let whole = whole_len(&file, max);
    Ok(CappedRead {
        text: into_text(bytes)?,
        whole,
        truncated: true,
    })
}

/// Bytes to text, failing the way `std::fs::read_to_string` fails.
///
/// Callers already handle that error and some match on its wording, so a
/// bounded read reports a non-text file identically to an unbounded one.
fn into_text(bytes: Vec<u8>) -> io::Result<String> {
    String::from_utf8(bytes).map_err(|_| not_utf8())
}

/// The error `std::fs::read_to_string` gives for a file that is not text.
fn not_utf8() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "stream did not contain valid UTF-8",
    )
}

/// Write `contents` to `path` so that `path` is never left partially written.
///
/// On success `path` holds exactly `contents`. On failure `path` is untouched
/// — it keeps whatever it held before, including not existing — and no
/// temporary file is left behind.
///
/// A name that must still be free when the write lands -- one chosen a while
/// ago, which somebody else's file may have taken since -- wants
/// [`write_new_atomically`] instead: this replaces whatever is there.
///
/// # Symlinks
///
/// A symlink target is followed, and the *resolved* file is replaced. This
/// matters because rename-over replaces whatever it renames onto: without
/// resolving, saving a file the user opened through a symlink would delete
/// their symlink and leave a regular file in its place, which is not what
/// "save" means. Editing dotfiles through a symlinked config directory is the
/// ordinary case here, not an exotic one.
///
/// # Permissions
///
/// When the target already exists its permissions are copied onto the
/// replacement, because the new file would otherwise be created with the
/// process's default mode. A save must not quietly make a private file
/// world-readable.
///
/// # Errors
///
/// Returns the underlying [`io::Error`] if the directory cannot be written,
/// the data cannot be flushed, or the rename fails.
pub fn write_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
    // Resolve a symlink to the file it points at. `canonicalize` fails when
    // the path does not exist yet, which is the ordinary "save a new file"
    // case, so that failure is not an error — fall back to the path as given.
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

    let parent = target.parent().filter(|p| !p.as_os_str().is_empty());
    let dir: PathBuf = parent.map_or_else(|| PathBuf::from("."), Path::to_path_buf);

    let (mut file, tmp_path) = create_temp_in(&dir, &target)?;

    // From here on every failure has to remove the temporary before returning,
    // or a failed save leaves a full-size copy of the document sitting next to
    // it under a name the user never chose.
    let result = (|| -> io::Result<()> {
        file.write_all(contents)?;
        // Flush the contents to the device before the rename. Without this the
        // rename can reach the disk first, leaving a file that atomically
        // points at unwritten blocks — the "empty file after a crash" that
        // atomic rename is supposed to prevent.
        file.sync_all()
    })();

    // Close before renaming: Windows refuses to rename a file that is still
    // open, so leaving this to the end of the scope would make every save fail.
    drop(file);

    if let Err(e) = result {
        let _ = fs::remove_file(&tmp_path); // Best effort; the write error is the one worth reporting.
        return Err(e);
    }

    // Copy the original's permissions onto the replacement, if there was an
    // original. A missing target is the new-file case, where the default mode
    // is correct.
    if let Ok(meta) = fs::metadata(&target)
        && fs::set_permissions(&tmp_path, meta.permissions()).is_err()
    {
        // Not fatal. The contents are what the user asked to save, and
        // failing the save to preserve a mode bit would lose the edit to
        // protect a detail the user cannot see. Filesystems that do not
        // support permissions at all reach this on every save.
    }

    if let Err(e) = fs::rename(&tmp_path, &target) {
        let _ = fs::remove_file(&tmp_path); // Best effort; the rename error is the one worth reporting.
        return Err(e);
    }

    // Counted after the rename, so the number reports saves that actually
    // landed rather than saves that were attempted.
    #[cfg(feature = "audit")]
    WRITES.fetch_add(1, Ordering::Relaxed);

    Ok(())
}

/// Create a uniquely-named temporary file in `dir` alongside `target`.
///
/// The temporary must share a directory with the target: `rename` is only
/// atomic within one filesystem, and a temporary in `/tmp` would silently
/// degrade to a copy across a mount point — reintroducing exactly the partial
/// write this crate exists to prevent.
///
/// Uniqueness is enforced by `create_new`, which fails rather than truncating
/// an existing file. Deriving a name and trusting it to be free is how a
/// second saver ends up writing into the first's temporary.
fn create_temp_in(dir: &Path, target: &Path) -> io::Result<(fs::File, PathBuf)> {
    let stem = target.file_name().map_or_else(
        || "unnamed".to_string(),
        |n| n.to_string_lossy().to_string(),
    );
    let pid = std::process::id();

    for _ in 0..MAX_TEMP_ATTEMPTS {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        // Leading dot so the temporary is hidden, and a suffix that cannot be
        // mistaken for a document if one is ever left behind by a hard kill.
        let tmp_path = dir.join(format!(".{stem}.slate-save-{pid}-{n}"));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
        {
            Ok(file) => return Ok((file, tmp_path)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!(
            "could not create a temporary file next to {}",
            target.display()
        ),
    ))
}

/// [`write_atomically`] for text, which is what most callers have.
///
/// # Errors
///
/// As [`write_atomically`].
pub fn write_str_atomically(path: &Path, contents: &str) -> io::Result<()> {
    write_atomically(path, contents.as_bytes())
}

/// [`write_atomically`] for a name that must still be free: the write fails
/// with [`io::ErrorKind::AlreadyExists`] rather than replace anything.
///
/// For a program that picks a free name some time before it writes -- a
/// converter planning a queue of outputs, an exporter whose "save as" was
/// answered a minute ago. A name that was free when it was chosen is not free
/// when it is written merely because it once was, and [`write_atomically`]
/// would rename over whatever arrived in between: somebody's file, under the
/// very name the program had checked was not in use.
///
/// The contents go to a temporary beside the target and are flushed, as in
/// [`write_atomically`]. The temporary is then *linked* at the target's name,
/// which the filesystem refuses if the name is in use -- the check and the
/// claim are one operation, so nothing can arrive between them, and the name
/// is never seen holding part of the file. A filesystem without hard links
/// (FAT, exFAT) is served by claiming the name with an exclusive create and
/// renaming the temporary over that claim: just as unable to replace anything,
/// but a crash between the two leaves an empty file at the name.
///
/// # Errors
///
/// [`io::ErrorKind::AlreadyExists`] when anything has the name -- a file, a
/// directory, a symlink, dangling or not; otherwise the underlying error. On
/// any failure nothing at `path` has changed and no temporary is left behind.
pub fn write_new_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
    write_new_with(path, contents, |from, to| fs::hard_link(from, to))
}

/// [`write_new_atomically`] with its link step given, so that the route for a
/// filesystem without hard links can be tested on one that has them.
fn write_new_with(
    path: &Path,
    contents: &[u8],
    link: impl Fn(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    // Not canonicalised, unlike `write_atomically`: a symlink at the name is
    // something at the name, and following it would create a file wherever it
    // points -- a place the caller never chose.
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    let dir: PathBuf = parent.map_or_else(|| PathBuf::from("."), Path::to_path_buf);

    let (mut file, tmp_path) = create_temp_in(&dir, path)?;
    let written = file.write_all(contents).and_then(|()| file.sync_all());
    // Closed before the link or the rename: Windows refuses to rename a file
    // that is still open.
    drop(file);
    if let Err(e) = written {
        let _ = fs::remove_file(&tmp_path); // Best effort; the write error is the one worth reporting.
        return Err(e);
    }

    if link(&tmp_path, path).is_ok() {
        // The temporary is now a second name for the file at `path`.
        let _ = fs::remove_file(&tmp_path); // Best effort; a stray second name for a whole file loses nothing.
    } else if let Err(e) = claim_then_rename(&tmp_path, path) {
        // The link's own error is not the one reported. Either this
        // filesystem has no hard links, which the claim serves; or the name
        // is in use, or the folder is in trouble, and the claim meets that
        // again and says so itself.
        let _ = fs::remove_file(&tmp_path); // Best effort; the publish error is the one worth reporting.
        // Whatever the platform calls it. Windows answers an exclusive create
        // over a directory with "access denied", which would send a caller
        // looking at permissions for what is a name in use.
        if e.kind() != io::ErrorKind::AlreadyExists && fs::symlink_metadata(path).is_ok() {
            return Err(io::Error::from(io::ErrorKind::AlreadyExists));
        }
        return Err(e);
    }

    // Counted after the file is in place, as in `write_atomically`.
    #[cfg(feature = "audit")]
    WRITES.fetch_add(1, Ordering::Relaxed);

    Ok(())
}

/// Put a finished temporary at `target` on a filesystem without hard links:
/// claim the name with an exclusive create, which fails if anything has it,
/// then rename the temporary over the empty claim.
fn claim_then_rename(tmp_path: &Path, target: &Path) -> io::Result<()> {
    let claim = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    drop(claim);
    if let Err(e) = fs::rename(tmp_path, target) {
        // The claim is ours and empty: a name held by nothing anybody wrote.
        let _ = fs::remove_file(target); // Best effort; the rename error is the one worth reporting.
        return Err(e);
    }
    Ok(())
}

/// Copy `src` to `dest` so that `dest` never exists in a partial state.
///
/// `fs::copy` writes straight into the destination, so an interrupted copy
/// leaves a truncated file *at the destination's final name*. That is merely
/// bad for a plain file copy and actively dangerous for a content-addressed
/// store, where a file's presence at a hash-derived path is taken as proof
/// that the hash's content is there: one interrupted copy makes every future
/// deduplication against that hash hand back truncated data, forever, with no
/// error anywhere.
///
/// The copy therefore goes to a temporary in the destination's directory and
/// is renamed into place only once it is complete and flushed.
///
/// # Errors
///
/// Returns the underlying [`io::Error`] if the source cannot be read, the
/// destination directory cannot be written, or the rename fails. On any
/// failure `dest` is untouched and no temporary is left behind.
pub fn copy_atomically(src: &Path, dest: &Path) -> io::Result<u64> {
    let target = fs::canonicalize(dest).unwrap_or_else(|_| dest.to_path_buf());
    let parent = target.parent().filter(|p| !p.as_os_str().is_empty());
    let dir: PathBuf = parent.map_or_else(|| PathBuf::from("."), Path::to_path_buf);

    // Create the temporary through the same exclusive path the write uses, so
    // two concurrent copies of the same blob cannot share one temporary.
    let (file, tmp_path) = create_temp_in(&dir, &target)?;
    // `fs::copy` needs to open the destination itself, and it is worth keeping
    // rather than hand-rolling a read/write loop: it uses the platform's
    // accelerated path (`CopyFileEx`, `copy_file_range`) where one exists.
    drop(file);

    let copied = match fs::copy(src, &tmp_path) {
        Ok(n) => n,
        Err(e) => {
            let _ = fs::remove_file(&tmp_path); // Best effort; the copy error is the one worth reporting.
            return Err(e);
        }
    };

    // Flush before the rename, for the same reason as in `write_atomically`:
    // a rename that reaches the disk ahead of the data leaves a
    // correctly-named file full of nothing.
    //
    // Reopened for *write*: `sync_all` issues a flush, which Windows refuses
    // on a read-only handle with ERROR_ACCESS_DENIED. A read handle here made
    // every copy fail.
    if let Err(e) = fs::OpenOptions::new()
        .write(true)
        .open(&tmp_path)
        .and_then(|f| f.sync_all())
    {
        let _ = fs::remove_file(&tmp_path); // Best effort; the sync error is the one worth reporting.
        return Err(e);
    }

    if let Err(e) = fs::rename(&tmp_path, &target) {
        let _ = fs::remove_file(&tmp_path); // Best effort; the rename error is the one worth reporting.
        return Err(e);
    }

    // Counted after the rename, for the same reason as in `write_atomically`.
    #[cfg(feature = "audit")]
    COPIES.fetch_add(1, Ordering::Relaxed);

    Ok(copied)
}

#[cfg(test)]
mod tests {
    // A test that unwraps a failure should fail loudly at the line that did
    // it. The defensive lints exist to keep panics out of code that runs on a
    // user's data, which this is not.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// The cap bounds the read, and the true length is still reported.
    ///
    /// The allocation bound itself cannot be asserted from inside the process
    /// -- nothing here can watch a `Vec` that was never grown -- so what is
    /// pinned is the contract that goes with it: exactly `max` bytes come
    /// back, and `whole` is the file's real size rather than the read's.
    #[test]
    fn a_file_past_the_cap_returns_the_cap_and_reports_its_real_length() {
        let dir = std::env::temp_dir().join("slateos-safeio-bounded");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("big.bin");
        std::fs::write(&path, vec![7_u8; 4096]).expect("write");

        let got = read_capped(&path, 10).expect("read");
        assert_eq!(got.bytes.len(), 10, "the cap is what came back");
        assert_eq!(got.whole, 4096, "and the file's real size is reported");
        assert!(got.truncated);
        assert_eq!(got.note(10), "INCOMPLETE (10 of 4096 bytes read): ");

        let _ = std::fs::remove_file(&path);
    }

    /// A cut that lands inside a character keeps the character whole.
    ///
    /// The cap is 11 bytes into ten ASCII letters followed by `e`-acute, so it
    /// falls between that character's two bytes. Reading is done on bytes now,
    /// not on an already-decoded `String`, so this is the case that would
    /// otherwise return a fragment that is not text at all.
    #[test]
    fn a_cut_inside_a_character_backs_up_to_the_one_before_it() {
        let dir = std::env::temp_dir().join("slateos-safeio-bounded");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("accented.txt");
        let contents = format!("{}{}", "a".repeat(10), "\u{e9}".repeat(5));
        assert_eq!(
            contents.len(),
            20,
            "ten ASCII bytes and five two-byte characters"
        );
        std::fs::write(&path, &contents).expect("write");

        let got = read_to_string_capped(&path, 11).expect("read");
        assert_eq!(got.text, "a".repeat(10), "the half character was dropped");
        assert_eq!(got.whole, 20);
        assert!(got.truncated);

        let _ = std::fs::remove_file(&path);
    }

    /// Bytes that are not text, past the cap, no longer fail the read.
    ///
    /// This is the behaviour change that stopping at the cap brings, and it is
    /// the one worth having: a log whose far end is binary junk is still
    /// readable at the near end, which is the end the caller asked for.
    #[test]
    fn junk_past_the_cap_does_not_fail_a_bounded_read() {
        let dir = std::env::temp_dir().join("slateos-safeio-bounded");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("tail-is-junk.log");
        let mut contents = b"hello world".to_vec();
        contents.extend_from_slice(&[0xFF, 0xFE, 0xFF]);
        std::fs::write(&path, &contents).expect("write");

        let got = read_to_string_capped(&path, 5).expect("the read succeeds");
        assert_eq!(got.text, "hello");
        assert!(got.truncated);

        let _ = std::fs::remove_file(&path);
    }

    /// Bytes that are not text *within* the cap still fail, as before.
    ///
    /// The counterpart to the test above: relaxing what is checked past the
    /// cap must not relax what is checked inside it, or a caller that asked
    /// for a text file would be handed something else without a word.
    #[test]
    fn junk_inside_the_cap_still_fails_the_read() {
        let dir = std::env::temp_dir().join("slateos-safeio-bounded");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("not-text.bin");
        std::fs::write(&path, [0xFF_u8, 0xFE, 0xFF]).expect("write");

        let err = read_to_string_capped(&path, 1024).expect_err("not text");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);

        let _ = std::fs::remove_file(&path);
    }

    /// An under-cap read returns the file and says nothing was cut.
    #[test]
    fn a_short_file_is_read_whole_and_reports_no_truncation() {
        let dir = std::env::temp_dir().join("slateos-safeio-capped");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("short.txt");
        std::fs::write(&path, "hello\n").expect("write");

        let got = read_to_string_capped(&path, 1024).expect("read");
        assert_eq!(got.text, "hello\n");
        assert_eq!(got.whole, 6);
        assert!(!got.truncated);
        assert_eq!(got.note(1024), "", "nothing was cut, so say nothing");

        let _ = std::fs::remove_file(&path);
    }

    /// The cut lands on a character boundary.
    ///
    /// Slicing a `String` anywhere else panics. The cap here falls in the
    /// middle of a three-byte character on purpose.
    #[test]
    fn the_cut_moves_back_to_a_character_boundary() {
        let dir = std::env::temp_dir().join("slateos-safeio-capped");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("wide.txt");
        // "aa" then twenty three-byte characters.
        let body = format!("aa{}", "\u{65e5}".repeat(20));
        std::fs::write(&path, &body).expect("write");

        // 6 bytes in: "aa" plus one whole character (3) lands at 5, and 6 is
        // one byte into the second character.
        let got = read_to_string_capped(&path, 6).expect("read");
        assert!(got.truncated);
        assert_eq!(got.text, "aa\u{65e5}", "cut in the middle of a character");
        assert_eq!(got.whole, body.len());
        assert!(
            got.note(6).starts_with("INCOMPLETE ("),
            "a truncation must be reported first: {}",
            got.note(6)
        );
    }

    /// A cap larger than the file is not a truncation.
    #[test]
    fn a_cap_at_exactly_the_file_length_does_not_truncate() {
        let dir = std::env::temp_dir().join("slateos-safeio-capped");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("exact.txt");
        std::fs::write(&path, "abcd").expect("write");

        let got = read_to_string_capped(&path, 4).expect("read");
        assert!(!got.truncated, "a file that fits is not cut");
        assert_eq!(got.text, "abcd");
    }

    /// `to_last_line` drops a half-record.
    ///
    /// For line-oriented formats a partial final line is worse than a missing
    /// one, because a parser accepts it and the row silently loses columns.
    #[test]
    fn to_last_line_drops_a_partial_final_record() {
        let dir = std::env::temp_dir().join("slateos-safeio-capped");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("rows.csv");
        std::fs::write(&path, "a,b\n1,2\n3,4\n").expect("write");

        // Cut inside the third line.
        let got = read_to_string_capped(&path, 9)
            .expect("read")
            .to_last_line();
        assert_eq!(got.text, "a,b\n1,2", "the half row should be gone");
        assert!(got.truncated);
    }

    /// An untruncated read is left alone by `to_last_line`.
    #[test]
    fn to_last_line_does_not_touch_a_whole_file() {
        let dir = std::env::temp_dir().join("slateos-safeio-capped");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("whole.csv");
        std::fs::write(&path, "a,b\n1,2\n").expect("write");

        let got = read_to_string_capped(&path, 4096)
            .expect("read")
            .to_last_line();
        assert_eq!(
            got.text, "a,b\n1,2\n",
            "a complete file keeps its last line"
        );
    }

    /// A missing file is an error, not an empty read.
    #[test]
    fn a_missing_file_is_an_error_rather_than_an_empty_string() {
        let path = std::env::temp_dir().join("slateos-safeio-absent-file.txt");
        let _ = std::fs::remove_file(&path);
        assert!(read_to_string_capped(&path, 16).is_err());
    }

    use scratchdir::ScratchDir;

    /// A private temporary directory for one test, removed when the returned
    /// guard drops.
    ///
    /// The name used to carry the system clock in nanoseconds, which is not
    /// unique. `cargo test` runs a binary's tests as threads of one process,
    /// and the clock a thread reads is only refreshed on a timer interrupt, so
    /// every test that starts within the same tick draws the same tag and they
    /// share one directory. `ScratchDir` names itself from the process id and a
    /// per-process atomic counter, which is unique by construction.
    ///
    /// Bind the guard to a named local, never to `_`: a bare `_` drops it
    /// immediately and the directory is gone before the test's first line.
    fn temp_dir(label: &str) -> ScratchDir {
        ScratchDir::new(&format!("safeio_test_{label}"))
    }

    #[test]
    fn a_new_file_is_created_with_the_given_contents() {
        let scratch = temp_dir("new");
        let dir = scratch.dir().to_path_buf();
        let path = dir.join("doc.txt");

        write_atomically(&path, b"hello").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"hello");
    }

    #[test]
    fn an_existing_file_is_replaced_wholesale() {
        let scratch = temp_dir("replace");
        let dir = scratch.dir().to_path_buf();
        let path = dir.join("doc.txt");
        fs::write(&path, b"the old and much longer contents").unwrap();

        write_atomically(&path, b"new").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"new");
    }

    /// The whole point: a save that cannot complete must not damage the file
    /// it was saving. A directory standing where the target file should be
    /// makes the final rename fail on every platform, which is the closest
    /// portable stand-in for a disk filling up mid-write.
    #[test]
    fn a_failed_save_leaves_the_original_untouched() {
        let scratch = temp_dir("fail_keeps_original");
        let dir = scratch.dir().to_path_buf();
        let path = dir.join("doc.txt");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("marker"), b"still here").unwrap();

        let err = write_atomically(&path, b"replacement").unwrap_err();

        // The original is exactly as it was.
        assert!(path.is_dir(), "the target must not have been replaced");
        assert_eq!(fs::read(path.join("marker")).unwrap(), b"still here");
        let _ = err;
    }

    /// A failed save must not leave its temporary behind either. A stray
    /// full-size copy of the document beside it is how a failed save of a
    /// large file silently consumes its own size in disk space.
    #[test]
    fn a_failed_save_cleans_up_after_itself() {
        let scratch = temp_dir("fail_no_litter");
        let dir = scratch.dir().to_path_buf();
        let path = dir.join("doc.txt");
        fs::create_dir_all(&path).unwrap();

        let _ = write_atomically(&path, b"replacement");

        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains("slate-save"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temporaries left behind: {leftovers:?}"
        );
    }

    /// A successful save must not litter either.
    #[test]
    fn a_successful_save_leaves_only_the_file() {
        let scratch = temp_dir("clean");
        let dir = scratch.dir().to_path_buf();
        let path = dir.join("doc.txt");

        write_atomically(&path, b"contents").unwrap();

        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["doc.txt".to_string()]);
    }

    /// Two documents saved at once must not pick the same temporary name and
    /// write into each other's half-finished file.
    #[test]
    fn concurrent_saves_do_not_share_a_temporary() {
        let scratch = temp_dir("concurrent");
        let dir = scratch.dir().to_path_buf();
        let paths: Vec<PathBuf> = (0..8).map(|i| dir.join(format!("doc{i}.txt"))).collect();

        std::thread::scope(|s| {
            for (i, path) in paths.iter().enumerate() {
                s.spawn(move || {
                    let body = format!("contents of {i}").repeat(500);
                    write_atomically(path, body.as_bytes()).unwrap();
                });
            }
        });

        for (i, path) in paths.iter().enumerate() {
            let expected = format!("contents of {i}").repeat(500);
            assert_eq!(fs::read_to_string(path).unwrap(), expected);
        }
    }

    /// The same file saved repeatedly, which is what an editor with autosave
    /// does. Every save must land whole.
    #[test]
    fn repeated_saves_of_one_file_all_land() {
        let scratch = temp_dir("repeat");
        let dir = scratch.dir().to_path_buf();
        let path = dir.join("doc.txt");

        for i in 0..50 {
            write_str_atomically(&path, &format!("revision {i}")).unwrap();
            assert_eq!(fs::read_to_string(&path).unwrap(), format!("revision {i}"));
        }
    }

    #[test]
    fn an_empty_write_produces_an_empty_file() {
        let scratch = temp_dir("empty");
        let dir = scratch.dir().to_path_buf();
        let path = dir.join("doc.txt");
        fs::write(&path, b"previous").unwrap();

        write_atomically(&path, b"").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"");
    }

    #[test]
    fn a_copy_reproduces_the_source() {
        let scratch = temp_dir("copy_ok");
        let dir = scratch.dir().to_path_buf();
        let src = dir.join("src.bin");
        let dest = dir.join("dest.bin");
        let body: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
        fs::write(&src, &body).unwrap();

        let n = copy_atomically(&src, &dest).unwrap();

        assert_eq!(n, body.len() as u64);
        assert_eq!(fs::read(&dest).unwrap(), body);
    }

    /// The content-addressed-store case: a copy that cannot complete must not
    /// leave anything at the destination name, because a store treats the mere
    /// presence of that name as proof the content is there.
    #[test]
    fn a_failed_copy_leaves_nothing_at_the_destination() {
        let scratch = temp_dir("copy_fail");
        let dir = scratch.dir().to_path_buf();
        let dest = dir.join("blob");

        let err = copy_atomically(&dir.join("no_such_source"), &dest).unwrap_err();

        assert!(
            !dest.exists(),
            "a failed copy must not create the destination"
        );
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(leftovers.is_empty(), "left behind: {leftovers:?}");
        let _ = err;
    }

    /// A path with no directory component must still work — it means the
    /// current directory, not "no directory".
    #[test]
    fn a_bare_file_name_writes_to_the_current_directory() {
        let scratch = temp_dir("bare");
        let dir = scratch.dir().to_path_buf();
        let path = dir.join("bare.txt");
        // Exercised through a full path whose parent exists; the bare-name
        // branch is the `dir` fallback in `write_atomically`, checked here by
        // construction rather than by changing the process's cwd, which would
        // race every other test in this binary.
        write_atomically(&path, b"ok").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"ok");

        assert_eq!(
            Path::new("bare.txt")
                .parent()
                .filter(|p| !p.as_os_str().is_empty()),
            None,
            "a bare name has no usable parent, so the fallback is the branch taken"
        );
    }

    /// Every name in `dir`, sorted.
    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        names
    }

    /// A free name is written, and nothing else is left beside it -- in
    /// particular not the temporary, which the link leaves as a second name
    /// for the same file.
    #[test]
    fn a_free_name_is_written_and_nothing_else_is_left() {
        let scratch = temp_dir("new_free");
        let dir = scratch.dir().to_path_buf();
        let path = dir.join("out.wav");

        write_new_atomically(&path, b"RIFF").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"RIFF");
        assert_eq!(names_in(&dir), vec!["out.wav".to_string()]);
    }

    /// The point of the function: a name in use is refused, and what has it
    /// is exactly as it was.
    #[test]
    fn a_name_in_use_is_refused_and_left_as_it_was() {
        let scratch = temp_dir("new_taken");
        let dir = scratch.dir().to_path_buf();
        let path = dir.join("out.wav");
        fs::write(&path, b"somebody's file").unwrap();

        let err = write_new_atomically(&path, b"a conversion").unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&path).unwrap(), b"somebody's file");
        assert_eq!(
            names_in(&dir),
            vec!["out.wav".to_string()],
            "no temporary left"
        );
    }

    /// A directory has a name too.
    #[test]
    fn a_directory_at_the_name_is_refused() {
        let scratch = temp_dir("new_dir");
        let dir = scratch.dir().to_path_buf();
        let path = dir.join("out.wav");
        fs::create_dir(&path).unwrap();

        let err = write_new_atomically(&path, b"a conversion").unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert!(path.is_dir());
        assert_eq!(names_in(&dir), vec!["out.wav".to_string()]);
    }

    /// Without hard links (FAT, exFAT) the name is claimed by an exclusive
    /// create instead: the file still lands whole, and a name in use is still
    /// refused and untouched.
    #[test]
    fn without_hard_links_the_name_is_still_claimed_exclusively() {
        let scratch = temp_dir("new_nolink");
        let dir = scratch.dir().to_path_buf();
        let path = dir.join("out.wav");
        let no_links = |_: &Path, _: &Path| Err(io::Error::from(io::ErrorKind::Unsupported));

        write_new_with(&path, b"first", no_links).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"first");
        assert_eq!(names_in(&dir), vec!["out.wav".to_string()]);

        let err = write_new_with(&path, b"second", no_links).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&path).unwrap(), b"first");
        assert_eq!(names_in(&dir), vec!["out.wav".to_string()]);
    }

    /// A folder that is not there is reported, and not made.
    #[test]
    fn a_missing_folder_is_reported_and_not_made() {
        let scratch = temp_dir("new_nofolder");
        let dir = scratch.dir().to_path_buf();
        let missing = dir.join("no-such-folder").join("out.wav");

        let err = write_new_atomically(&missing, b"x").unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(
            names_in(&dir).is_empty(),
            "left behind: {:?}",
            names_in(&dir)
        );
    }

    /// A symlink at the name is something at the name, dangling or not; and
    /// it is not followed, so nothing appears where it points.
    #[cfg(unix)]
    #[test]
    fn a_symlink_at_the_name_is_refused_and_not_followed() {
        let scratch = temp_dir("new_symlink");
        let dir = scratch.dir().to_path_buf();
        let path = dir.join("out.wav");
        let pointee = dir.join("elsewhere.wav");
        std::os::unix::fs::symlink(&pointee, &path).unwrap();

        let err = write_new_atomically(&path, b"x").unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert!(!pointee.exists(), "the link was not followed");
        assert!(
            fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}
