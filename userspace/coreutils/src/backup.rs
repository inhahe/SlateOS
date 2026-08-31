//! Emacs-style backup names: what `cp -b`, `mv -b`, `ln -b` and `install -b`
//! move the destination aside to.
//!
//! This is gnulib's `backupfile.c` and `backup-find.c`. It is in this crate
//! rather than in one utility for the reason most things here are: the four
//! programs that make backups must make the *same* backup, because a person
//! learns the rule once. `cp -b f g` and `mv -b f g` both have to produce
//! `g~`, and `VERSION_CONTROL=numbered` has to mean the same thing to both, or
//! the option is not a feature but a lottery.
//!
//! Three parts, none of which is guessable from the option's name:
//!
//! - **The control word** ([`BackupType`], [`control`]). `--backup=numbered`,
//!   but also `--backup=t`, `--backup=nu`, and — when the option is given bare
//!   — the `VERSION_CONTROL` environment variable, and — when *that* is unset
//!   or empty — `existing`. The words are matched as prefixes with ambiguity
//!   judged by value rather than spelling, which is [`getopt`]'s
//!   [`argmatch`](crate::getopt::Program::argmatch) and is why this module
//!   hands it a table rather than parsing the word itself: `--backup=n` is
//!   ambiguous (`none` and `numbered` disagree) while `--backup=no` is not,
//!   and nobody re-derives that rule correctly by hand.
//!
//! - **The suffix** ([`suffix`]). `--suffix`/`-S`, else `$SIMPLE_BACKUP_SUFFIX`,
//!   else `~`. The validity test is upstream's and is stranger than it looks:
//!   a suffix is rejected — silently, falling back to `~` — when it is empty
//!   or when it is not its own last path component. So `-S .bak` is fine,
//!   `-S ''` quietly becomes `~`, `-S a/b` quietly becomes `~`, and `-S 'a/'`
//!   is *accepted*, because a trailing slash does not start a new component.
//!
//! - **The name** ([`Backup::find_name`], [`Backup::rename`]). For a simple
//!   backup that is the file name with the suffix stuck on. For a numbered one
//!   it is `NAME.~N~` where `N` is one past the highest that already exists —
//!   which means reading the destination's directory, and comparing version
//!   numbers *as strings* rather than as integers, so that a directory
//!   containing `f.~99999999999999999999~` does not overflow anything. That
//!   string comparison is why the increment carries by prepending a `0` before
//!   it adds one: `f.~9~` yields `f.~10~`, and the buffer has to grow by a
//!   digit to say so.
//!
//! The two rules that exist for safety rather than for tidiness are worth
//! naming, because both are invisible until they fire.
//!
//! The first is that a numbered rename is attempted with `RENAME_NOREPLACE`,
//! not a plain one. Two `cp -b --backup=numbered` runs against one directory
//! both scan, both decide on `.~4~`, and a plain rename would let the second
//! destroy the first's backup — the file the whole option exists to preserve.
//! With the flag the loser gets `EEXIST`, rescans, and takes `.~5~`. Our
//! kernel does not implement the flag yet (`posix::file::renameat2` answers
//! `EINVAL` for any non-zero flags), so this falls back to gnulib's own
//! non-atomic emulation — an `lstat` and then a rename, with the race back.
//! That is what GNU does on a pre-3.15 Linux and is not a reason to skip the
//! flag: the call is already written for the day the kernel grows it. Logged
//! as `B-NUMBERED-BACKUPS-RACE-WITHOUT-RENAME-NOREPLACE` in `known-issues.md`.
//!
//! The second is the length check ([`check_extension`]). Appending `.~1~` to a
//! name that is already near the filesystem's per-component limit produces a
//! name the filesystem will refuse, and the refusal would arrive as a confusing
//! `File name too long` from a program the user did not ask to create a file.
//! Upstream shortens instead: the whole extension collapses to a single `~`,
//! and if that is still too long a byte of the original name goes too. The
//! shortened name can collide with a name already there, which is the *other*
//! reason the rename loop exists — but a shortened name cannot be made longer
//! by rescanning, so a collision on a shortened name is a hard failure rather
//! than a retry.
//!
//! [`getopt`]: crate::getopt

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::getopt::{Error, Program};
use crate::pathname::{base_len, last_component_offset};
use crate::quote::{os_bytes, os_from_bytes};

/// When to make a backup, and of what shape.
///
/// gnulib's `enum backup_type`. The names are upstream's meanings rather than
/// upstream's spellings: `numbered_existing_backups` is [`Self::NumberedExisting`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupType {
    /// Never. What a utility has when `-b` was not given — and also what
    /// `--backup=none` means, which is why the option can turn itself off.
    None,
    /// Always `NAME` + suffix, overwriting a previous backup of the same name.
    Simple,
    /// Numbered where numbered backups already exist, simple otherwise. The
    /// default, and the one bare `-b` selects.
    NumberedExisting,
    /// Always `NAME.~N~`.
    Numbered,
}

