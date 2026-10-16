//! Following log files as they grow: `journalctl -f` and `syslogd follow`.
//!
//! A follower reads, each round, only what was appended to each file since
//! the round before, and hands back the lines that completes. Two things
//! make that more than an offset per file name:
//!
//! * **A rotation renames every file under the reader.** The live log
//!   becomes `.1`, `.1` becomes `.2`, and a new live log is started. What a
//!   follower has read of a file goes with the file, so files are known here
//!   by identity -- device and inode -- and not by name. Keyed by name, as
//!   `journalctl -f` was until 2026-10-07, each name's old offset was applied
//!   to whatever file had that name now, and every rotation showed lines a
//!   second time or skipped them.
//! * **A record can be caught half-appended.** A last line whose newline has
//!   not been written yet is held back, and finished by the round that reads
//!   the rest of it, rather than shown short with its remainder shown later
//!   as a line of its own.
//!
//! A file shorter than what has been read of it was truncated in place, and
//! is read again from its start. A file *rewritten* -- `journalctl
//! --vacuum-*` renames a new file over the log (design-decisions §1037) -- is
//! a new file, and is read whole: the records it kept are shown again, as
//! `tail -F` shows a replaced file's contents again.
//!
//! Where there are no inodes to tell files apart -- the Windows host the
//! unit tests also run on -- a file is known by its name.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

/// How far back [`Follow::skip_to_end`] looks for the start of a last line
/// still being written. A record longer than this that is mid-append at that
/// moment is not shown; every other is.
pub(crate) const LAST_LINE_WINDOW: u64 = 64 * 1024;

/// Log files being followed, each known by its identity. See the module
/// documentation.
#[derive(Debug, Default)]
pub struct Follow {
    files: BTreeMap<Key, Tail>,
    /// The current round, for [`Follow::end_round`].
    round: u64,
}

/// What a [`Follow`] knows of one file.
#[derive(Debug, Default)]
struct Tail {
    /// Bytes of the file read so far.
    offset: u64,
    /// The start of a line whose newline has not been written yet.
    partial: Vec<u8>,
    /// The round the file was last seen in.
    round: u64,
}

impl Follow {
    /// A follower that has read nothing yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What has been appended to the file at `path` since this last read it
    /// -- all of it, for a file not read before -- as bytes that end where a
    /// line ends. With `hold_back`, a last line with no newline yet is kept
    /// back for the read that finishes it; without, it is included, for a
    /// listing that will not read again. `Ok(None)` when there is no such
    /// file.
    ///
    /// # Errors
    ///
    /// The file cannot be opened, examined or read. Nothing more of it
    /// counts as read, so the next read starts where this one would have.
    pub fn read(&mut self, path: &Path, hold_back: bool) -> io::Result<Option<Vec<u8>>> {
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                // Unreadable this round but still there: what was read of it
                // is kept for when it can be read again, rather than
                // forgotten and the whole file shown a second time.
                if let Ok(meta) = fs::metadata(path) {
                    self.seen(&key(path, &meta));
                }
                return Err(e);
            }
        };
        let meta = file.metadata()?;
        let round = self.round;
        let tail = self.files.entry(key(path, &meta)).or_default();
        tail.round = round;
        if meta.len() < tail.offset {
            // Truncated in place: whatever it holds now is new.
            *tail = Tail {
                round,
                ..Tail::default()
            };
        }
        file.seek(SeekFrom::Start(tail.offset))?;
        let mut fresh = Vec::new();
        file.read_to_end(&mut fresh)?;
        tail.offset = tail.offset.saturating_add(len_u64(&fresh));
        let mut bytes = std::mem::take(&mut tail.partial);
        bytes.extend_from_slice(&fresh);
        if hold_back {
            tail.partial = bytes.split_off(complete_len(&bytes));
        }
        Ok(Some(bytes))
    }

    /// Take the file at `path` as read up to its last complete line, without
    /// reading what comes before: following it starts from there, and a
    /// record being appended at that moment is shown once it is finished.
    /// Nothing happens when there is no such file.
    ///
    /// # Errors
    ///
    /// The file cannot be opened, examined or read.
    pub fn skip_to_end(&mut self, path: &Path) -> io::Result<()> {
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        let meta = file.metadata()?;
        let start = meta.len().saturating_sub(LAST_LINE_WINDOW);
        file.seek(SeekFrom::Start(start))?;
        let mut last = Vec::new();
        file.read_to_end(&mut last)?;
        let end = start.saturating_add(len_u64(&last));
        let partial = match last.iter().rposition(|&b| b == b'\n') {
            Some(nl) => last.split_off(nl.saturating_add(1)),
            // No line ends in the window: a line longer than it, or a file
            // of one unfinished line. Following starts at the end.
            None if start > 0 => Vec::new(),
            None => last,
        };
        let round = self.round;
        self.files.insert(
            key(path, &meta),
            Tail {
                offset: end,
                partial,
                round,
            },
        );
        Ok(())
    }

    /// Forget every file not seen since the last call -- removed, or no
    /// longer among those followed -- so that a new file given its identity
    /// is read from its start. Call once a round, after reading each file.
    pub fn end_round(&mut self) {
        let round = self.round;
        self.files.retain(|_, tail| tail.round == round);
        self.round = round.wrapping_add(1);
    }

    /// Count the file known as `key`, if it is known, as seen this round.
    fn seen(&mut self, key: &Key) {
        let round = self.round;
        if let Some(tail) = self.files.get_mut(key) {
            tail.round = round;
        }
    }
}

/// The lines of `bytes`: split at each newline, a final newline ending the
/// last line rather than starting an empty one. What [`Follow::read`] returns
/// is read this way, and so is a whole log file.
pub fn lines(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    let body = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    let empty = bytes.is_empty();
    body.split(|&b| b == b'\n').filter(move |_| !empty)
}

/// How much of `bytes` is complete lines: up to and including the last
/// newline.
fn complete_len(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |nl| nl.saturating_add(1))
}

fn len_u64(bytes: &[u8]) -> u64 {
    u64::try_from(bytes.len()).unwrap_or(u64::MAX)
}

/// What a file is known by: its device and inode.
#[cfg(unix)]
type Key = (u64, u64);

#[cfg(unix)]
fn key(_path: &Path, meta: &fs::Metadata) -> Key {
    use std::os::unix::fs::MetadataExt;
    (meta.dev(), meta.ino())
}

/// What a file is known by where there are no inodes: its name.
#[cfg(not(unix))]
type Key = std::path::PathBuf;

#[cfg(not(unix))]
fn key(path: &Path, _meta: &fs::Metadata) -> Key {
    path.to_path_buf()
}