/// The words `--backup` and `$VERSION_CONTROL` accept, in upstream's order.
///
/// The order is observable: it is the order the words are listed in when one of
/// them is rejected, and synonyms are grouped by sharing a value, so
/// `‘none’, ‘off’` prints on one line. gnulib's comment on the same table — "in
/// a series of synonyms, present the most meaningful first" — is the reason
/// `none` precedes `off` rather than any alphabetical rule.
pub const CONTROL_WORDS: &[(&str, BackupType)] = &[
    ("none", BackupType::None),
    ("off", BackupType::None),
    ("simple", BackupType::Simple),
    ("never", BackupType::Simple),
    ("existing", BackupType::NumberedExisting),
    ("nil", BackupType::NumberedExisting),
    ("numbered", BackupType::Numbered),
    ("t", BackupType::Numbered),
];

/// What a suffix is when nobody chose one, and when the choice was not usable.
pub const DEFAULT_SUFFIX: &[u8] = b"~";

/// The context these words are rejected *for*, in the diagnostic.
///
/// Measured: `cp --backup=zz` says `invalid argument ‘zz’ for ‘backup type’` —
/// the phrase, not the option's own name, which is the one place in this crate
/// where those differ. Upstream passes it as `_("backup type")` from all four
/// callers, so all four say the same thing.
const OPTION_CONTEXT: &str = "backup type";

/// The context when the word came from the environment rather than the option.
const ENV_CONTEXT: &str = "$VERSION_CONTROL";

/// gnulib's `xget_version`: which backup shape was asked for.
///
/// `given` is `--backup`'s argument, which is optional — `None` for bare `-b`.
/// An argument that is present but *empty* (`--backup=`) is treated as absent,
/// as upstream's `version && *version` does, so it too falls through to the
/// environment.
///
/// Note that this answers the question only for a utility that was *given* the
/// option. A caller that was not must use [`BackupType::None`] without calling
/// here, because `$VERSION_CONTROL` alone never enables backups.
///
/// # Errors
///
/// The word matching none of [`CONTROL_WORDS`], or several that disagree. The
/// error carries status 1, as every `argmatch` failure does.
pub fn control(program: Program, given: Option<&OsStr>) -> Result<BackupType, Error> {
    if let Some(word) = given {
        let bytes = os_bytes(word);
        if !bytes.is_empty() {
            return program.argmatch(&bytes, OPTION_CONTEXT, CONTROL_WORDS);
        }
    }
    let Some(from_env) = std::env::var_os("VERSION_CONTROL") else {
        return Ok(BackupType::NumberedExisting);
    };
    let bytes = os_bytes(&from_env);
    if bytes.is_empty() {
        return Ok(BackupType::NumberedExisting);
    }
    program.argmatch(&bytes, ENV_CONTEXT, CONTROL_WORDS)
}

/// gnulib's `set_simple_backup_suffix`: the string a simple backup ends in.
///
/// `explicit` is `--suffix`/`-S`'s argument. When it is absent the environment
/// variable `SIMPLE_BACKUP_SUFFIX` is consulted, and when neither is usable the
/// answer is `~`.
///
/// "Usable" is upstream's test — non-empty, and equal to its own last component
/// — and it fails *silently*, which is worth knowing when a `-S ''` appears not
/// to have been read. The test's odd corner is that a trailing slash is not a
/// component boundary, so `-S 'x/'` is accepted and produces backups named
/// `f x/`… which is to say, upstream does not defend against a suffix
/// containing a separator so much as against one *starting* a new component.
#[must_use]
pub fn suffix(explicit: Option<&OsStr>) -> Vec<u8> {
    let chosen = match explicit {
        Some(given) => Some(os_bytes(given).into_owned()),
        // Only consulted when the option was absent: `-S ''` is a choice, and
        // an unusable one, rather than a request to ask the environment.
        None => std::env::var_os("SIMPLE_BACKUP_SUFFIX")
            .map(|from_env| os_bytes(&from_env).into_owned()),
    };
    match chosen {
        Some(s) if !s.is_empty() && last_component_offset(&s) == 0 => s,
        _ => DEFAULT_SUFFIX.to_vec(),
    }
}

/// A backup policy: the shape and the suffix together.
///
/// The two are one value here where upstream keeps the type in each utility's
/// options struct and the suffix in a global, because a caller that has one
/// always needs the other — `Simple` without a suffix cannot name anything, and
/// `Numbered` still needs the suffix for the "would this backup destroy the
/// source?" check that `cp` makes before it starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Backup {
    kind: BackupType,
    suffix: Vec<u8>,
}

/// [`Backup::disabled`], so that a utility's options struct can `derive(Default)`
/// and get "no `-b` was given" rather than something subtly different.
///
/// Not `derive`d, and the difference is not cosmetic: the derived value would
/// pair `BackupType::None` with an *empty* suffix, and an empty suffix makes
/// every name look like its own backup to `cp`'s "backing up X might destroy
/// the source" check — which reads the suffix before it looks at the type. See
/// [`Backup::disabled`].
impl Default for Backup {
    fn default() -> Self {
        Self::disabled()
    }
}

impl Backup {
    /// A policy from a resolved [`control`] and [`suffix`] pair.
    #[must_use]
    pub fn new(kind: BackupType, suffix: Vec<u8>) -> Self {
        Self { kind, suffix }
    }

    /// The policy of a utility that was not given `-b`.
    ///
    /// The suffix is still `~` rather than empty, because a caller may consult
    /// it — `cp`'s "backing up X might destroy source" check reads the suffix
    /// before it looks at the type — and an empty one would make every name
    /// look like its own backup.
    #[must_use]
    pub fn disabled() -> Self {
        Self::new(BackupType::None, DEFAULT_SUFFIX.to_vec())
    }

    /// Which shape of backup this makes.
    #[must_use]
    pub const fn kind(&self) -> BackupType {
        self.kind
    }

    /// Whether any backup is made at all — `x->backup_type != no_backups`.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.kind != BackupType::None
    }

    /// The suffix a simple backup ends in.
    #[must_use]
    pub fn simple_suffix(&self) -> &[u8] {
        &self.suffix
    }

    /// gnulib's `find_backup_file_name`: the name a backup of `file` would
    /// take, without making it.
    ///
    /// Do not call this when [`enabled`](Self::enabled) is false; upstream says
    /// the same, and the answer would be a simple backup rather than nothing.
    ///
    /// # Errors
    ///
    /// Only the length check can fail here, and only by way of a name this
    /// process cannot represent; the directory scan swallows its own failures
    /// on purpose, because "I could not read the directory" and "there are no
    /// numbered backups" lead to the same next step.
    pub fn find_name(&self, file: &Path) -> io::Result<PathBuf> {
        self.build(file, false).map(|(name, _)| name)
    }

    /// gnulib's `backup_file_rename`: move `file` aside and answer where it
    /// went.
    ///
    /// # Errors
    ///
    /// The rename's own failure. `NotFound` means `file` did not exist, which
    /// every caller treats as "nothing to back up" rather than as an error —
    /// upstream's `else if (errno != ENOENT)`.
    pub fn rename(&self, file: &Path) -> io::Result<PathBuf> {
        self.build(file, true).map(|(name, _)| name)
    }

    /// The body of both, which is upstream's `backupfile_internal`.
    ///
    /// The second half of the answer is the `extended` flag — true when the
    /// name was *not* shortened by the length check — and it exists because it
    /// decides what an `EEXIST` from the rename means. It is returned rather
    /// than kept private so that the loop below reads as the upstream one does.
    fn build(&self, file: &Path, do_rename: bool) -> io::Result<(PathBuf, bool)> {
        let raw = os_bytes(file.as_os_str());
        let base_offset = last_component_offset(&raw);
        let base = raw.get(base_offset..).unwrap_or_default();
        // Trailing slashes are decoration: `cp -b d/ e/` backs up `e`, not a
        // name ending in a separator. `base_len` is what drops them, and the
        // result is a *prefix length* of the original rather than a new string,
        // which is what lets the buffer be rebuilt from `raw` on every retry.
        let filelen = base_offset.saturating_add(base_len(base));

        // The directory the numbered scan reads and the rename happens in.
        // Upstream opens it once and works relative to the descriptor; we work
        // by path, so this is only needed for `read_dir` and for the length
        // limit.
        let parent = raw.get(..base_offset).unwrap_or_default();
        let dir = if parent.is_empty() {
            PathBuf::from(".")
        } else {
            PathBuf::from(os_from_bytes(parent))
        };

        let mut kind = self.kind;
        let mut name_max: Option<usize> = None;
        let mut previously_taken: Option<Vec<u8>> = None;
        loop {
            let mut buf: Vec<u8> = raw.get(..filelen).unwrap_or_default().to_vec();
            let mut extended = true;

            if kind == BackupType::Simple {
                buf.extend_from_slice(&self.suffix);
            } else {
                match numbered_backup(&dir, &mut buf, filelen, base_offset) {
                    Scan::SameLength => {}
                    // No numbered backup exists, so `existing` means a simple
                    // one from here on — including on a retry, which is why the
                    // policy is narrowed rather than the name alone.
                    Scan::New => {
                        if kind == BackupType::NumberedExisting {
                            kind = BackupType::Simple;
                            buf.truncate(filelen);
                            buf.extend_from_slice(&self.suffix);
                        }
                        extended = check_extension(&mut buf, filelen, b'~', &dir, &mut name_max);
                    }
                    // A digit longer than any name the directory is known to
                    // hold, so the length is not yet known to be acceptable.
                    Scan::Longer => {
                        extended = check_extension(&mut buf, filelen, b'~', &dir, &mut name_max);
                    }
                }
            }

            // Upstream retries on `EEXIST` for as long as the name was not
            // shortened, on the reasoning that a rescan finds a higher number.
            // That holds only if the rescan can *see* the name that collided —
            // and in a directory that is writable but not readable it cannot,
            // so upstream spins forever on `--backup=numbered` there. Requiring
            // the retry to have produced a different name is the same rule
            // stated in terms of progress rather than of intent, and it cannot
            // change the outcome in any case where upstream terminates.
            if previously_taken.as_deref() == Some(buf.as_slice()) {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "backup file already exists",
                ));
            }

            let made = PathBuf::from(os_from_bytes(&buf));
            if !do_rename {
                return Ok((made, extended));
            }

            // A simple backup deliberately *replaces* a previous one, so it
            // renames without the flag; a numbered one must not, because the
            // name it chose was chosen from a scan that another process may
            // have invalidated.
            let replace = if kind == BackupType::Simple {
                Replace::Yes
            } else {
                Replace::No
            };
            match rename_maybe_noreplace(file, &made, replace) {
                Ok(()) => return Ok((made, extended)),
                // The name was taken between the scan and the rename. Rescanning
                // finds a higher number — unless the name was shortened, which
                // rescanning cannot change, so that case falls through as the
                // failure it is.
                Err(e) if extended && e.kind() == io::ErrorKind::AlreadyExists => {
                    previously_taken = Some(buf);
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// What [`numbered_backup`] found, which decides whether the name it built
/// needs its length checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scan {
    /// A numbered backup exists and the new name is the same length as it, so
    /// the filesystem has already accepted a name that long.
    SameLength,
    /// A numbered backup exists but the new name is a digit longer — the `.~9~`
    /// to `.~10~` step — so the length is not yet known to be acceptable.
    Longer,
    /// No numbered backup exists.
    New,
}

/// gnulib's `numbered_backup`: replace `buf`'s tail with the next free
/// `.~N~`.
///
/// On entry `buf` is the file name truncated to `filelen`; on return it is that
/// name with a version extension appended. The comparison is done on the digit
/// *strings* rather than on parsed integers, which is upstream's stated reason —
/// a version number in a file name is attacker-chosen and has no width limit,
/// and there is nothing to be gained by giving it one.
///
/// A directory that cannot be read is reported as [`Scan::New`], matching
/// upstream: the caller's next step (fall back to a simple backup, or use
/// `.~1~`) is the same as it would be for an empty directory, and there is no
/// diagnostic to attach the failure to.
fn numbered_backup(dir: &Path, buf: &mut Vec<u8>, filelen: usize, base_offset: usize) -> Scan {
    let baselen = filelen.saturating_sub(base_offset);
    buf.truncate(filelen);
    buf.extend_from_slice(b".~1~");

    let Ok(entries) = fs::read_dir(dir) else {
        return Scan::New;
    };

    let mut result = Scan::New;
    let mut versionlenmax = 1usize;
    for entry in entries {
        // A read failure mid-walk stops the scan and keeps the highest number
        // seen so far, which is upstream's "if an I/O or other read error
        // occurs, use the highest backup number that was found".
        let Ok(entry) = entry else { break };
        let name = os_bytes(&entry.file_name()).into_owned();

        // `NAME.~1~` is the shortest thing that could match.
        if name.len() < baselen.saturating_add(4) {
            continue;
        }
        // The `NAME.~` prefix, read out of the buffer so that the comparison is
        // against the same bytes the answer will be built from.
        let prefix_len = baselen.saturating_add(2);
        let want = buf
            .get(base_offset..base_offset.saturating_add(prefix_len))
            .unwrap_or_default();
        if name.get(..prefix_len) != Some(want) {
            continue;
        }
        let tail = name.get(prefix_len..).unwrap_or_default();

        // A version starts at 1, never at 0, and never has a leading zero —
        // which is what makes the string comparison below a valid ordering.
        let Some(&first) = tail.first() else { continue };
        if !(b'1'..=b'9').contains(&first) {
            continue;
        }
        let versionlen = tail.iter().take_while(|c| c.is_ascii_digit()).count();
        let all_9s = tail
            .get(..versionlen)
            .is_some_and(|digits| digits.iter().all(|&c| c == b'9'));
        // Exactly one `~` after the digits, and nothing after that.
        if tail.get(versionlen..) != Some(b"~".as_slice()) {
            continue;
        }

        // Is this larger than what the buffer already holds? Longer wins
        // outright; equal length is decided bytewise, and `<=` rather than `<`
        // because the buffer holds one *past* the last number accepted.
        let held = buf
            .get(filelen.saturating_add(2)..)
            .and_then(|rest| rest.get(..versionlen));
        let larger = match versionlenmax.cmp(&versionlen) {
            std::cmp::Ordering::Less => true,
            std::cmp::Ordering::Equal => {
                held.is_some_and(|h| h <= tail.get(..versionlen).unwrap_or_default())
            }
            std::cmp::Ordering::Greater => false,
        };
        if !larger {
            continue;
        }

        versionlenmax = versionlen.saturating_add(usize::from(all_9s));
        result = if all_9s {
            Scan::Longer
        } else {
            Scan::SameLength
        };

        buf.truncate(filelen);
        buf.extend_from_slice(b".~");
        // The carry digit, written before the copy so that incrementing
        // `999` in place turns `0999` into `1000` without a second pass.
        if all_9s {
            buf.push(b'0');
        }
        buf.extend_from_slice(tail.get(..versionlen.saturating_add(1)).unwrap_or_default());
        increment_digits(buf, filelen.saturating_add(2));
    }
    result
}

/// Add one to the decimal digit run that starts at `from` and ends before the
/// buffer's final byte (which is the closing `~`).
///
/// The run never consists entirely of nines when this is called: its caller
/// prepends a `0` in exactly that case, so the carry always stops.
fn increment_digits(buf: &mut [u8], from: usize) {
    let mut i = buf.len().saturating_sub(1);
    while i > from {
        i = i.saturating_sub(1);
        match buf.get_mut(i) {
            Some(d) if *d == b'9' => *d = b'0',
            Some(d) => {
                *d = d.saturating_add(1);
                return;
            }
            None => return,
        }
    }
}

/// gnulib's `check_extension`: shorten a name that the filesystem would refuse.
///
/// `buf` holds the name with its extension already appended; `filelen` is how
/// long it was before. Returns true when nothing had to change — which is the
/// signal the rename loop reads as "a retry could pick a different name".
///
/// The replacement is deliberately blunt: the whole extension becomes the
/// single byte `e`, and if the original name was itself at or over the limit a
/// byte of *it* goes too. Upstream's reasoning is that a name this long is
/// already unusual and the alternative — failing — is worse; ours is that
/// matching it is the point.
fn check_extension(
    buf: &mut Vec<u8>,
    filelen: usize,
    e: u8,
    dir: &Path,
    cached: &mut Option<usize>,
) -> bool {
    let base_offset = last_component_offset(buf);
    let baselen = base_len(buf.get(base_offset..).unwrap_or_default());

    // Upstream starts from the length every host it configures for is known to
    // allow, and consults the filesystem whenever the new name is longer than
    // the smallest limit a conforming host may impose. Those are two different
    // numbers — 255 and 14 — and reading them as one is the mistake this
    // comment exists to prevent: with the threshold at 255 the lookup would be
    // reached only by names already past the edge, and every name in this
    // function would keep its full `.~N~`. It is 14, so the lookup is reached
    // by every name worth backing up, and the answer it gives is what decides
    // the length. See [`NAME_MAX_MINIMUM`].
    let mut baselen_max = LONG_FILE_NAME_MAX;
    if NAME_MAX_MINIMUM < baselen {
        baselen_max = *cached.get_or_insert_with(|| name_max(dir));
    }

    if baselen <= baselen_max {
        return true;
    }
    // The original name's own last component, which is what gets truncated —
    // the extension is discarded entirely rather than shortened.
    let mut keep = filelen.saturating_sub(base_offset);
    if baselen_max <= keep {
        keep = baselen_max.saturating_sub(1);
    }
    buf.truncate(base_offset.saturating_add(keep));
    buf.push(e);
    false
}

/// The smallest per-component limit a conforming host may impose: the length
/// below which no filesystem can object, so no filesystem need be asked.
///
/// Upstream spells this `_XOPEN_NAME_MAX` where that exists and
/// `_POSIX_NAME_MAX` — 14 — where it does not. **glibc does not define
/// `_XOPEN_NAME_MAX`**, checked with `gcc -E -dM` over `<limits.h>` and
/// `<unistd.h>` under `_GNU_SOURCE`: it has `NAME_MAX`, `_POSIX_NAME_MAX`,
/// `HOST_NAME_MAX` and a dozen others, and not that one. So on the host GNU
/// `cp` is built for, this is 14, and it is 14 here.
///
/// The number is deliberately not 255, and the difference is visible in the
/// name a backup gets. It is only a threshold — the value that decides whether
/// [`name_max`] is called at all — and setting it to 255 stops that call ever
/// happening for a name that fits in a directory entry, which in turn stops
/// the one-byte adjustment in [`name_max`] from ever applying. That was this
/// module's first reading, and a differential run against GNU disproved it: a
/// 251-byte name backs up to 252 bytes, not 255.
///
/// It doubles as the answer when the filesystem reports no limit at all, which
/// is upstream's choice too — an unknown limit is not a licence to build a name
/// of any length.
const NAME_MAX_MINIMUM: usize = 14;

/// The starting guess for a component's limit, before the filesystem is asked.
///
/// Upstream's `HAVE_LONG_FILE_NAMES ? 255 : NAME_MAX_MINIMUM`, and every host
/// either project configures for sets that macro. It is only ever the final
/// answer for a name of 14 bytes or fewer, which cannot exceed it anyway.
const LONG_FILE_NAME_MAX: usize = 255;

/// `pathconf (DIR, _PC_NAME_MAX)`, with upstream's reading of its three answers.
///
/// `pathconf` reports "no limit" by returning -1 without touching `errno`, and
/// a genuine failure by returning -1 and setting it — a distinction that cannot
/// be made without clearing `errno` first, which is why that is done here. So
/// the three cases are: a real limit; no limit at all, where the conservative
/// floor is used because we cannot tell how long is too long; and a failure,
/// where no limit is imposed and the rename reports whatever the filesystem
/// thinks.
///
/// Upstream writes all three as `name_max -= !errno` followed by one
/// three-way conditional, and that one line **also takes a byte off a limit it
/// successfully read** — `errno` is zero on success too. It is reproduced here
/// because it is observable and not by a byte: measured against GNU `cp
/// --backup=numbered` on a 255-byte-limit filesystem, a 251-byte name backs up
/// to a 252-byte one (`NAME` shortened by four, plus `~`) rather than to the
/// 255-byte `NAME.~1~` that a limit of 255 would allow. Whether the subtraction
/// was meant is not a question this module gets to answer; producing a
/// different name than GNU for the same directory is the one outcome that is
/// certainly wrong.
#[cfg(unix)]
fn name_max(dir: &Path) -> usize {
    /// `<unistd.h>`'s `_PC_NAME_MAX`. Measured as 3 on the host, and
    /// `posix::unistd::_PC_NAME_MAX` is 3 on the target.
    const PC_NAME_MAX: i32 = 3;

    unsafe extern "C" {
        fn pathconf(path: *const u8, name: i32) -> i64;
        fn __errno_location() -> *mut i32;
    }

    let Ok(path) = c_path(dir) else {
        return usize::MAX;
    };
    // SAFETY: `__errno_location` is defined to return a valid pointer to this
    // thread's `errno`, live for the whole life of the thread, and never fails.
    unsafe { *__errno_location() = 0 };
    // SAFETY: `path` is a NUL-terminated byte string that outlives the call, and
    // `pathconf` reads it without retaining it.
    let raw = unsafe { pathconf(path.as_ptr(), PC_NAME_MAX) };
    // SAFETY: as above.
    let errno = unsafe { *__errno_location() };

    // `name_max -= !errno`, written out. The subtraction applies to a
    // successful read as well as to the indeterminate one; see the doc comment.
    let limit = if errno == 0 {
        raw.saturating_sub(1)
    } else {
        raw
    };
    if limit >= 0 {
        usize::try_from(limit).unwrap_or(usize::MAX)
    } else if limit < -1 {
        NAME_MAX_MINIMUM
    } else {
        usize::MAX
    }
}

/// The same question on the one non-POSIX host this builds on, where there is
/// no `pathconf` and 255 is the answer for every filesystem worth testing on.
///
/// Less upstream's byte, so that a name shortened here is shortened to the same
/// length it would be on the target — the tests assert those lengths.
#[cfg(not(unix))]
fn name_max(_dir: &Path) -> usize {
    LONG_FILE_NAME_MAX.saturating_sub(1)
}

/// A path as a NUL-terminated byte string.
///
/// Fails for a name containing a NUL, which the filesystem forbids and which a
/// caller can nonetheless construct; letting it through would silently truncate
/// the name and act on a different file.
#[cfg(unix)]
fn c_path(path: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = os_bytes(path.as_os_str()).into_owned();
    if bytes.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains a NUL byte",
        ));
    }
    bytes.push(0);
    Ok(bytes)
}

/// Whether a rename may destroy an existing destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Replace {
    Yes,
    No,
}

/// gnulib's `renameatu` narrowed to the two cases this module needs.
///
/// [`Replace::No`] asks for `RENAME_NOREPLACE` and falls back to an `lstat`
/// when the kernel has no such flag — which ours does not yet, so this is
/// currently always the racy path on the target. The fallback is upstream's,
/// including its treatment of `EOVERFLOW` as "it is there": a destination whose
/// metadata does not fit in a `stat` is still a destination.
fn rename_maybe_noreplace(from: &Path, to: &Path, replace: Replace) -> io::Result<()> {
    if replace == Replace::Yes {
        return fs::rename(from, to);
    }
    #[cfg(unix)]
    {
        /// `AT_FDCWD`. Names relative to it are resolved from the working
        /// directory, which is where this module's callers' names already are.
        const AT_FDCWD: i32 = -100;
        /// `RENAME_NOREPLACE`.
        const NOREPLACE: u32 = 1;

        unsafe extern "C" {
            fn renameat2(
                olddirfd: i32,
                oldpath: *const u8,
                newdirfd: i32,
                newpath: *const u8,
                flags: u32,
            ) -> i32;
        }

        if let (Ok(old), Ok(new)) = (c_path(from), c_path(to)) {
            // SAFETY: both are NUL-terminated byte strings that outlive the
            // call, and `renameat2` does not retain either.
            let rc =
                unsafe { renameat2(AT_FDCWD, old.as_ptr(), AT_FDCWD, new.as_ptr(), NOREPLACE) };
            if rc == 0 {
                return Ok(());
            }
            let err = io::Error::last_os_error();
            // Upstream's three "the flag is not supported here" codes. Anything
            // else is the rename's real answer and is returned as it stands.
            const EINVAL: i32 = 22;
            const ENOSYS: i32 = 38;
            const ENOTSUP: i32 = 95;
            if !matches!(err.raw_os_error(), Some(EINVAL | ENOSYS | ENOTSUP)) {
                return Err(err);
            }
        }
    }
    // The emulation, with the race upstream also has: something can create `to`
    // between this test and the rename below.
    match fs::symlink_metadata(to) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "backup file already exists",
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => fs::rename(from, to),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scratchdir::ScratchDir;

    fn touch(dir: &Path, name: &str) {
        fs::write(dir.join(name), b"x").expect("fixture write");
    }

    fn names_in(dir: &Path) -> Vec<String> {
        let mut got: Vec<String> = fs::read_dir(dir)
            .expect("read fixture dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        got.sort();
        got
    }

    #[test]
    fn a_bare_option_is_existing_which_is_simple_when_nothing_is_numbered() {
        let scratch = ScratchDir::new("backup-bare");
        touch(scratch.dir(), "f");
        let backup = Backup::new(BackupType::NumberedExisting, b"~".to_vec());
        let made = backup.rename(&scratch.path("f")).expect("rename");
        assert_eq!(made, scratch.path("f~"));
        assert_eq!(names_in(scratch.dir()), vec!["f~".to_string()]);
    }

    #[test]
    fn existing_goes_numbered_once_a_numbered_backup_is_there() {
        let scratch = ScratchDir::new("backup-existing");
        touch(scratch.dir(), "f");
        touch(scratch.dir(), "f.~1~");
        let backup = Backup::new(BackupType::NumberedExisting, b"~".to_vec());
        let made = backup.rename(&scratch.path("f")).expect("rename");
        assert_eq!(made, scratch.path("f.~2~"));
    }

    #[test]
    fn numbered_takes_one_past_the_highest_not_one_past_the_count() {
        let scratch = ScratchDir::new("backup-highest");
        touch(scratch.dir(), "f");
        touch(scratch.dir(), "f.~1~");
        touch(scratch.dir(), "f.~7~");
        let backup = Backup::new(BackupType::Numbered, b"~".to_vec());
        assert_eq!(
            backup.find_name(&scratch.path("f")).expect("name"),
            scratch.path("f.~8~")
        );
    }

    #[test]
    fn the_version_is_compared_as_a_string_so_ten_beats_nine() {
        let scratch = ScratchDir::new("backup-ten");
        touch(scratch.dir(), "f");
        touch(scratch.dir(), "f.~9~");
        touch(scratch.dir(), "f.~10~");
        let backup = Backup::new(BackupType::Numbered, b"~".to_vec());
        assert_eq!(
            backup.find_name(&scratch.path("f")).expect("name"),
            scratch.path("f.~11~")
        );
    }

    #[test]
    fn all_nines_carries_into_a_longer_number() {
        let scratch = ScratchDir::new("backup-carry");
        touch(scratch.dir(), "f");
        touch(scratch.dir(), "f.~99~");
        let backup = Backup::new(BackupType::Numbered, b"~".to_vec());
        assert_eq!(
            backup.find_name(&scratch.path("f")).expect("name"),
            scratch.path("f.~100~")
        );
    }

    #[test]
    fn a_version_wider_than_any_integer_is_still_incremented() {
        let scratch = ScratchDir::new("backup-huge");
        touch(scratch.dir(), "f");
        touch(scratch.dir(), "f.~99999999999999999999999~");
        let backup = Backup::new(BackupType::Numbered, b"~".to_vec());
        assert_eq!(
            backup.find_name(&scratch.path("f")).expect("name"),
            scratch.path("f.~100000000000000000000000~")
        );
    }

    #[test]
    fn a_leading_zero_is_not_a_version() {
        let scratch = ScratchDir::new("backup-zero");
        touch(scratch.dir(), "f");
        touch(scratch.dir(), "f.~01~");
        touch(scratch.dir(), "f.~0~");
        let backup = Backup::new(BackupType::Numbered, b"~".to_vec());
        assert_eq!(
            backup.find_name(&scratch.path("f")).expect("name"),
            scratch.path("f.~1~")
        );
    }

    #[test]
    fn a_backup_of_another_name_is_not_counted() {
        let scratch = ScratchDir::new("backup-other");
        touch(scratch.dir(), "f");
        touch(scratch.dir(), "ff.~9~");
        touch(scratch.dir(), "g.~9~");
        let backup = Backup::new(BackupType::Numbered, b"~".to_vec());
        assert_eq!(
            backup.find_name(&scratch.path("f")).expect("name"),
            scratch.path("f.~1~")
        );
    }

    #[test]
    fn a_simple_backup_replaces_the_previous_one() {
        let scratch = ScratchDir::new("backup-replace");
        fs::write(scratch.path("f"), b"new").expect("write");
        fs::write(scratch.path("f~"), b"old").expect("write");
        let backup = Backup::new(BackupType::Simple, b"~".to_vec());
        backup.rename(&scratch.path("f")).expect("rename");
        assert_eq!(fs::read(scratch.path("f~")).expect("read"), b"new".to_vec());
    }

    #[test]
    fn a_missing_file_is_reported_as_not_found_rather_than_backed_up() {
        let scratch = ScratchDir::new("backup-absent");
        let backup = Backup::new(BackupType::Simple, b"~".to_vec());
        let err = backup
            .rename(&scratch.path("nope"))
            .expect_err("a missing file cannot be renamed");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn trailing_slashes_do_not_reach_the_backup_name() {
        let scratch = ScratchDir::new("backup-slash");
        fs::create_dir(scratch.path("d")).expect("mkdir");
        let backup = Backup::new(BackupType::Simple, b"~".to_vec());
        let mut with_slash = scratch.path("d").into_os_string();
        with_slash.push("/");
        let made = backup.rename(Path::new(&with_slash)).expect("rename");
        assert_eq!(made, scratch.path("d~"));
    }

    #[test]
    fn a_custom_suffix_is_used_verbatim() {
        let scratch = ScratchDir::new("backup-suffix");
        touch(scratch.dir(), "f");
        let backup = Backup::new(BackupType::Simple, b".bak".to_vec());
        let made = backup.rename(&scratch.path("f")).expect("rename");
        assert_eq!(made, scratch.path("f.bak"));
    }

    /// The lengths of `NAME` and of the backup GNU `cp --backup=numbered` makes
    /// of it, measured with coreutils 9.4 on a filesystem whose `_PC_NAME_MAX`
    /// is 255.
    ///
    /// The table exists rather than one example because it is what pins the
    /// limit at 254 rather than 255 — see [`NAME_MAX_MINIMUM`] and
    /// [`name_max`], which between them cost the byte. `250` is the longest
    /// name that keeps its `.~1~`; `251` is the first that loses it, and loses
    /// four bytes of itself in the bargain, which no reading of "the limit is
    /// 255" predicts. Note the two shapes the result takes: at or under the
    /// limit the name grows by four, and over it the name is cut back and
    /// given a bare `~`, so the answer *falls* from 254 to 252 and climbs
    /// again. A single example could sit on either side and prove neither.
    #[cfg(unix)]
    const MEASURED_LENGTHS: &[(usize, usize)] = &[
        (245, 249),
        (249, 253),
        (250, 254),
        (251, 252),
        (252, 253),
        (253, 254),
        (254, 254),
        (255, 254),
    ];

    #[cfg(unix)]
    #[test]
    fn an_over_long_name_is_shortened_where_gnu_shortens_it() {
        for &(original, want) in MEASURED_LENGTHS {
            let scratch = ScratchDir::new("backup-long");
            let long = "n".repeat(original);
            touch(scratch.dir(), &long);
            let backup = Backup::new(BackupType::Numbered, b"~".to_vec());
            let made = backup.rename(&scratch.path(&long)).expect("rename");
            let got = made
                .file_name()
                .expect("a last component")
                .to_string_lossy()
                .into_owned();
            assert_eq!(got.len(), want, "a {original}-byte name became {got}");
            assert!(got.ends_with('~'), "still marked as a backup: {got}");
            assert!(made.exists(), "and one the filesystem took");
        }
    }

    // The suffix and control-word rules read the environment, which is process
    // -wide, so they are checked against explicit arguments only; the
    // environment halves are exercised by `cp`'s own tests, which run in a
    // subprocess.

    #[test]
    fn an_empty_suffix_falls_back_to_tilde() {
        assert_eq!(suffix(Some(OsStr::new(""))), b"~".to_vec());
    }

    #[test]
    fn a_suffix_that_starts_a_component_falls_back_to_tilde() {
        assert_eq!(suffix(Some(OsStr::new("a/b"))), b"~".to_vec());
        assert_eq!(suffix(Some(OsStr::new("/b"))), b"~".to_vec());
    }

    #[test]
    fn a_suffix_that_merely_ends_in_a_slash_is_accepted() {
        // Upstream's test is `s == last_component (s)`, and a trailing slash
        // does not start a new component. Odd, and reproduced on purpose.
        assert_eq!(suffix(Some(OsStr::new("a/"))), b"a/".to_vec());
    }

    #[test]
    fn a_plain_suffix_is_taken_as_given() {
        assert_eq!(suffix(Some(OsStr::new(".orig"))), b".orig".to_vec());
    }

    #[test]
    fn the_control_words_pair_each_synonym_with_its_meaning() {
        const PROG: Program = Program::new("cp", 1);
        for (word, want) in CONTROL_WORDS {
            let got = control(PROG, Some(OsStr::new(word))).expect("a listed word");
            assert_eq!(got, *want, "{word}");
        }
    }

    #[test]
    fn a_control_word_may_be_abbreviated_when_it_is_not_ambiguous() {
        const PROG: Program = Program::new("cp", 1);
        assert_eq!(
            control(PROG, Some(OsStr::new("nu"))).expect("unambiguous"),
            BackupType::Numbered
        );
        assert!(
            control(PROG, Some(OsStr::new("n"))).is_err(),
            "none vs numbered"
        );
        // `ne` matches `never` only, and `ni` matches `nil` only.
        assert_eq!(
            control(PROG, Some(OsStr::new("ne"))).expect("unambiguous"),
            BackupType::Simple
        );
    }

    #[test]
    fn an_unknown_control_word_is_refused_with_status_one() {
        const PROG: Program = Program::new("cp", 1);
        let err = control(PROG, Some(OsStr::new("zz"))).expect_err("not a word");
        assert_eq!(err.status, 1);
        assert!(
            err.sentence.contains("invalid argument"),
            "{:?}",
            err.sentence
        );
        assert!(
            err.sentence.contains("backup type"),
            "the context is the phrase, not the option: {:?}",
            err.sentence
        );
    }
}
