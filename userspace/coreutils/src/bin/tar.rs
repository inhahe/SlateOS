//! tar — tape archive utility.
//!
//! Usage: tar -c [-f ARCHIVE] [-v] [FILE...]   create archive
//!        tar -x [-f ARCHIVE] [-v] [-C DIR]    extract archive
//!        tar -t [-f ARCHIVE]                   list archive
//!
//! Supports basic POSIX/ustar tar format (uncompressed).
//! Files > 8GB and paths > 255 chars are not supported.
//!
//! Create mode is unix-only (requires `mode`/`uid`/`gid`/`mtime` from
//! `MetadataExt`).  Listing and extraction are platform-independent at
//! the parsing level; the cross-platform helpers
//! (`parse_args`, `parse_octal`, `extract_string`, `TarHeader`,
//! `list_archive`, `sanitize_member_name`) are exercised by unit tests on
//! every host.
//!
//! # An archive is untrusted input
//!
//! The member names in a tar file are chosen by whoever made it, and an
//! extractor that believes them will write wherever it is told. This one used
//! to: `fs::write(&name, ...)` with `name` straight out of the header, so a
//! member called `../../etc/passwd` or `/etc/shadow` was written there, and
//! `-C` was no protection at all — an absolute name ignores the current
//! directory entirely. That is the "tar slip" class of vulnerability, and
//! `tar -xf` on a downloaded archive is exactly the situation it exists for.
//! `sanitize_member_name` now stands between the header and the filesystem:
//! every member is forced to be a relative path with no `..` in it, and one
//! that cannot be is refused rather than adjusted. See `known-issues.md` →
//! `B-tar-EXTRACTS-OUTSIDE-THE-DESTINATION-DIRECTORY`.
//!
//! The related rule is that a failed write is never silent. Creating an
//! archive that could not be written, or extracting a member that could not
//! be created, exits 2 (GNU's fatal-error status), not 0.

use coreutils::diag;
use coreutils::errmsg::strerror;
// `escape`, not `quotef`, and that is a deliberate departure from the house
// style of the other 85 bins. GNU tar calls `set_quoting_style (NULL,
// escape_quoting_style)` at startup, so *every* name it prints -- in a
// diagnostic, in `-t`, and in `-cv`/`-xv` -- comes out the same way: C escapes,
// octal for anything that is not a valid character, and no quotes at all.
// Measured: `tar: caf\351: Not found in archive`, where a `quotef`-shaped tar
// would have said `tar: 'caf'$'\351': Not found in archive`.
use coreutils::quote::{escape, escape_os, os_bytes, os_from_bytes, quoteaf};
use coreutils::stdfd;
#[cfg(unix)]
use std::collections::BTreeMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use std::process;

/// GNU tar's exit status for "a fatal error occurred". Used for every failure
/// that leaves the archive or the extracted tree incomplete, because a caller
/// that only sees 0 has no way to discover that half its files are missing.
const EXIT_FATAL: i32 = 2;

/// GNU tar's exit status for a *command line* that could not be parsed.
///
/// Not 2, and not 1: tar's argument parser is argp, and argp exits with
/// `EX_USAGE` (64) when an option is unknown or its argument is missing.
/// Measured: `tar -Q; echo $?` and `tar -cf; echo $?` both print 64, while
/// every runtime failure prints 2. The distinction is worth keeping because it
/// is the one a wrapper script can act on — 64 means "I typed it wrong", 2
/// means "the archive or the filesystem was the problem".
const EXIT_USAGE: i32 = 64;

/// The second line argp prints after any usage error, verbatim.
const TRY_HELP: &str = "Try 'tar --help' or 'tar --usage' for more information.";

/// Close out a run that had at least one non-fatal failure.
///
/// GNU prints this once, at exit, in addition to whatever was said about the
/// individual member — so a log that scrolled past the specific complaint still
/// ends with the fact that the run did not do what was asked. Returns the
/// status so call sites can `return failed_with_previous_errors()`.
fn failed_with_previous_errors() -> i32 {
    diag!("tar: Exiting with failure status due to previous errors");
    EXIT_FATAL
}

/// Close out a run that could not continue at all.
///
/// The distinction from [`failed_with_previous_errors`] is GNU's and is not
/// cosmetic: "previous errors" means the rest of the archive was processed and
/// some members were not, while this one means processing stopped where it was.
/// A reader of the log can tell from the last line alone whether the output is
/// partial or merely incomplete.
fn fatal() -> i32 {
    diag!("tar: Error is not recoverable: exiting now");
    EXIT_FATAL
}

// ============================================================================
// argv parsing — pure, cross-platform
// ============================================================================

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct TarArgs {
    create: bool,
    extract: bool,
    list: bool,
    verbose: bool,
    /// `-p`, `--same-permissions`: restore the stored mode exactly, umask and
    /// setuid bits included. Without it a non-root extraction applies
    /// `mode & 0o777 & !umask`, which is what GNU does and what this tar did
    /// not do at all — it left every extracted file at whatever `File::create`
    /// produced.
    same_permissions: bool,
    archive_file: Option<OsString>,
    directory: Option<OsString>,
    files: Vec<OsString>,
}

/// Parse tar's argv.  Supports clustered short flags; `f` and `C`
/// consume the following argv element as their value (even when
/// clustered as e.g. `-xvf`, in which case the next argv is the value
/// of `f`).  Unknown short flags return an error.
///
/// The error strings are argp's, verbatim, less the `tar: ` prefix the caller
/// adds: `invalid option -- 'Q'` and `option requires an argument -- 'f'`. They
/// used to read `option -f requires an argument`, which says the same thing in
/// a word order nothing else in the system uses — and a caller matching tar's
/// stderr is matching the real tar's, not ours.
///
/// The scan is over **bytes**, and the values and operands come out as
/// `OsString` unchanged. Every one of them is a path — the archive, the `-C`
/// destination, and each file to add — and on this OS a path may hold any byte
/// but `/` and NUL. Reading argv as `String` made `tar -cf a.tar <name>` abort
/// before doing anything at all when the name was not valid UTF-8, which is a
/// legal name here. See `known-issues.md` → `B-tar-READ-EVERY-PATH-AS-UTF-8`.
///
/// A cluster is walked byte by byte rather than `char` by `char`. That is not
/// merely the byte-safe spelling of the same loop: a multi-byte character in a
/// cluster used to be reported whole (`unknown option: -é`), and now reports
/// its first byte. Since no such cluster is ever valid, the difference is only
/// in the wording of a refusal — but the byte version cannot panic on a cluster
/// that is not UTF-8 at all, which the `char` version could not even reach.
fn parse_args(args: &[OsString]) -> Result<TarArgs, String> {
    let mut out = TarArgs::default();
    let mut i: usize = 0;

    while let Some(arg) = args.get(i) {
        let bytes = os_bytes(arg);
        // `--anything` is not an option here: this tar has no long options, and
        // treating `--` as the start of a cluster would read each of its letters
        // as a flag. It falls through to the operand branch, as it always has.
        if bytes.first() == Some(&b'-') && bytes.len() > 1 && bytes.get(1) != Some(&b'-') {
            let rest = bytes.get(1..).unwrap_or(&[]);
            for &c in rest {
                match c {
                    b'c' => out.create = true,
                    b'x' => out.extract = true,
                    b't' => out.list = true,
                    b'v' => out.verbose = true,
                    b'p' => out.same_permissions = true,
                    b'f' => {
                        i = i.saturating_add(1);
                        let v = args
                            .get(i)
                            .ok_or_else(|| "option requires an argument -- 'f'".to_string())?;
                        out.archive_file = Some(v.clone());
                    }
                    b'C' => {
                        i = i.saturating_add(1);
                        let v = args
                            .get(i)
                            .ok_or_else(|| "option requires an argument -- 'C'".to_string())?;
                        out.directory = Some(v.clone());
                    }
                    other => {
                        // `quoteaf` rather than `char::from`: `other` is an
                        // arbitrary byte from the command line, and rendering it
                        // raw would let a crafted argument forge a line of
                        // tar's stderr.
                        //
                        // The wording is GNU's, measured: `tar -Q` says
                        // `tar: invalid option -- 'Q'`. It used to read
                        // `unknown option: -Q`, and since `quoteaf` always
                        // quotes, keeping that shape would have produced the
                        // odd `-'Q'` — so the message moved to the one it
                        // should have had anyway.
                        return Err(format!("invalid option -- {}", quoteaf(&[other])));
                    }
                }
            }
        } else {
            out.files.push(arg.clone());
        }
        i = i.saturating_add(1);
    }

    Ok(out)
}

fn main() {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let parsed = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            diag!("tar: {e}");
            diag!("{TRY_HELP}");
            process::exit(EXIT_USAGE);
        }
    };

    // Every mode returns its own status rather than exiting inline, so that
    // "some members failed" survives to the caller. A tool that reports 0
    // after writing half an archive is worse than one that fails outright:
    // the script that invoked it deletes the source and moves on.
    let status = if parsed.create {
        // The one case where the member list is a diagnostic rather than
        // output: with no `-f`, the archive itself is on stdout, and a name
        // printed there would be a block of the archive.
        let verbose = match (parsed.verbose, parsed.archive_file.is_some()) {
            (false, _) => Verbose::Off,
            (true, true) => Verbose::Stdout,
            (true, false) => Verbose::Stderr,
        };
        #[cfg(unix)]
        {
            do_create(parsed.archive_file.as_deref(), &parsed.files, verbose)
        }
        #[cfg(not(unix))]
        {
            let _ = verbose;
            diag!("tar: create mode is unix-only on this build");
            EXIT_FATAL
        }
    } else if parsed.extract {
        do_extract(
            parsed.archive_file.as_deref(),
            parsed.directory.as_deref(),
            if parsed.verbose {
                Verbose::Stdout
            } else {
                Verbose::Off
            },
            &parsed.files,
            parsed.same_permissions,
        )
    } else if parsed.list {
        do_list_main(
            parsed.archive_file.as_deref(),
            parsed.verbose,
            &parsed.files,
        )
    } else {
        // GNU's own sentence, listing options this tar does not have. That is
        // deliberate: the message tells the reader what the *format* accepts,
        // and a user who reaches for `-r` after reading it gets a specific
        // `invalid option` rather than being told twice that they typed
        // nothing. The status is 2, not argp's 64 — this is not a malformed
        // command line, it is a well-formed one that asked for no operation.
        diag!(
            "tar: You must specify one of the '-Acdtrux', '--delete' or '--test-label' options"
        );
        diag!("{TRY_HELP}");
        EXIT_FATAL
    };

    process::exit(status);
}

// ============================================================================
// TAR header format (512 bytes, POSIX ustar) — cross-platform
// ============================================================================

const BLOCK_SIZE: usize = 512;

#[repr(C)]
#[cfg_attr(not(unix), allow(dead_code))]
struct TarHeader {
    name: [u8; 100],
    mode: [u8; 8],
    uid: [u8; 8],
    gid: [u8; 8],
    size: [u8; 12],
    mtime: [u8; 12],
    checksum: [u8; 8],
    typeflag: u8,
    linkname: [u8; 100],
    magic: [u8; 6],
    version: [u8; 2],
    uname: [u8; 32],
    gname: [u8; 32],
    devmajor: [u8; 8],
    devminor: [u8; 8],
    prefix: [u8; 155],
    _pad: [u8; 12],
}

/// The `name` field's width. A name of exactly this length fills it with no
/// room for a terminator, which is legal: ustar NUL-terminates only when the
/// name is short enough to leave a byte spare.
const NAME_FIELD: usize = 100;

/// The `prefix` field's width.
const PREFIX_FIELD: usize = 155;

/// Why a member name could not be stored in a ustar header.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum NameTooLong {
    /// Longer than the two fields and the `/` between them can hold at all.
    Max,
    /// Short enough in total, but with no `/` in a position that would put no
    /// more than [`NAME_FIELD`] bytes after it.
    CannotSplit,
}

impl NameTooLong {
    /// GNU's wording, which names the limit in the first case and not in the
    /// second. Both end `; not dumped`, and both leave the archive otherwise
    /// intact — the member is skipped, the rest are written, and the exit
    /// status is 2.
    fn message(self, name: &[u8]) -> String {
        match self {
            Self::Max => format!(
                "tar: {}: file name is too long (max {}); not dumped",
                escape(name),
                NAME_FIELD.saturating_add(PREFIX_FIELD).saturating_add(1)
            ),
            Self::CannotSplit => format!(
                "tar: {}: file name is too long (cannot be split); not dumped",
                escape(name)
            ),
        }
    }
}

/// Split `full` into ustar's `prefix` and `name`, as GNU's `split_long_name`
/// does.
///
/// The whole name is `prefix` + `/` + `name` when a prefix is used, so the
/// split has to fall on a `/` and that `/` is not stored. Names that fit in
/// [`NAME_FIELD`] outright use no prefix at all.
///
/// The search is *backwards from a capped position*, not "the last slash", and
/// the cap is what makes some names unsplittable that look splittable:
///
/// 1. Consider only the first `PREFIX_FIELD + 1` bytes, since a prefix cannot
///    reach past that anyway.
/// 2. Unless the cap already applied, ignore a trailing `/` — a directory
///    member is stored with one and it must not be chosen as the split point.
/// 3. Take the last `/` at or before that position; offset 0 does not count,
///    because an empty prefix is not a prefix.
/// 4. Refuse if what follows it is empty or longer than [`NAME_FIELD`].
///
/// Measured against GNU tar 1.35 across the boundary (`tar-longname.sh`):
/// `t/` + 96×`d` + `/fff` splits at offset 98; a 100-byte remainder is
/// accepted and a 101-byte one is refused; and `t/` + 150×`d` + `/` — 153
/// bytes, which would fit a 152-byte prefix and a 0-byte name — is refused,
/// because rule 3 finds the slash at offset 1 and leaves 151 bytes after it.
fn split_ustar_name(full: &[u8]) -> Result<(&[u8], &[u8]), NameTooLong> {
    if full.len() <= NAME_FIELD {
        return Ok((&[], full));
    }
    if full.len() > NAME_FIELD.saturating_add(PREFIX_FIELD).saturating_add(1) {
        return Err(NameTooLong::Max);
    }
    let capped = PREFIX_FIELD.saturating_add(1);
    let mut end = full.len();
    if end > capped {
        end = capped;
    } else if full.get(end.saturating_sub(1)) == Some(&b'/') {
        end = end.saturating_sub(1);
    }
    // Backwards over `full[1..end]`: offset 0 is excluded because a prefix of
    // no bytes is the no-prefix case, which the length test above already took.
    let split = full
        .get(1..end)
        .unwrap_or(&[])
        .iter()
        .rposition(|&b| b == b'/')
        .map(|i| i.saturating_add(1));
    let Some(i) = split else {
        return Err(NameTooLong::CannotSplit);
    };
    let (Some(prefix), Some(name)) = (full.get(..i), full.get(i.saturating_add(1)..)) else {
        return Err(NameTooLong::CannotSplit);
    };
    if name.is_empty() || name.len() > NAME_FIELD {
        return Err(NameTooLong::CannotSplit);
    }
    Ok((prefix, name))
}

#[cfg_attr(not(unix), allow(dead_code))]
impl TarHeader {
    fn new() -> Self {
        Self {
            name: [0; 100],
            mode: [0; 8],
            uid: [0; 8],
            gid: [0; 8],
            size: [0; 12],
            mtime: [0; 12],
            checksum: [0; 8],
            typeflag: 0,
            linkname: [0; 100],
            magic: [0; 6],
            version: [0; 2],
            uname: [0; 32],
            gname: [0; 32],
            devmajor: [0; 8],
            devminor: [0; 8],
            prefix: [0; 155],
            _pad: [0; 12],
        }
    }

    /// Store a member name across the header's `name` and `prefix` fields.
    ///
    /// Bytes, not `&str`: the fields hold whatever the filesystem gave us, and
    /// ustar has never required it to be text.
    ///
    /// This used to copy the first 99 bytes into `name` and stop. Two separate
    /// defects in one line: a 100-byte name lost its last byte, because the
    /// field is not NUL-terminated when it is full; and a name longer than that
    /// was silently truncated, producing a well-formed archive holding the
    /// wrong name and exiting 0. See [`split_ustar_name`] for the split and for
    /// what happens when there is none.
    fn set_name(&mut self, full: &[u8]) -> Result<(), NameTooLong> {
        let (prefix, name) = split_ustar_name(full)?;
        if let (Some(dst), Some(src)) = (self.name.get_mut(..name.len()), name.get(..)) {
            dst.copy_from_slice(src);
        }
        if let (Some(dst), Some(src)) = (self.prefix.get_mut(..prefix.len()), prefix.get(..)) {
            dst.copy_from_slice(src);
        }
        Ok(())
    }

    /// Store a link target in the 100-byte `linkname` field, cutting it to fit.
    ///
    /// Returns whether the whole target fit; the caller warns when it did not.
    ///
    /// There is no `prefix` for this one — ustar gives the link target a single
    /// field and no escape hatch — and unlike a member name, which GNU refuses
    /// outright, a target that does not fit is stored truncated. Measured: a
    /// 101-byte symlink target produces the warning, exit status 2, *and* a
    /// member in the archive whose link is the first 100 bytes.
    fn set_linkname(&mut self, target: &[u8]) -> bool {
        let kept = target.len().min(NAME_FIELD);
        if let (Some(dst), Some(src)) = (self.linkname.get_mut(..kept), target.get(..kept)) {
            dst.copy_from_slice(src);
        }
        target.len() <= NAME_FIELD
    }

    /// Write `value` as a zero-padded octal string into `field`.  The
    /// field always ends with a trailing null byte, matching ustar.
    fn set_octal(field: &mut [u8], value: u64) {
        if field.is_empty() {
            return;
        }
        let width = field.len().saturating_sub(1);
        let s = format!("{value:0>width$o}");
        let bytes = s.as_bytes();
        // If `s` is longer than the field allows, take only the rightmost
        // `width` chars so the low-order digits survive.
        let start = bytes.len().saturating_sub(width);
        let src = bytes.get(start..).unwrap_or(&[]);
        let copy_len = src.len().min(width);
        if let (Some(dst), Some(src)) = (field.get_mut(..copy_len), src.get(..copy_len)) {
            dst.copy_from_slice(src);
        }
        // Trailing byte stays NUL.
    }

    fn compute_checksum(&mut self) {
        // Fill checksum field with spaces for computation.
        self.checksum = [b' '; 8];

        // SAFETY: `TarHeader` is `#[repr(C)]` with explicit byte-array
        // fields whose sizes add to exactly `BLOCK_SIZE` (512).  There
        // are no padding bytes or non-trivial drop glue, so it is sound
        // to view `self` as `[u8; BLOCK_SIZE]`.  The borrow lasts only
        // for the duration of this function.
        let header_bytes =
            unsafe { std::slice::from_raw_parts((self as *const Self).cast::<u8>(), BLOCK_SIZE) };
        let sum: u32 = header_bytes.iter().map(|&b| u32::from(b)).sum();

        let s = format!("{sum:06o}\0 ");
        let bytes = s.as_bytes();
        let copy_len = bytes.len().min(8);
        if let (Some(dst), Some(src)) = (self.checksum.get_mut(..copy_len), bytes.get(..copy_len)) {
            dst.copy_from_slice(src);
        }
    }

    fn as_bytes(&self) -> &[u8; BLOCK_SIZE] {
        // SAFETY: see `compute_checksum` — `#[repr(C)]` byte fields
        // tiling to exactly `BLOCK_SIZE` make this cast sound.
        unsafe { &*(self as *const Self).cast::<[u8; BLOCK_SIZE]>() }
    }
}

/// Where `-v` writes its running list of member names.
///
/// This used to be "stderr, always", which is wrong in the ordinary case and
/// right in exactly one unusual one. GNU writes the list to **stdout**, because
/// it is output, not a diagnostic: `tar -cvf a.tar d > manifest` is how you get
/// a manifest, and ours produced an empty `manifest` and printed the names past
/// the redirection onto the terminal. The single exception is an archive being
/// written *to* stdout — `tar -cvf - d` — where the names would be interleaved
/// with the archive bytes and ruin both; there, and only there, they go to
/// stderr.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Verbose {
    /// No `-v`: say nothing.
    Off,
    /// The usual case.
    Stdout,
    /// `-cv` with the archive itself on stdout.
    Stderr,
}

impl Verbose {
    /// Announce one member name, rendered exactly as a diagnostic would render
    /// it.
    ///
    /// This used to write the name's bytes raw, on the reasoning that a listing
    /// is output rather than a message and should carry the name intact. That
    /// is not what GNU does and it is not safe: `-cv`, `-xv` and `-t` all go
    /// through the same `escape` style as tar's diagnostics, so a member called
    /// `a\nb` prints as `a\nb` on one line rather than as two lines that a
    /// script reading the manifest would take for two files. Measured against
    /// GNU tar 1.35 for all of `-t`, `-tv`, `-cv` and `-xv`.
    ///
    /// The cost is that the rendering is no longer reversible — a name holding
    /// a literal backslash comes back doubled — which is exactly the cost GNU
    /// pays, and the reason `tar -t` has never been a safe way to feed names to
    /// another program.
    fn line(self, name: &[u8]) {
        let shown = escape(name);
        let mut line = Vec::with_capacity(shown.len().saturating_add(1));
        line.extend_from_slice(shown.as_bytes());
        line.push(b'\n');
        match self {
            Self::Off => {}
            // Unbuffered, by fd. Nothing else in `-c`/`-x` writes to stdout, so
            // there is no ordering to keep with a `BufWriter`, and a failure to
            // write the listing must not abort the archive.
            Self::Stdout => drop(stdfd::write_all(1, &line)),
            Self::Stderr => stdfd::diag_bytes(&line),
        }
    }
}

/// Reduce an archive member name to a path that cannot escape the current
/// directory, or refuse it.
///
/// Two things a hostile (or merely careless) archive can do are handled
/// differently on purpose:
///
/// * **A leading `/`** is *stripped*, with the same reasoning as GNU tar's
///   "Removing leading `/' from member names": archives of system trees are
///   routinely made with absolute paths and are perfectly safe to unpack
///   relative to somewhere else, so refusing them would break a common case
///   for no gain. Note this is not cosmetic — `Path::join` with an absolute
///   path *discards* the base, so an unstripped `/etc/passwd` would ignore
///   `-C` entirely.
/// * **A `..` component** is *refused*, and the member is skipped. It cannot
///   be stripped safely: `a/../b` looks equivalent to `b` only if `a` is a
///   real directory and not a symlink, and the archive is precisely the thing
///   we are not willing to trust about that. Refusing costs a rare, loud
///   failure; guessing costs an arbitrary file write.
///
/// `.` components and repeated slashes are dropped, since they name the same
/// path and only serve to disguise the two cases above.
///
/// The `..` test also splits on `\`, which is *not* a separator in this OS
/// (`design.txt`: paths allow every byte but `/` and NUL). That is defence in
/// depth for the host builds this file is unit-tested on, where `..\..\x` does
/// traverse. The rebuilt name still joins with `/` only, so a slateos file
/// legitimately containing a backslash keeps its name; the sole casualty is a
/// file with a literal `..\` component, which is refused rather than silently
/// renamed.
///
/// Operates on **bytes**, because the name it is given is 100 bytes out of an
/// archive header and nothing guarantees they are text. The previous version
/// took `&str`, which meant the name had already been through
/// `String::from_utf8_lossy` before this function saw it — and that is not just
/// a display problem here, it is a correctness one in both directions:
///
/// * A member legitimately named with a non-UTF-8 byte was **extracted under a
///   different name**, with each bad byte replaced by U+FFFD. Silent data
///   corruption of exactly the kind rule 7 of `CLAUDE.md` names.
/// * Worse, the replacement happened *before* the `..` test, so the guarantee
///   this function exists to provide was being made about a string that was no
///   longer the name being written. Comparing bytes to bytes removes a whole
///   class of question about whether the check and the write agree.
///
/// The error messages render the raw name with `escape` — tar's one quoting
/// style — since it is attacker-chosen and printing it raw would let a crafted
/// archive forge a line of tar's stderr.
fn sanitize_member_name(raw: &[u8]) -> Result<Vec<u8>, String> {
    let mut parts: Vec<&[u8]> = Vec::new();
    for component in raw.split(|&b| b == b'/') {
        if component.is_empty() || component == b"." {
            // A leading empty component is the absolute-path `/`; an interior
            // one is a doubled slash. Both are dropped.
            continue;
        }
        if component == b".." || component.split(|&b| b == b'\\').any(|p| p == b"..") {
            // GNU's sentence, and GNU's quoting of the `..` inside it. Ours
            // said "refusing to extract X: member name escapes the destination
            // directory", which is the same refusal described in words no other
            // tar uses; the name is what the reader needs, and it is already
            // there. The member is skipped either way.
            return Err(format!(
                "{}: Member name contains '..'",
                escape(raw)
            ));
        }
        parts.push(component);
    }
    if parts.is_empty() {
        // A name that is only slashes and dots (`/`, `./.`) survives every rule
        // above and then names nothing. This wording is ours, not measured from
        // GNU, because GNU's own answer here depends on which of several
        // stripping passes runs out of name first; what matters is that such a
        // member is refused rather than resolved to `.`, which is the directory
        // being extracted into.
        return Err(format!("{}: Cannot extract: empty member name", escape(raw)));
    }
    Ok(parts.join(&b'/'))
}

/// Split a device number the way ustar stores it, into `devmajor`/`devminor`.
///
/// Not a plain shift: Linux packs `dev_t` in two pieces so that old 16-bit
/// numbers keep their old encoding — 12 bits of major and 8 of minor in the
/// low half, the rest of each in the high half.
#[cfg(unix)]
fn split_dev(rdev: u64) -> (u64, u64) {
    let major = ((rdev >> 8) & 0xfff) | ((rdev >> 32) & !0xfff);
    let minor = (rdev & 0xff) | ((rdev >> 12) & !0xff);
    (major, minor)
}

/// Split off the leading components tar refuses to *store*, returning
/// `(removed, rest)`.
///
/// An archive holding `/etc/passwd` is a loaded gun: extracting it anywhere
/// writes to `/etc/passwd`. GNU's answer is at both ends — it strips the prefix
/// when writing the archive as well as when reading one — and this is the
/// writing end.
///
/// What counts as a prefix, measured against GNU tar 1.35 (`tar-lead.sh`):
///
/// | given | stored | said |
/// |---|---|---|
/// | `/a/b` | `a/b` | ``Removing leading `/' from member names`` |
/// | `//a/b` | `a/b` | ``Removing leading `//' ...`` — the exact run, not one `/` |
/// | `../a` | `a` | ``Removing leading `../' ...`` |
/// | `..` | `.` | ``Removing leading `..' ...`` |
/// | `./a` | `./a` | nothing — a leading `.` is not a prefix |
///
/// So: any run of `/`, and any `..` that is a whole component, in any order and
/// repeated. A leading `.` is deliberately not in that set; it names the
/// directory being archived and takes the extractor nowhere it was not already.
fn strip_leading(name: &[u8]) -> (&[u8], &[u8]) {
    let mut i = 0usize;
    loop {
        match name.get(i) {
            Some(&b'/') => i = i.saturating_add(1),
            Some(&b'.')
                if name.get(i.saturating_add(1)) == Some(&b'.')
                    && matches!(name.get(i.saturating_add(2)), None | Some(&b'/')) =>
            {
                i = i.saturating_add(2);
            }
            _ => break,
        }
    }
    (name.get(..i).unwrap_or(&[]), name.get(i..).unwrap_or(&[]))
}

/// The state one `-c` pass carries from member to member.
///
/// This was four free functions threading a `&mut i32`. The hard-link table is
/// why it is a struct: recognising a second name for an inode means
/// remembering every inode already written for the whole run and across
/// operands — measured, GNU stores `t/h` as a link to `t/a.txt` even when the
/// two are separate command-line arguments, and stores the *first* name it
/// happened to archive, so `tar -c t/h t/a.txt` links `a.txt` to `h`.
#[cfg(unix)]
struct Creator<'a> {
    out: &'a mut dyn Write,
    verbose: Verbose,
    /// 0, or [`EXIT_FATAL`] once anything has gone wrong. A member that cannot
    /// be archived sets this and is skipped; it does not abandon the archive.
    status: i32,
    /// Every inode already archived that could have another name, and the name
    /// it went in under. Keyed by `(dev, ino)`, because an inode number is only
    /// unique within one filesystem — a bare `ino` key would link together two
    /// unrelated files that happen to share a number across mount points.
    links: BTreeMap<(u64, u64), Vec<u8>>,
    /// The last prefix [`strip_leading`] removed from a member name, and the
    /// last one it removed from a hard link's target. GNU warns when the prefix
    /// *changes*, not once per archive and not once per member — which is why
    /// `tar -c ..` produces two lines, ``Removing leading `..'`` for the
    /// directory itself and ``Removing leading `../'`` for everything in it.
    last_name_prefix: Vec<u8>,
    last_link_prefix: Vec<u8>,
    /// `(dev, ino)` of the archive being written, when it is a file we can
    /// identify. `tar -cf backup.tar .` names the archive among the things to
    /// archive, and a tar that obliges copies the archive into itself as it
    /// grows — the result is a much larger file holding a truncated snapshot of
    /// itself, and no warning that it happened.
    archive_id: Option<(u64, u64)>,
    /// Cleared by the first failed write. There is no point continuing after
    /// one: every later member would land at the wrong offset, producing a file
    /// that looks like an archive and is not one.
    writable: bool,
}

#[cfg(unix)]
impl Creator<'_> {
    fn fail(&mut self) {
        self.status = EXIT_FATAL;
    }

    fn write(&mut self, buf: &[u8]) -> bool {
        if !self.writable {
            return false;
        }
        match self.out.write_all(buf) {
            Ok(()) => true,
            Err(e) => {
                diag!("tar: Cannot write: {}", strerror(&e));
                self.writable = false;
                self.fail();
                false
            }
        }
    }

    /// The name this member goes into the archive under: `name` with any
    /// leading `/` or `../` taken off, and with the trailing slash a directory
    /// member carries put on.
    ///
    /// The two happen in that order, which matters for `tar -c ..`: strip
    /// first and `..` becomes nothing, which is stored as `.` and listed as
    /// `./`. Appending first would have stripped the slash back off again.
    fn stored_name(&mut self, name: &[u8], dir: bool) -> Vec<u8> {
        let (removed, rest) = strip_leading(name);
        if !removed.is_empty() && self.last_name_prefix != removed {
            diag!("tar: Removing leading `{}' from member names", escape(removed));
            self.last_name_prefix = removed.to_vec();
        }
        let mut stored = if rest.is_empty() {
            b".".to_vec()
        } else {
            rest.to_vec()
        };
        if dir {
            stored.push(b'/');
        }
        stored
    }

    /// The same for a hard link's target, which is a member name too and gets
    /// the same treatment under a message of its own.
    ///
    /// Not for a *symlink* target: that one is data, not a member name, and an
    /// absolute symlink is a legitimate thing to archive. Measured — GNU stores
    /// `/etc/passwd` for `ln -s /etc/passwd x` and says nothing.
    fn stored_link_target(&mut self, target: &[u8]) -> Vec<u8> {
        let (removed, rest) = strip_leading(target);
        if !removed.is_empty() && self.last_link_prefix != removed {
            diag!(
                "tar: Removing leading `{}' from hard link targets",
                escape(removed)
            );
            self.last_link_prefix = removed.to_vec();
        }
        if rest.is_empty() {
            b".".to_vec()
        } else {
            rest.to_vec()
        }
    }

    /// Fill in the fields every member type shares. `None` means the name
    /// cannot be stored — reported here, and the member is skipped.
    ///
    /// `size` is left at zero: only a regular file overrides it, and getting
    /// that wrong on a link or a device would make the extractor read the next
    /// member's header as file contents.
    fn header(&mut self, name: &[u8], meta: &fs::Metadata, dir: bool) -> Option<TarHeader> {
        use std::os::unix::fs::MetadataExt;
        let name = &self.stored_name(name, dir);
        let mut header = TarHeader::new();
        if let Err(e) = header.set_name(name) {
            // Skipped, not fatal to the archive: GNU writes every other member
            // and exits 2, so one unstorable name does not cost you the backup.
            // Measured — an archive of a tree holding such a file still lists
            // the rest.
            diag!("{}", e.message(name));
            self.fail();
            return None;
        }
        TarHeader::set_octal(&mut header.mode, u64::from(meta.mode()) & 0o7777);
        TarHeader::set_octal(&mut header.uid, u64::from(meta.uid()));
        TarHeader::set_octal(&mut header.gid, u64::from(meta.gid()));
        TarHeader::set_octal(&mut header.size, 0);
        TarHeader::set_octal(&mut header.mtime, meta.mtime().unsigned_abs());
        header.magic = *b"ustar\0";
        header.version = *b"00";
        Some(header)
    }

    /// Archive whatever `path` turns out to be, under the member name `name`.
    ///
    /// The type test is `symlink_metadata`, not `metadata`. The previous code
    /// asked `path.is_dir()` and then `fs::metadata`, both of which follow
    /// symlinks, so a symlink was archived as a *copy of whatever it pointed
    /// at* — a symlink to a directory pulled that whole directory into the
    /// archive under the link's name, and a symlink to a file duplicated the
    /// file. Restoring such an archive does not restore the tree.
    fn add(&mut self, path: &Path, name: &[u8]) {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        let meta = match fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) => {
                diag!("tar: {}: Cannot stat: {}", escape(name), strerror(&e));
                self.fail();
                return;
            }
        };
        if self.archive_id == Some((meta.dev(), meta.ino())) {
            // A warning, not an error: GNU exits 0 for this, because the
            // archive it produced is exactly the one that was asked for minus
            // the one member that could not possibly have gone in it.
            diag!(
                "tar: {}: archive cannot contain itself; not dumped",
                escape(name)
            );
            return;
        }
        let ft = meta.file_type();
        if ft.is_dir() {
            self.add_dir(path, name, &meta);
            return;
        }
        // A second name for an inode already archived is stored as a link to
        // the first, whatever the two names are. Checked before the type
        // dispatch because it applies to fifos and devices too, not just
        // regular files, and checked only when the inode admits another name:
        // a link count of one cannot have a second name to find.
        if meta.nlink() > 1 {
            let key = (meta.dev(), meta.ino());
            if let Some(first) = self.links.get(&key) {
                let first = first.clone();
                self.add_link(name, &meta, b'1', &first);
                return;
            }
            self.links.insert(key, name.to_vec());
        }
        if ft.is_symlink() {
            let target = match fs::read_link(path) {
                Ok(t) => os_bytes(t.as_os_str()).into_owned(),
                Err(e) => {
                    diag!("tar: {}: Cannot read link: {}", escape(name), strerror(&e));
                    self.fail();
                    return;
                }
            };
            self.add_link(name, &meta, b'2', &target);
        } else if ft.is_file() {
            self.add_regular(path, name, &meta);
        } else if ft.is_fifo() {
            self.add_special(name, &meta, b'6');
        } else if ft.is_char_device() {
            self.add_special(name, &meta, b'3');
        } else if ft.is_block_device() {
            self.add_special(name, &meta, b'4');
        } else if ft.is_socket() {
            // Not an error, and measured as such: a socket is a kernel object
            // with no contents an archive could hold, so GNU says so and still
            // exits 0. Skipping it silently would be the wrong half of that —
            // the file is missing from the archive and the user should know.
            diag!("tar: {}: socket ignored", escape(name));
        } else {
            diag!("tar: {}: Unknown file type; file ignored", escape(name));
            self.fail();
        }
    }

    /// A member that is a name pointing at another name and nothing else: a
    /// symlink (`2`) or a hard link (`1`). Both store zero bytes of data.
    fn add_link(&mut self, name: &[u8], meta: &fs::Metadata, typeflag: u8, target: &[u8]) {
        // The order is GNU's: the member name's prefix is reported before the
        // link target's, because `header` runs first.
        let Some(mut header) = self.header(name, meta, false) else {
            return;
        };
        let target = &if typeflag == b'1' {
            self.stored_link_target(target)
        } else {
            target.to_vec()
        };
        header.typeflag = typeflag;
        if !header.set_linkname(target) {
            // GNU says "not dumped" and then dumps it anyway, with the target
            // cut to 100 bytes — measured, the member is in the archive with a
            // truncated link. We match that rather than skipping, because the
            // alternative loses the member entirely: a truncated target almost
            // certainly does not exist, so extraction fails loudly, whereas a
            // skipped member is simply absent. Note the message names the
            // *target*, not the member — that is GNU's wording, not a slip.
            diag!("tar: {}: link name is too long; not dumped", escape(target));
            self.fail();
        }
        header.compute_checksum();
        if self.write(header.as_bytes()) {
            self.verbose.line(name);
        }
    }

    /// A fifo (`6`) or a device (`3`/`4`): a header, no data.
    fn add_special(&mut self, name: &[u8], meta: &fs::Metadata, typeflag: u8) {
        use std::os::unix::fs::MetadataExt;
        let Some(mut header) = self.header(name, meta, false) else {
            return;
        };
        header.typeflag = typeflag;
        if typeflag == b'3' || typeflag == b'4' {
            let (major, minor) = split_dev(meta.rdev());
            TarHeader::set_octal(&mut header.devmajor, major);
            TarHeader::set_octal(&mut header.devminor, minor);
        }
        header.compute_checksum();
        if self.write(header.as_bytes()) {
            self.verbose.line(name);
        }
    }

    /// A regular file: a header, then its contents padded out to a block.
    fn add_regular(&mut self, path: &Path, name: &[u8], meta: &fs::Metadata) {
        // The header commits to a length, so the body must be exactly that
        // many bytes however the read goes. Writing fewer would not merely
        // truncate this member: the extractor reads a fixed number of blocks
        // per header, so every subsequent member would be read from the wrong
        // offset and the whole archive after this point would be garbage.
        let declared = meta.len();

        let mut f = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                diag!("tar: {}: Cannot open: {}", escape(name), strerror(&e));
                self.fail();
                return;
            }
        };

        let Some(mut header) = self.header(name, meta, false) else {
            return;
        };
        TarHeader::set_octal(&mut header.size, declared);
        header.typeflag = b'0';
        header.compute_checksum();
        if !self.write(header.as_bytes()) {
            return;
        }

        self.verbose.line(name);

        let mut remaining = declared;
        let mut buf = [0u8; BLOCK_SIZE];
        let mut short = false;
        while remaining > 0 {
            let want = usize::try_from(remaining)
                .unwrap_or(BLOCK_SIZE)
                .min(BLOCK_SIZE);
            let mut filled = 0usize;
            while filled < want && !short {
                match f.read(buf.get_mut(filled..want).unwrap_or(&mut [])) {
                    // Only 0 means end of file. A short read is ordinary — the
                    // previous code took any single `read` as the whole block
                    // and NUL-padded the rest, so a file delivered in pieces
                    // was archived with holes punched through it.
                    Ok(0) => short = true,
                    Ok(n) => filled = filled.saturating_add(n),
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => {
                        diag!("tar: {}: Cannot read: {}", escape(name), strerror(&e));
                        self.fail();
                        short = true;
                    }
                }
            }
            if let Some(pad) = buf.get_mut(filled..) {
                pad.fill(0);
            }
            if !self.write(&buf) {
                return;
            }
            remaining = remaining.saturating_sub(want as u64);
        }
        if short {
            // The file shrank between the stat and the read, or never had the
            // length it claimed. The archive stays well-formed because the
            // remaining blocks were padded, but it no longer holds the file.
            diag!(
                "tar: {}: file shorter than expected; padded with zeros",
                escape(name)
            );
            self.fail();
        }
    }

    /// A directory (`5`) and, after it, everything under it in name order.
    ///
    /// `name` is the directory's member name *without* the trailing slash the
    /// header carries; children are named by appending to it.
    fn add_dir(&mut self, dir: &Path, name: &[u8], meta: &fs::Metadata) {
        // A directory has an owner, permissions and an mtime like anything
        // else, and all four used to be hard-coded here: every directory in
        // every archive we wrote came out `drwxr-xr-x 0/0` stamped 1970. Not a
        // cosmetic difference — restoring such an archive as root would hand
        // every directory in it to root and open a 0700 directory to the world.
        let Some(mut header) = self.header(name, meta, true) else {
            // The directory is skipped and so, necessarily, is everything under
            // it: a member name that cannot be stored has no children whose
            // names could be.
            return;
        };
        header.typeflag = b'5';
        header.compute_checksum();
        if !self.write(header.as_bytes()) {
            return;
        }

        // The *unstripped* name, with the trailing slash. GNU's `-cv` names the
        // file it is reading, not the member it is writing: `tar -cvf a.tar
        // /etc` lists `/etc/...` while the archive holds `etc/...`. Measured.
        let mut shown = name.to_vec();
        shown.push(b'/');
        self.verbose.line(&shown);

        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) => {
                // Previously `if let Ok(entries)`, so an unreadable directory
                // produced an archive silently missing its whole subtree.
                diag!("tar: {}: Cannot open: {}", escape(name), strerror(&e));
                self.fail();
                return;
            }
        };
        // Sorted so that archiving the same tree twice produces the same
        // bytes; `read_dir` order is whatever the filesystem feels like.
        //
        // The names are collected as bytes. `to_string_lossy().into_owned()`
        // was here, and it did not merely misprint a name under `-v`: the
        // lossy copy was what got stored in the header *and* what the recursion
        // descended with, so a directory entry whose name is not UTF-8 — legal
        // on this OS — was archived under a different name than it has on disk,
        // and restoring the archive would not restore the tree. Sorting by
        // bytes rather than by `String` also keeps the ordering stable for
        // names that no longer survive a lossy round trip.
        let mut children: Vec<(Vec<u8>, std::path::PathBuf)> = Vec::new();
        for entry in entries {
            match entry {
                Ok(e) => children.push((os_bytes(&e.file_name()).into_owned(), e.path())),
                Err(e) => {
                    diag!("tar: {}: Cannot read: {}", escape(name), strerror(&e));
                    self.fail();
                }
            }
        }
        children.sort();
        for (file_name, entry_path) in children {
            let mut entry_name = name.to_vec();
            entry_name.push(b'/');
            entry_name.extend_from_slice(&file_name);
            self.add(&entry_path, &entry_name);
        }
    }
}

#[cfg(unix)]
fn do_create(archive_file: Option<&OsStr>, files: &[OsString], verbose: Verbose) -> i32 {
    // Identified by inode, not by name: `tar -cf ./b.tar .` and `tar -cf b.tar
    // .` name the archive differently and it is the same file both times, and
    // comparing the strings would catch neither.
    let mut archive_id = None;
    let mut out: Box<dyn Write> = match archive_file {
        Some(path) => match File::create(path) {
            Ok(f) => {
                use std::os::unix::fs::MetadataExt;
                // A stat that fails is not fatal; it only costs the self-check,
                // and the archive is otherwise fine.
                if let Ok(m) = f.metadata() {
                    archive_id = Some((m.dev(), m.ino()));
                }
                Box::new(f)
            }
            Err(e) => {
                diag!("tar: {}: Cannot open: {}", escape_os(path), strerror(&e));
                return fatal();
            }
        },
        None => Box::new(io::stdout()),
    };

    let mut creator = Creator {
        out: &mut out,
        verbose,
        status: 0,
        links: BTreeMap::new(),
        last_name_prefix: Vec::new(),
        last_link_prefix: Vec::new(),
        archive_id,
        writable: true,
    };
    for operand in files {
        // The member name is the operand exactly as typed, byte for byte —
        // which is what GNU stores too.
        let name = os_bytes(operand);
        creator.add(Path::new(operand), &name);
    }

    let zero_block = [0u8; BLOCK_SIZE];
    let _ = creator.write(&zero_block) && creator.write(&zero_block);
    let mut status = creator.status;
    // The end-of-archive marker is the last thing written, so a flush that
    // fails here loses precisely the bytes that make the file a valid archive.
    if let Err(e) = out.flush() {
        diag!("tar: Cannot write: {}", strerror(&e));
        status = EXIT_FATAL;
    }
    if status == 0 {
        0
    } else {
        failed_with_previous_errors()
    }
}

// ============================================================================
// reading an archive — one decoder, shared by `-t` and `-x`
// ============================================================================

/// One member's header, decoded.
///
/// The two modes used to decode headers separately, and had drifted: `-t` and
/// `-x` each read their own 100 bytes of name, each ignored `prefix`, and each
/// stopped silently at the first block they did not understand. A single
/// decoder is not tidiness — it is the only way the listing and the extraction
/// of the same archive are guaranteed to be talking about the same members.
struct Member {
    /// The full stored name: `prefix` + `/` + `name` when `prefix` is used.
    name: Vec<u8>,
    /// The stored permission bits, all twelve of them (setuid/setgid/sticky
    /// included). What is *applied* on extraction is decided elsewhere.
    mode: u32,
    uid: u32,
    gid: u32,
    size: u64,
    mtime: i64,
    typeflag: u8,
    /// The target of a symlink, or the other name of a hard link.
    linkname: Vec<u8>,
    /// Owner and group *names*, which ustar stores beside the numbers. Empty
    /// in an archive written with `--numeric-owner`, and then `-tv` falls back
    /// to the numbers — which is what GNU does.
    uname: Vec<u8>,
    gname: Vec<u8>,
}

impl Member {
    /// Is this member a directory?
    ///
    /// The typeflag is authoritative, but the trailing slash is not merely a
    /// fallback for old archives: a v7 header has no typeflag at all, and a
    /// directory in one is recognisable only by the `/`.
    fn is_dir(&self) -> bool {
        self.typeflag == b'5'
            || (matches!(self.typeflag, b'0' | b'\0') && self.name.last() == Some(&b'/'))
    }

    /// Does this member carry data blocks after its header?
    fn has_data(&self) -> bool {
        matches!(self.typeflag, b'0' | b'\0' | b'7') && !self.is_dir()
    }

    /// The type flag to *render*, which is the stored one except that a v7
    /// directory — flagged as a regular file and recognisable only by its
    /// trailing slash — is reported as the directory it is.
    fn effective_typeflag(&self) -> u8 {
        if self.is_dir() { b'5' } else { self.typeflag }
    }
}

/// Whether a 512-byte block's stored checksum matches its contents.
///
/// This is the check that was missing, and its absence was not a nicety: with
/// no checksum test, `tar -tf not-an-archive` read 512 bytes of text, found a
/// NUL-free "name", printed it, and exited **0**. A caller cannot tell that
/// from an empty archive. GNU refuses the file outright.
///
/// Both the unsigned and the signed sum are accepted. Historic tars on
/// platforms with a signed `char` computed the sum with sign extension, so an
/// archive holding a member name with a byte above 0x7F — legal here, where a
/// name is bytes — can carry either. Refusing the signed form would reject
/// real archives written by real tars.
fn checksum_ok(block: &[u8; BLOCK_SIZE]) -> bool {
    let stored = parse_octal(block.get(148..156).unwrap_or(&[]));
    let mut unsigned: u64 = 0;
    let mut signed: i64 = 0;
    for (i, &raw) in block.iter().enumerate() {
        // The checksum field itself counts as eight spaces, which is how the
        // sum can cover a field that does not exist yet when it is computed.
        let b = if (148..156).contains(&i) { b' ' } else { raw };
        unsigned = unsigned.saturating_add(u64::from(b));
        let as_signed = i64::from(b).saturating_sub(if b >= 0x80 { 256 } else { 0 });
        signed = signed.saturating_add(as_signed);
    }
    stored == unsigned || i64::try_from(stored).is_ok_and(|s| s == signed)
}

/// Decode a header block that has already passed [`checksum_ok`].
fn decode_member(block: &[u8; BLOCK_SIZE]) -> Member {
    let name_field = field_bytes(block.get(..100).unwrap_or(&[]));
    // The `prefix` field is the whole reason ustar can hold a name longer than
    // 100 bytes, and it was never read. An archive of a deep tree therefore
    // listed and extracted every such member under its *last* 100 bytes —
    // `long/dd…dd/ff…ff` came out as `ff…ff` in the top-level directory, which
    // is silent misplacement of data, not a display bug.
    //
    // Honoured only when the magic says ustar: in the older v7 format those
    // bytes are padding, and reading them would invent a directory prefix out
    // of whatever happened to be there.
    let prefix = if block.get(257..262) == Some(b"ustar") {
        field_bytes(block.get(345..500).unwrap_or(&[]))
    } else {
        &[]
    };
    let mut name = Vec::with_capacity(
        prefix.len().saturating_add(name_field.len()).saturating_add(1),
    );
    if !prefix.is_empty() {
        name.extend_from_slice(prefix);
        name.push(b'/');
    }
    name.extend_from_slice(name_field);

    let octal32 = |range: std::ops::Range<usize>| -> u32 {
        u32::try_from(parse_octal(block.get(range).unwrap_or(&[]))).unwrap_or(0)
    };
    Member {
        name,
        mode: octal32(100..108),
        uid: octal32(108..116),
        gid: octal32(116..124),
        size: parse_octal(block.get(124..136).unwrap_or(&[])),
        // A time before the epoch cannot be stored in an octal field, so the
        // only way this saturates is a hostile header; `i64::MAX` is then
        // refused by the clock rather than silently becoming a small number.
        mtime: i64::try_from(parse_octal(block.get(136..148).unwrap_or(&[]))).unwrap_or(i64::MAX),
        typeflag: block.get(156).copied().unwrap_or(0),
        linkname: field_bytes(block.get(157..257).unwrap_or(&[])).to_vec(),
        uname: field_bytes(block.get(265..297).unwrap_or(&[])).to_vec(),
        gname: field_bytes(block.get(297..329).unwrap_or(&[])).to_vec(),
    }
}

/// Why a walk over an archive stopped.
///
/// Every variant but [`Stop::End`] used to be the same code path — `break` —
/// and the same exit status: zero. That is the defect this enum exists to
/// remove. A tool that cannot distinguish "the archive ended" from "the file
/// was never an archive" reports success for both.
#[cfg_attr(test, derive(Debug))]
enum Stop {
    /// Ran out of blocks at a header boundary. An archive may legally end
    /// without its two zero blocks and GNU accepts that in silence, so this is
    /// the *only* clean ending.
    End,
    /// Ended after a single zero block where the marker is a pair. Clean — GNU
    /// exits 0 — but it warns, and the warning carries the block's ordinal.
    LoneZeroBlock(u64),
    /// The first block was not a header: an empty file, a short read at offset
    /// zero, or a checksum that does not match.
    NotAnArchive,
    /// A later block was not a header.
    BadHeader,
    /// The stream ended inside a member's *data*. Note that ending inside a
    /// later *header* is not this — see [`walk`].
    Truncated,
    /// The archive could not be read at all — the classic case being a
    /// directory passed to `-f`, which opens and then fails at the first read.
    /// The flag is "this was the very first block", which GNU words differently
    /// ("At beginning of tape, quitting now").
    Unreadable(io::Error, bool),
    /// The handler asked to stop and has already reported why.
    Handler(i32),
}

/// Why [`read_block`] could not deliver a whole block.
enum ReadStop {
    /// Some bytes arrived, then the stream ended.
    Short,
    /// The read itself failed.
    Io(io::Error),
}

/// What a member handler did with the member's data blocks.
enum Handled {
    /// The handler read all of them.
    Consumed,
    /// The driver should skip them.
    Skip,
    /// The data ran out before the member did.
    Truncated,
    /// Stop the walk with this status; the reason is already reported.
    Stop(i32),
}

/// Read exactly one block. `Ok(None)` is a clean end at a block boundary.
fn read_block(input: &mut dyn Read, buf: &mut [u8; BLOCK_SIZE]) -> Result<Option<()>, ReadStop> {
    let mut filled = 0usize;
    while filled < BLOCK_SIZE {
        match input.read(buf.get_mut(filled..).unwrap_or(&mut [])) {
            Ok(0) => break,
            Ok(n) => filled = filled.saturating_add(n),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            // A read *error* is not an end of file, and conflating the two is
            // how `tar -tf some-directory` came to exit 0 having printed
            // nothing: `read_exact` reports both as `Err(())`.
            Err(e) => return Err(ReadStop::Io(e)),
        }
    }
    if filled == 0 {
        Ok(None)
    } else if filled == BLOCK_SIZE {
        Ok(Some(()))
    } else {
        Err(ReadStop::Short)
    }
}

/// Walk an archive, handing each member header to `handle`.
///
/// The handler is responsible for the member's data blocks only when it returns
/// [`Handled::Consumed`]; otherwise the driver skips them, which is what keeps
/// the stream aligned when a member is refused.
fn walk<F>(input: &mut dyn Read, mut handle: F) -> Stop
where
    F: FnMut(&Member, &mut dyn Read) -> Handled,
{
    let mut first = true;
    // Counts every block consumed, so the lone-zero-block warning can name one.
    let mut ordinal = 0u64;
    loop {
        let mut block = [0u8; BLOCK_SIZE];
        match read_block(input, &mut block) {
            Ok(Some(())) => ordinal = ordinal.saturating_add(1),
            Ok(None) => {
                // An empty file is not an archive; an archive that simply ran
                // out after its last member is one that ended.
                return if first { Stop::NotAnArchive } else { Stop::End };
            }
            // A short read where a *header* should start is an ending, not a
            // truncation — GNU only calls it truncation when a member's data
            // runs out. Measured on a 3584-byte archive whose members are 6 and
            // 8 bytes: `head -c 512`, `-c 513` and `-c 700` all exit 0 in
            // silence, while `-c 1024` and `-c 1100` — which cut into the first
            // member's data — give "Unexpected EOF in archive". Only a short
            // read of the *first* block is rejected outright, as GNU's "This
            // does not look like a tar archive" (`head -c 300`).
            Err(ReadStop::Short) if first => return Stop::NotAnArchive,
            Err(ReadStop::Short) => return Stop::End,
            Err(ReadStop::Io(e)) => return Stop::Unreadable(e, first),
        }

        // The end-of-archive marker is *two* zero blocks. GNU accepts one and
        // exits 0, but warns, so look ahead once to tell the two apart.
        if block.iter().all(|&b| b == 0) {
            let mut next = [0u8; BLOCK_SIZE];
            return match read_block(input, &mut next) {
                Ok(Some(())) if next.iter().all(|&b| b == 0) => Stop::End,
                _ => Stop::LoneZeroBlock(ordinal),
            };
        }
        if !checksum_ok(&block) {
            return if first {
                Stop::NotAnArchive
            } else {
                Stop::BadHeader
            };
        }
        first = false;

        let member = decode_member(&block);
        // A header that passes the checksum and still names nothing is not
        // something to guess about.
        if member.name.is_empty() {
            return Stop::BadHeader;
        }
        let size = if member.has_data() { member.size } else { 0 };
        match handle(&member, input) {
            Handled::Consumed => {}
            Handled::Skip => {
                if !skip_data(input, size) {
                    return Stop::Truncated;
                }
            }
            Handled::Truncated => return Stop::Truncated,
            Handled::Stop(s) => return Stop::Handler(s),
        }
    }
}

/// Turn the reason a walk stopped into GNU's closing diagnostics and a status.
///
/// `label` is the archive's name in bytes, for the one message that mentions
/// it — `-` when the archive is standard input, as GNU spells it.
fn report_stop(stop: Stop, label: &[u8]) -> i32 {
    match stop {
        Stop::End => 0,
        Stop::LoneZeroBlock(n) => {
            // A warning, not an error: GNU prints this and still exits 0.
            // Measured — a 3584-byte archive cut to 3072 leaves one zero block
            // as its sixth, and GNU says "A lone zero block at 6", rc 0.
            diag!("tar: A lone zero block at {n}");
            0
        }
        Stop::NotAnArchive => {
            diag!("tar: This does not look like a tar archive");
            failed_with_previous_errors()
        }
        Stop::BadHeader => {
            // GNU scans forward for the next plausible header and says so. We
            // stop instead — the remaining bytes are of unknown provenance and
            // resynchronising on them is guessing — but the line it prints is
            // the same, because what a caller needs to know is that a header
            // was not where one was expected.
            diag!("tar: Skipping to next header");
            failed_with_previous_errors()
        }
        Stop::Truncated => {
            diag!("tar: Unexpected EOF in archive");
            fatal()
        }
        Stop::Unreadable(e, at_start) => {
            diag!("tar: {}: Cannot read: {}", escape(label), strerror(&e));
            if at_start {
                // GNU's phrasing for "nothing at all was read", inherited from
                // when the archive really was on tape. Kept because it is the
                // line that distinguishes an unreadable archive from one that
                // failed part-way through.
                diag!("tar: At beginning of tape, quitting now");
            }
            fatal()
        }
        Stop::Handler(s) => s,
    }
}

/// Number of 512-byte blocks a member of `size` bytes occupies.
fn data_blocks(size: u64) -> u64 {
    size.saturating_add(BLOCK_SIZE as u64 - 1)
        .saturating_div(BLOCK_SIZE as u64)
}

/// Consume and discard a member's data blocks so the next header is read from
/// the right offset. Returns false if the archive ended early.
fn skip_data(input: &mut dyn Read, size: u64) -> bool {
    let mut block = [0u8; BLOCK_SIZE];
    for _ in 0..data_blocks(size) {
        if input.read_exact(&mut block).is_err() {
            return false;
        }
    }
    true
}

// ============================================================================
// member selection, and the metadata an extraction restores
// ============================================================================

/// The operands after the archive: which members the caller asked for.
///
/// With none, everything is wanted. With some, only the named members and —
/// this is the part that is easy to get wrong — everything *under* a named
/// directory, because `tar -xf a.tar dir` is expected to unpack the subtree,
/// not the bare directory entry.
///
/// This did not exist. `tar -xf a.tar one-file` unpacked the entire archive,
/// which is not a cosmetic difference: it writes files the caller did not ask
/// for, over whatever was already there.
struct Selector {
    /// Each operand, trailing slashes trimmed, paired with "did anything match
    /// it". The flag is what makes `NAME: Not found in archive` possible.
    wanted: Vec<(Vec<u8>, bool)>,
}

impl Selector {
    fn new(members: &[OsString]) -> Self {
        Self {
            wanted: members
                .iter()
                .map(|m| (trim_slashes(&os_bytes(m)).to_vec(), false))
                .collect(),
        }
    }

    /// Does the caller want the member named `name`? Records the match.
    fn wants(&mut self, name: &[u8]) -> bool {
        if self.wanted.is_empty() {
            return true;
        }
        // The stored name of a directory ends in `/` and the operand normally
        // does not, so both sides are trimmed before they are compared.
        let n = trim_slashes(name);
        let mut hit = false;
        for (w, matched) in &mut self.wanted {
            let under = n.len() > w.len()
                && n.get(..w.len()) == Some(w.as_slice())
                && n.get(w.len()) == Some(&b'/');
            if n == w.as_slice() || under {
                *matched = true;
                hit = true;
            }
        }
        hit
    }

    /// Complain about every operand that named nothing, GNU's way, and return a
    /// non-zero status if there was one. Silence here is what let
    /// `tar -xf a.tar typo` succeed while extracting nothing.
    fn report_missing(&self) -> i32 {
        let mut status = 0;
        for (w, matched) in &self.wanted {
            if !matched {
                diag!("tar: {}: Not found in archive", escape(w));
                status = EXIT_FATAL;
            }
        }
        status
    }
}

/// Drop trailing `/` from a member name or an operand, but never reduce a name
/// to nothing — `/` alone stays `/` rather than becoming the empty string,
/// which would then match every member.
fn trim_slashes(name: &[u8]) -> &[u8] {
    let mut end = name.len();
    while end > 1 && name.get(end.saturating_sub(1)) == Some(&b'/') {
        end = end.saturating_sub(1);
    }
    name.get(..end).unwrap_or(name)
}

// The umask has to be read to be known — POSIX gives no read-only spelling, so
// reading it means setting it — and `std` exposes no wrapper.
#[cfg(unix)]
unsafe extern "C" {
    fn umask(mask: u32) -> u32;
}

/// The process umask, read once and left as it was found.
///
/// Two halves, each needed for a measured reason:
///
/// *Cached*, because reading is destructive — the call sets the mask and returns
/// the old value, so a naive second call would answer whatever the first one
/// stored.
///
/// *Restored*, because the umask still has a job to do here. This tar sets the
/// mode of every member it extracts explicitly, so for those the kernel's mask
/// is irrelevant; but the parent directories it creates implicitly on the way to
/// a member (`dir/sub/f` extracted on its own) are left to `mkdir`, and GNU
/// lets the umask gate those. Leaving the mask at `0` made them 0777 where GNU
/// produced 0755 — visible in `scripts/tar-diff.sh` as a mode mismatch on an
/// implicitly created parent.
#[cfg(unix)]
fn read_umask() -> u32 {
    use std::sync::OnceLock;
    static UMASK: OnceLock<u32> = OnceLock::new();
    *UMASK.get_or_init(|| {
        // SAFETY: `umask` is a POSIX call that cannot fail and touches only this
        // process's file-mode creation mask. The pair leaves the mask exactly as
        // it was found; it is racy only against another thread creating a file
        // in between, and this runs before any extraction work starts.
        unsafe {
            let old = umask(0);
            umask(old);
            old
        }
    })
}

/// [`read_umask`] on the target; `0` on a host that has no such thing, so that
/// the pure arithmetic in [`extraction_mode`] is still testable there.
#[cfg(not(unix))]
fn read_umask() -> u32 {
    0
}

/// The mode an extracted member actually gets.
///
/// Measured against GNU as a non-root user: by default the stored mode is
/// masked by the umask *and* stripped of setuid, setgid and sticky — a 0777
/// file lands 0755 under umask 022 and 0700 under umask 077, and an 04755 file
/// lands 0755. With `-p` the stored mode is applied whole, setuid included.
///
/// The reasoning behind the default is worth stating because it looks
/// over-cautious: an archive is an untrusted input, and honouring a setuid bit
/// out of one would let anyone who can hand you a tarball hand you a setuid
/// binary. `-p` is the caller saying they know where the archive came from.
///
/// Pure, so it can be unit-tested on every host rather than only where a real
/// umask exists.
fn extraction_mode(stored: u32, same_permissions: bool, umask: u32) -> u32 {
    if same_permissions {
        stored & 0o7777
    } else {
        stored & 0o777 & !umask
    }
}

/// Apply a member's stored mode and mtime to a path that has been created.
///
/// Both were dropped entirely: an extracted file kept whatever mode
/// `File::create` gave it and whatever time it was written at, so unpacking a
/// tree of scripts produced a tree of non-executable files, and every `make`
/// run after an unpack rebuilt everything.
///
/// The two failures are reported separately, and neither aborts the other: a
/// filesystem that cannot store a timestamp can usually still store a mode, and
/// getting one of the two right is better than getting neither.
///
/// The wording of the mode failure is GNU's, symbolic bits and all
/// (`Cannot change mode to rwxr-xr-x`), which is why [`mode_string`] is shared
/// with the `-tv` listing rather than each having its own.
fn restore_metadata(name: &[u8], path: &Path, mode: u32, mtime: i64, status: &mut i32) {
    if let Err(e) = set_mtime(path, mtime) {
        diag!("tar: {}: Cannot utime: {}", escape(name), strerror(&e));
        *status = EXIT_FATAL;
    }
    if let Err(e) = set_mode(path, mode) {
        let bits = mode_string(mode, b'0');
        diag!(
            "tar: {}: Cannot change mode to {}: {}",
            escape(name),
            String::from_utf8_lossy(bits.get(1..).unwrap_or(&[])),
            strerror(&e)
        );
        *status = EXIT_FATAL;
    }
}

/// Set a path's modification time (and its access time with it — ustar stores
/// only the one, and leaving atime at "now" would be a lie of the same size).
fn set_mtime(path: &Path, mtime: i64) -> io::Result<()> {
    use std::fs::FileTimes;
    use std::time::{Duration, SystemTime};
    let t = if mtime >= 0 {
        SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(mtime.unsigned_abs()))
    } else {
        SystemTime::UNIX_EPOCH.checked_sub(Duration::from_secs(mtime.unsigned_abs()))
    };
    // A header can hold a time no clock can represent; refusing it is right,
    // and refusing it *loudly* is what tells the caller the tree is not the
    // archive.
    let Some(t) = t else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "timestamp out of range",
        ));
    };
    // A read handle, not a write one: `futimens` is permitted on a read-only
    // descriptor, and a directory — which this must also work on — cannot be
    // opened for writing at all.
    let f = File::open(path)?;
    f.set_times(FileTimes::new().set_accessed(t).set_modified(t))
}

/// Set a path's permission bits. A no-op off unix, where there are none to set.
#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> io::Result<()> {
    Ok(())
}

fn do_extract(
    archive_file: Option<&OsStr>,
    directory: Option<&OsStr>,
    verbose: Verbose,
    members: &[OsString],
    same_permissions: bool,
) -> i32 {
    // The archive is opened before the `-C` chdir, so its own path is resolved
    // against the directory the user was standing in, as GNU does.
    let mut input: Box<dyn Read> = match archive_file {
        Some(path) => match File::open(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                diag!("tar: {}: Cannot open: {}", escape_os(path), strerror(&e));
                return fatal();
            }
        },
        None => Box::new(io::stdin()),
    };

    if let Some(dir) = directory
        && let Err(e) = env::set_current_dir(dir)
    {
        diag!("tar: {}: Cannot chdir: {}", escape_os(dir), strerror(&e));
        return fatal();
    }

    let mut status = 0;
    let mut warned_absolute = false;
    let mut warned_dotdot = false;
    let mut selector = Selector::new(members);
    let umask = read_umask();
    // Directory metadata is applied last, in reverse order. It has to be: a
    // directory's mtime is bumped by every child written into it, and a
    // directory whose stored mode has no write bit cannot receive children at
    // all. GNU defers both for the same reason, which is why `tar -xf` restores
    // a 0500 directory's mode *and* its timestamp, and ours restored neither.
    let mut pending_dirs: Vec<(Vec<u8>, u32, i64)> = Vec::new();

    let stop = walk(input.as_mut(), |member, input| {
        let raw_name = member.name.as_slice();

        // GNU prints this once, not once per member, and only when it applies.
        // The quoting is GNU's own — a grave accent opening and an apostrophe
        // closing — and not this project's `'...'`. It is copied rather than
        // normalised because the line is an interface: a script grepping for
        // it is grepping for tar's wording, not for ours.
        if raw_name.first() == Some(&b'/') && !warned_absolute {
            diag!("tar: Removing leading `/' from member names");
            warned_absolute = true;
        }
        // And the same once-only notice for a name that starts by climbing out.
        // GNU prints it *and then still refuses the member*, which reads like a
        // contradiction until you see that the two come from different places:
        // the notice is about the prefix it would have stripped, the refusal is
        // about the `..` still in the name. Both lines are reproduced because
        // both are what a caller comparing stderr will see.
        if raw_name
            .split(|&b| b == b'/')
            .find(|c| !c.is_empty())
            .is_some_and(|c| c == b"..")
            && !warned_dotdot
        {
            diag!("tar: Removing leading `../' from member names");
            warned_dotdot = true;
        }

        // Operands select members. With none, everything is wanted — but a
        // member the caller did not ask for must be skipped *before* anything
        // is written, which is why this test comes first. `tar -xf a.tar one`
        // used to unpack the whole archive.
        if !selector.wants(raw_name) {
            return Handled::Skip;
        }

        // Nothing below may use `raw_name` as a path. It is attacker-chosen.
        let name = match sanitize_member_name(raw_name) {
            Ok(n) => n,
            Err(e) => {
                diag!("tar: {e}");
                status = EXIT_FATAL;
                return Handled::Skip;
            }
        };

        // The announced name is the member's, with the trailing slash a
        // directory carries — `tree/`, not `tree`. That slash is how the reader
        // of a `-v` listing tells a directory from a file, and stripping it (as
        // the sanitiser must, since it is building a path) threw the
        // distinction away.
        if verbose != Verbose::Off {
            let mut shown = name.clone();
            if member.is_dir() {
                shown.push(b'/');
            }
            verbose.line(&shown);
        }

        match member.typeflag {
            _ if member.is_dir() => {
                if let Err(e) = fs::create_dir_all(os_from_bytes(&name)) {
                    diag!("tar: {}: Cannot mkdir: {}", escape(&name), strerror(&e));
                    status = EXIT_FATAL;
                } else {
                    pending_dirs.push((
                        name,
                        extraction_mode(member.mode, same_permissions, umask),
                        member.mtime,
                    ));
                }
                Handled::Skip
            }
            b'0' | b'\0' | b'7' => {
                let mode = extraction_mode(member.mode, same_permissions, umask);
                if extract_regular_file(input, &name, member.size, mode, member.mtime, &mut status)
                {
                    Handled::Consumed
                } else {
                    Handled::Truncated
                }
            }
            other => {
                // Hard links, symlinks, devices, FIFOs and the GNU long-name
                // extensions all land here. Skipping them keeps the stream in
                // sync, but the extracted tree is then not the archive, so say
                // so rather than pretending it worked.
                // The type flag is one byte of the archive's own header, so it
                // is exactly as attacker-chosen as the name beside it, which
                // is already quoted. `char::from(b'\n')` printed a raw
                // newline, so a crafted archive could forge a line of `tar`'s
                // stderr; `quoteaf` renders that byte as `''$'\n'`.
                diag!(
                    "tar: {}: unsupported entry type {}; skipped",
                    escape(&name),
                    quoteaf(&[other])
                );
                status = EXIT_FATAL;
                Handled::Skip
            }
        }
    });

    // Deepest first: `pending_dirs` is in archive order, which is parents
    // before children, so the reverse leaves a parent's timestamp untouched by
    // work still to be done inside it.
    for (name, mode, mtime) in pending_dirs.into_iter().rev() {
        let path = os_from_bytes(&name);
        restore_metadata(&name, Path::new(&path), mode, mtime, &mut status);
    }

    let missing = selector.report_missing();
    let walk_status = report_stop(stop, &archive_label(archive_file));
    if walk_status != 0 {
        return walk_status;
    }
    if status == 0 && missing == 0 {
        0
    } else {
        failed_with_previous_errors()
    }
}

/// The archive's name for a diagnostic: its path, or `-` for standard input.
fn archive_label(archive_file: Option<&OsStr>) -> Vec<u8> {
    archive_file.map_or_else(|| b"-".to_vec(), |p| os_bytes(p).into_owned())
}

/// Stream one regular member out of `input` into `name`. Returns false when
/// the archive ended mid-member, which means the outer loop must stop.
///
/// This streams rather than buffering. The previous version did
/// `Vec::with_capacity(size)` from the header's own size field, so an archive
/// whose header claimed 2^40 bytes made this program try to reserve a
/// terabyte before reading a single block — a one-line denial of service
/// costing the attacker 512 bytes of file.
fn extract_regular_file(
    input: &mut dyn Read,
    name: &[u8],
    size: u64,
    mode: u32,
    mtime: i64,
    status: &mut i32,
) -> bool {
    // `name` has been through `sanitize_member_name`, so it is a relative path
    // of `/`-separated non-`..` components — but its bytes are still the
    // archive's, and are turned back into a path without inspecting them.
    let path = os_from_bytes(name);
    let path = Path::new(&path);
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = fs::create_dir_all(parent)
    {
        diag!("tar: {}: Cannot mkdir: {}", escape_os(parent), strerror(&e));
        *status = EXIT_FATAL;
        return skip_data(input, size);
    }

    let mut file = match File::create(path) {
        Ok(f) => Some(f),
        Err(e) => {
            // Still consume the data: the archive may hold members after this
            // one, and abandoning the stream would lose them too.
            diag!("tar: {}: Cannot open: {}", escape(name), strerror(&e));
            *status = EXIT_FATAL;
            None
        }
    };

    let mut remaining = size;
    let mut block = [0u8; BLOCK_SIZE];
    for _ in 0..data_blocks(size) {
        if input.read_exact(&mut block).is_err() {
            diag!("tar: Unexpected EOF in archive");
            *status = EXIT_FATAL;
            return false;
        }
        let take = usize::try_from(remaining)
            .unwrap_or(BLOCK_SIZE)
            .min(BLOCK_SIZE);
        remaining = remaining.saturating_sub(take as u64);
        if let Some(f) = file.as_mut()
            && let Err(e) = f.write_all(block.get(..take).unwrap_or(&[]))
        {
            diag!("tar: {}: Cannot write: {}", escape(name), strerror(&e));
            *status = EXIT_FATAL;
            // Drop the handle so the rest of the member is only skipped, but
            // keep reading so the following headers stay aligned.
            file = None;
        }
    }
    // Buffered data is not the issue here (`File` is unbuffered), but a
    // filesystem that reports a write error at close would otherwise be
    // ignored, which is the same defect as the discarded `write_all` above.
    let wrote = match file {
        Some(mut f) => {
            if let Err(e) = f.flush() {
                diag!("tar: {}: Cannot write: {}", escape(name), strerror(&e));
                *status = EXIT_FATAL;
                false
            } else {
                true
            }
        }
        None => false,
    };
    // Only for a file this call actually created. Stamping the mode and time of
    // a member that could not be opened would be writing the archive's metadata
    // onto whatever was already at that path.
    if wrote {
        restore_metadata(name, path, mode, mtime, status);
    }
    true
}

fn do_list_main(archive_file: Option<&OsStr>, verbose: bool, members: &[OsString]) -> i32 {
    let mut input: Box<dyn Read> = match archive_file {
        Some(path) => match File::open(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                diag!("tar: {}: Cannot open: {}", escape_os(path), strerror(&e));
                return fatal();
            }
        },
        None => Box::new(io::stdin()),
    };

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    let mut selector = Selector::new(members);
    // Read once, before any member is printed. `-tv` renders every timestamp in
    // the machine's zone, and re-resolving `TZ` per member would let a listing
    // straddle a zone change mid-file.
    let zone = localtime::Zone::from_env();

    let (stop, write_err) = list_archive(input.as_mut(), &mut out, verbose, &mut selector, &zone);

    let flush_err = out.flush().err();
    if let Some(e) = write_err.or(flush_err) {
        // `tar -tf big.tar | head -5` closes the pipe on purpose; that is how
        // a pipeline ends, not a failure of this program.
        if e.kind() == io::ErrorKind::BrokenPipe {
            return 0;
        }
        diag!("tar: 'standard output': Cannot write: {}", strerror(&e));
        return fatal();
    }

    // Order matters: a member the caller named and the archive does not hold is
    // worth saying even when the archive also ended badly, because the two are
    // different complaints about different things.
    let missing = selector.report_missing();
    let walk_status = report_stop(stop, &archive_label(archive_file));
    if walk_status != 0 {
        return walk_status;
    }
    if missing == 0 {
        0
    } else {
        failed_with_previous_errors()
    }
}

/// The width GNU's `-tv` reserves for `user/group` plus the size, before any
/// member has been seen.
///
/// It is a running maximum that starts at 18 and only ever grows, which is why
/// a listing's columns line up within one archive and need not line up between
/// two. Reproduced rather than replaced by a two-pass measurement because the
/// output is an interface — `tar -tv | awk '{print $3}'` is a real idiom, and a
/// listing whose columns differ from GNU's breaks it for no gain.
const UGSWIDTH_MIN: usize = 18;

/// List an archive's members to `out`.
///
/// Returns the reason the walk stopped and the first write error, if any; the
/// caller turns those into a status. Splitting it that way is what lets the
/// unit tests drive a synthetic archive through the real code path and inspect
/// both the bytes written and *why* the read ended — the old version returned
/// `io::Result<()>` and answered `Ok(())` for a truncated archive, a corrupt
/// one, and a file that was never an archive alike.
fn list_archive(
    input: &mut dyn Read,
    out: &mut dyn Write,
    verbose: bool,
    selector: &mut Selector,
    zone: &localtime::Zone,
) -> (Stop, Option<io::Error>) {
    let mut ugswidth = UGSWIDTH_MIN;
    let mut write_err: Option<io::Error> = None;

    let stop = walk(input, |member, _data| {
        if !selector.wants(&member.name) {
            return Handled::Skip;
        }
        // Listing shows the name as stored, not the sanitized one: the point
        // of `tar -t` is to tell you what is in the archive, and a member
        // called `../../etc/passwd` is exactly what you want to be shown.
        //
        // Shown through `escape`, which is what GNU does and is why a name that
        // is not UTF-8 comes out as `caf\351.txt` rather than as the bytes
        // themselves. The earlier reasoning here — that `tar -t` output must be
        // feedable back to `tar -x`, so the bytes must survive — was wrong on
        // its own terms: GNU's output is not feedable back either, and a name
        // containing a newline would put two lines in the manifest.
        let line = if verbose {
            long_line(member, &mut ugswidth, zone)
        } else {
            let mut l = escape(&member.name).into_bytes();
            l.push(b'\n');
            l
        };
        if let Err(e) = out.write_all(&line) {
            write_err = Some(e);
            // Zero, not a failure status: the reason is carried in `write_err`
            // and a closed pipe is not an error at all.
            return Handled::Stop(0);
        }
        Handled::Skip
    });

    (stop, write_err)
}

/// One line of `tar -tv`, byte for byte as GNU lays it out.
///
/// The column arithmetic is GNU's and was measured rather than guessed:
/// `pad` counts the user, the group, the size and the one `/` between the
/// first two, `ugswidth` is the running maximum of every `pad` seen so far (and
/// never less than [`UGSWIDTH_MIN`]), and the gap before the size is
/// `ugswidth - pad + 1` spaces. Confirmed against `tar -tvf` for numeric
/// owners (`1000/1000`, 9 spaces), for names (`inhahe/inhahe`, 5), for a 20 MiB
/// member (2), and for a 46-column `user/group` where the gap collapses to the
/// single space the formula's `+ 1` guarantees.
fn long_line(member: &Member, ugswidth: &mut usize, zone: &localtime::Zone) -> Vec<u8> {
    // ustar stores the owner's *name* beside the number; `--numeric-owner`
    // leaves it empty and GNU then prints the number. Falling back the other
    // way — looking the uid up in this machine's passwd file — would be wrong:
    // the archive may come from a machine where uid 1000 is someone else.
    let user = if member.uname.is_empty() {
        member.uid.to_string().into_bytes()
    } else {
        member.uname.clone()
    };
    let group = if member.gname.is_empty() {
        member.gid.to_string().into_bytes()
    } else {
        member.gname.clone()
    };
    // A directory, a link and a device occupy no data blocks, and GNU prints 0
    // for them whatever the header's size field happens to say.
    let size = if member.has_data() { member.size } else { 0 };
    let size = size.to_string().into_bytes();

    let pad = user
        .len()
        .saturating_add(group.len())
        .saturating_add(size.len())
        .saturating_add(1);
    *ugswidth = (*ugswidth).max(pad);
    let gap = ugswidth.saturating_sub(pad).saturating_add(1);

    let mut line = mode_string(member.mode, member.effective_typeflag());
    line.push(b' ');
    line.extend_from_slice(&user);
    line.push(b'/');
    line.extend_from_slice(&group);
    line.resize(line.len().saturating_add(gap), b' ');
    line.extend_from_slice(&size);
    line.push(b' ');

    let tm = zone.local(member.mtime, 0);
    line.extend_from_slice(&localtime::strftime(b"%Y-%m-%d %H:%M", &tm));
    line.push(b' ');
    // The name and the link target are escaped; the user and group names above
    // are not. That asymmetry is GNU's — `print_header` passes the name and the
    // linkname through `quotearg` and prints the owner fields with a plain
    // `%s` — and it is the reason the column arithmetic can use the owner
    // lengths as they stand: nothing before the name column can change width.
    line.extend_from_slice(escape(&member.name).as_bytes());

    // GNU's two suffixes, and part of the reason `-tv` is worth having over
    // `-t`: a symlink's target and a hard link's other name are stored in the
    // header and are invisible in a plain listing.
    match member.typeflag {
        b'2' => {
            line.extend_from_slice(b" -> ");
            line.extend_from_slice(escape(&member.linkname).as_bytes());
        }
        b'1' => {
            line.extend_from_slice(b" link to ");
            line.extend_from_slice(escape(&member.linkname).as_bytes());
        }
        _ => {}
    }
    line.push(b'\n');
    line
}

/// The ten-character `drwxr-xr-x` rendering of a type and a mode.
///
/// Shared between the `-tv` listing and the `Cannot change mode to ...`
/// diagnostic, which is GNU's arrangement too — the two must agree, since a
/// user comparing them is comparing the mode that was asked for against the
/// mode that is there.
fn mode_string(mode: u32, typeflag: u8) -> Vec<u8> {
    let kind = match typeflag {
        b'1' => b'h',
        b'2' => b'l',
        b'3' => b'c',
        b'4' => b'b',
        b'5' => b'd',
        b'6' => b'p',
        b'7' => b'C',
        b'0' | b'\0' => b'-',
        // A type this tar does not know is not a regular file, and saying so is
        // more use than pretending.
        _ => b'?',
    };
    let mut out = vec![kind];
    // (read, write, execute, the bit that overrides the execute character,
    //  its letter when execute is set, its letter when execute is clear)
    let triads: [(u32, u32, u32, u32, u8, u8); 3] = [
        (0o400, 0o200, 0o100, 0o4000, b's', b'S'),
        (0o040, 0o020, 0o010, 0o2000, b's', b'S'),
        (0o004, 0o002, 0o001, 0o1000, b't', b'T'),
    ];
    for (r, w, x, extra, set, unset) in triads {
        out.push(if mode & r != 0 { b'r' } else { b'-' });
        out.push(if mode & w != 0 { b'w' } else { b'-' });
        out.push(match (mode & x != 0, mode & extra != 0) {
            (true, false) => b'x',
            (true, true) => set,
            (false, true) => unset,
            (false, false) => b'-',
        });
    }
    out
}

/// Take the used part of a fixed-size, NUL-padded header field.
///
/// Borrows rather than decoding. This was `extract_string`, which ran the
/// bytes through `String::from_utf8_lossy` — and since it is what read the
/// 100-byte `name` field, every member name in the archive passed through it
/// before anything else saw it. A member legitimately named with a byte that
/// is not UTF-8 (legal on this OS: any byte but `/` and NUL) was therefore
/// *listed* under a different name by `-t` and *extracted* under a different
/// name by `-x`, with each offending byte replaced by U+FFFD — silent
/// renaming, not a display quirk, and irreversible. See `known-issues.md` →
/// `B-tar-READ-EVERY-PATH-AS-UTF-8`.
fn field_bytes(buf: &[u8]) -> &[u8] {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    buf.get(..end).unwrap_or(&[])
}

/// Parse a NUL/space-padded octal field into a `u64`.  Non-octal input
/// silently parses as 0 (matching common tar implementations on
/// malformed archives).
///
/// A field that is not ASCII is not octal either, so the `from_utf8` failure
/// path lands on the same 0 as `"garbage"` does; the lossy decode this used to
/// go through could only ever have turned one non-number into another.
fn parse_octal(buf: &[u8]) -> u64 {
    let trimmed = field_bytes(buf).trim_ascii();
    str::from_utf8(trimmed)
        .ok()
        .and_then(|s| u64::from_str_radix(s, 8).ok())
        .unwrap_or(0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use localtime::Zone;

    fn s(items: &[&str]) -> Vec<OsString> {
        items.iter().map(|x| OsString::from(*x)).collect()
    }

    /// Build an argv out of raw byte strings, so a test can pass an argument
    /// that no `&str` can hold.
    fn b(items: &[&[u8]]) -> Vec<OsString> {
        items.iter().map(|x| os_from_bytes(x)).collect()
    }

    /// Build a single tar header block with the given name and size.
    fn make_header(name: &[u8], size: u64, typeflag: u8) -> [u8; BLOCK_SIZE] {
        let mut h = TarHeader::new();
        h.set_name(name).unwrap();
        TarHeader::set_octal(&mut h.mode, 0o644);
        TarHeader::set_octal(&mut h.uid, 0);
        TarHeader::set_octal(&mut h.gid, 0);
        TarHeader::set_octal(&mut h.size, size);
        TarHeader::set_octal(&mut h.mtime, 0);
        h.typeflag = typeflag;
        h.magic = *b"ustar\0";
        h.version = *b"00";
        h.compute_checksum();
        *h.as_bytes()
    }

    /// Drive the real listing path over a synthetic archive, in the plain
    /// (`-t`) form with no member operands and a fixed zone, and hand back the
    /// reason it stopped. The zone is UTC rather than the machine's so that a
    /// timestamp assertion means the same thing on every machine that runs the
    /// suite.
    fn list_names(input: &[u8], out: &mut Vec<u8>) -> Stop {
        let mut sel = Selector::new(&[]);
        let (stop, err) = list_archive(&mut &input[..], out, false, &mut sel, &Zone::utc());
        assert!(err.is_none(), "unexpected write error listing to a Vec");
        stop
    }

    /// As [`list_names`], in the long (`-tv`) form.
    fn list_long(input: &[u8], out: &mut Vec<u8>) -> Stop {
        let mut sel = Selector::new(&[]);
        let (stop, err) = list_archive(&mut &input[..], out, true, &mut sel, &Zone::utc());
        assert!(err.is_none(), "unexpected write error listing to a Vec");
        stop
    }

    /// A header with every field a `-tv` line reads set explicitly.
    #[allow(clippy::too_many_arguments)]
    fn make_full_header(
        name: &[u8],
        mode: u32,
        uid: u32,
        gid: u32,
        size: u64,
        mtime: u64,
        typeflag: u8,
        linkname: &[u8],
        uname: &[u8],
        gname: &[u8],
    ) -> [u8; BLOCK_SIZE] {
        let mut h = TarHeader::new();
        h.set_name(name).unwrap();
        TarHeader::set_octal(&mut h.mode, u64::from(mode));
        TarHeader::set_octal(&mut h.uid, u64::from(uid));
        TarHeader::set_octal(&mut h.gid, u64::from(gid));
        TarHeader::set_octal(&mut h.size, size);
        TarHeader::set_octal(&mut h.mtime, mtime);
        h.typeflag = typeflag;
        h.linkname[..linkname.len()].copy_from_slice(linkname);
        h.uname[..uname.len()].copy_from_slice(uname);
        h.gname[..gname.len()].copy_from_slice(gname);
        h.magic = *b"ustar\0";
        h.version = *b"00";
        h.compute_checksum();
        *h.as_bytes()
    }

    // ---------------- parse_args ----------------

    #[test]
    fn parse_empty() {
        let a = parse_args(&s(&[])).unwrap();
        assert_eq!(a, TarArgs::default());
    }

    #[test]
    fn parse_create_with_file() {
        let a = parse_args(&s(&["-c", "-f", "out.tar", "a", "b"])).unwrap();
        assert!(a.create);
        assert_eq!(a.archive_file.as_deref(), Some(OsStr::new("out.tar")));
        assert_eq!(a.files, s(&["a", "b"]));
    }

    #[test]
    fn parse_clustered_create_verbose_file() {
        // -cvf out.tar a -- the f consumes the next argv element.
        let a = parse_args(&s(&["-cvf", "out.tar", "a"])).unwrap();
        assert!(a.create);
        assert!(a.verbose);
        assert_eq!(a.archive_file.as_deref(), Some(OsStr::new("out.tar")));
        assert_eq!(a.files, s(&["a"]));
    }

    #[test]
    fn parse_extract_with_directory() {
        let a = parse_args(&s(&["-x", "-C", "/tmp", "-f", "in.tar"])).unwrap();
        assert!(a.extract);
        assert_eq!(a.directory.as_deref(), Some(OsStr::new("/tmp")));
        assert_eq!(a.archive_file.as_deref(), Some(OsStr::new("in.tar")));
    }

    #[test]
    fn parse_list() {
        let a = parse_args(&s(&["-tf", "x.tar"])).unwrap();
        assert!(a.list);
        assert_eq!(a.archive_file.as_deref(), Some(OsStr::new("x.tar")));
    }

    #[test]
    fn parse_unknown_flag_errors() {
        let err = parse_args(&s(&["-Z"])).unwrap_err();
        // Byte for byte what GNU tar 1.35 says for `tar -Q`, less the
        // `tar: ` prefix the caller adds.
        assert_eq!(err, "invalid option -- 'Z'");
    }

    #[test]
    fn parse_missing_f_value_errors() {
        // argp's wording, byte for byte: `tar -cf` says exactly this and exits
        // 64. See `scripts/tar-diff.sh`, case "-f with no argument".
        let err = parse_args(&s(&["-f"])).unwrap_err();
        assert_eq!(err, "option requires an argument -- 'f'");
    }

    #[test]
    fn parse_missing_c_value_errors() {
        let err = parse_args(&s(&["-C"])).unwrap_err();
        assert_eq!(err, "option requires an argument -- 'C'");
    }

    #[test]
    fn parse_files_with_dashes_handled() {
        // Bare positional arg starting with non-dash is a file.
        let a = parse_args(&s(&["-c", "f1", "f2"])).unwrap();
        assert!(a.create);
        assert_eq!(a.files, s(&["f1", "f2"]));
    }

    // The point of the byte conversion: every one of these arguments is a
    // legal filename on this OS (`design.txt`: any byte but `/` and NUL), and
    // every one of them made `tar` abort at startup, before touching an
    // archive, when argv was read as `String`. See `known-issues.md` ->
    // `B-tar-READ-EVERY-PATH-AS-UTF-8`.

    #[test]
    fn parse_keeps_an_operand_that_is_not_utf8() {
        let a = parse_args(&b(&[b"-c", b"caf\xe9", b"ok"])).unwrap();
        assert!(a.create);
        assert_eq!(a.files, b(&[b"caf\xe9", b"ok"]));
        // Not merely "did not crash": the bytes are the ones passed in, so the
        // file that gets archived is the file that was named.
        assert_eq!(os_bytes(&a.files[0]).as_ref(), b"caf\xe9");
    }

    #[test]
    fn parse_keeps_a_dash_f_value_that_is_not_utf8() {
        let a = parse_args(&b(&[b"-cf", b"\xff\xfe.tar", b"x"])).unwrap();
        let f = a.archive_file.unwrap();
        assert_eq!(os_bytes(&f).as_ref(), b"\xff\xfe.tar");
    }

    #[test]
    fn parse_keeps_a_dash_c_value_that_is_not_utf8() {
        let a = parse_args(&b(&[b"-x", b"-C", b"/tmp/d\x80r"])).unwrap();
        assert!(a.extract);
        let d = a.directory.unwrap();
        assert_eq!(os_bytes(&d).as_ref(), b"/tmp/d\x80r");
    }

    #[test]
    fn parse_refuses_a_cluster_byte_that_is_not_an_option_without_panicking() {
        // A cluster is walked byte by byte, so a `-` followed by something
        // that is not UTF-8 at all is refused like any other unknown flag
        // rather than being a case the parser cannot represent.
        let err = parse_args(&b(&[b"-\xe9"])).unwrap_err();
        assert!(err.contains("invalid option"), "{err}");
        // `quoteaf` renders the byte rather than emitting it raw, so the
        // message cannot forge a line of tar's stderr.
        assert!(!err.as_bytes().contains(&0xe9), "{err}");
    }

    // ---------------- sanitize_member_name ----------------
    //
    // Every case here was a file written outside the destination directory
    // before the sanitizer existed. See known-issues.md ->
    // B-tar-EXTRACTS-OUTSIDE-THE-DESTINATION-DIRECTORY.

    #[test]
    fn sanitize_plain_relative_name_unchanged() {
        assert_eq!(sanitize_member_name(b"a/b/c.txt").unwrap(), b"a/b/c.txt");
        assert_eq!(sanitize_member_name(b"file.txt").unwrap(), b"file.txt");
    }

    #[test]
    fn sanitize_strips_leading_slash() {
        // Critical: `Path::join` with an absolute path throws the base away,
        // so an unstripped name would ignore `-C` entirely.
        assert_eq!(sanitize_member_name(b"/etc/passwd").unwrap(), b"etc/passwd");
        assert_eq!(
            sanitize_member_name(b"///etc/passwd").unwrap(),
            b"etc/passwd"
        );
    }

    #[test]
    fn sanitize_drops_dot_and_doubled_slashes() {
        assert_eq!(sanitize_member_name(b"./a//b/./c").unwrap(), b"a/b/c");
    }

    #[test]
    fn sanitize_refuses_leading_dotdot() {
        assert!(sanitize_member_name(b"../../etc/passwd").is_err());
    }

    #[test]
    fn sanitize_refuses_interior_dotdot() {
        // `a/../b` is only equivalent to `b` if `a` is a real directory and
        // not a symlink -- which the archive is the last thing to trust about.
        assert!(sanitize_member_name(b"a/../../etc/passwd").is_err());
        assert!(sanitize_member_name(b"a/../b").is_err());
    }

    #[test]
    fn sanitize_refuses_absolute_dotdot_combination() {
        assert!(sanitize_member_name(b"/../../root/.ssh/authorized_keys").is_err());
    }

    #[test]
    fn sanitize_refuses_backslash_traversal() {
        // Not a separator on this OS, but this file's tests -- and any host
        // build -- run where it is.
        assert!(sanitize_member_name(b"..\\..\\windows\\system32\\x").is_err());
        assert!(sanitize_member_name(b"a/..\\b").is_err());
    }

    #[test]
    fn sanitize_passes_a_non_utf8_name_through_byte_for_byte() {
        // The name is what will be created on disk, so any alteration here is
        // a silent rename. When this function took `&str` the caller had
        // already replaced `\xe9` with U+FFFD, so the file was extracted under
        // a name the archive does not contain -- and the `..` check was being
        // made about that altered string rather than about what would be
        // written.
        assert_eq!(sanitize_member_name(b"caf\xe9/x").unwrap(), b"caf\xe9/x");
        assert_eq!(sanitize_member_name(b"/\xff\xfe").unwrap(), b"\xff\xfe");
    }

    #[test]
    fn sanitize_refuses_dotdot_beside_non_utf8_components() {
        // The traversal check is on the bytes, so it is not weakened by
        // neighbouring components that are not text.
        assert!(sanitize_member_name(b"\xe9/../../etc/passwd").is_err());
    }

    #[test]
    fn sanitize_keeps_a_lone_backslash_in_a_name() {
        // A slateos filename may contain a backslash; only a `..` component
        // is refused, and the name is rebuilt with `/` alone.
        assert_eq!(sanitize_member_name(b"a/b\\c").unwrap(), b"a/b\\c");
    }

    #[test]
    fn sanitize_refuses_names_that_reduce_to_nothing() {
        assert!(sanitize_member_name(b"").is_err());
        assert!(sanitize_member_name(b"/").is_err());
        assert!(sanitize_member_name(b"./.").is_err());
    }

    #[test]
    fn sanitize_allows_dotfiles_and_names_starting_with_dots() {
        // `..foo` and `...` are ordinary names, not traversal.
        assert_eq!(sanitize_member_name(b".bashrc").unwrap(), b".bashrc");
        assert_eq!(sanitize_member_name(b"a/..foo").unwrap(), b"a/..foo");
        assert_eq!(sanitize_member_name(b"a/...").unwrap(), b"a/...");
    }

    // ---------------- data_blocks ----------------

    #[test]
    fn data_blocks_rounds_up() {
        assert_eq!(data_blocks(0), 0);
        assert_eq!(data_blocks(1), 1);
        assert_eq!(data_blocks(512), 1);
        assert_eq!(data_blocks(513), 2);
        assert_eq!(data_blocks(1024), 2);
    }

    #[test]
    fn data_blocks_does_not_overflow() {
        // A hostile header can name any u64; this must not wrap to 0 blocks
        // and desynchronise the reader.
        assert!(data_blocks(u64::MAX) > 0);
    }

    // ---------------- field_bytes / parse_octal ----------------

    #[test]
    fn field_bytes_stops_at_nul() {
        let buf = b"hello\0\0\0world";
        assert_eq!(field_bytes(buf), b"hello");
    }

    #[test]
    fn field_bytes_no_nul_uses_all() {
        let buf = b"hello";
        assert_eq!(field_bytes(buf), b"hello");
    }

    #[test]
    fn field_bytes_empty() {
        assert_eq!(field_bytes(&[]), b"");
        assert_eq!(field_bytes(&[0u8; 8]), b"");
    }

    #[test]
    fn field_bytes_does_not_decode() {
        // The whole reason this is not `extract_string`: the 100-byte name
        // field holds a filename, and a filename here is bytes. Decoding it
        // lossily renamed the member.
        let mut buf = [0u8; 100];
        buf[..5].copy_from_slice(b"a\xff\xfeb\xc3");
        assert_eq!(field_bytes(&buf), b"a\xff\xfeb\xc3");
    }

    #[test]
    fn parse_octal_basic() {
        let mut buf = [0u8; 12];
        buf[..4].copy_from_slice(b"0755");
        assert_eq!(parse_octal(&buf), 0o755);
    }

    #[test]
    fn parse_octal_space_padded() {
        let mut buf = [0u8; 12];
        buf[..6].copy_from_slice(b"  0755");
        assert_eq!(parse_octal(&buf), 0o755);
    }

    #[test]
    fn parse_octal_garbage_is_zero() {
        let buf = *b"garbage\0\0\0\0\0";
        assert_eq!(parse_octal(&buf), 0);
    }

    #[test]
    fn parse_octal_empty_is_zero() {
        assert_eq!(parse_octal(&[]), 0);
    }

    #[test]
    fn parse_octal_non_ascii_is_zero() {
        // A hostile header can put any byte in the size field. Non-UTF-8 is
        // not a number, so it takes the same path as `garbage` -- and must
        // take it without panicking, since the result feeds `data_blocks`.
        let mut buf = [0u8; 12];
        buf[..3].copy_from_slice(b"\xff\xfe7");
        assert_eq!(parse_octal(&buf), 0);
    }

    #[test]
    fn parse_octal_rejects_digits_outside_the_base() {
        // `8` and `9` are not octal; the whole field is refused rather than
        // truncated at the bad digit, which would read a wrong size.
        let mut buf = [0u8; 12];
        buf[..3].copy_from_slice(b"789");
        assert_eq!(parse_octal(&buf), 0);
    }

    // ---------------- TarHeader::set_octal ----------------

    #[test]
    fn set_octal_basic() {
        let mut f = [0u8; 8];
        TarHeader::set_octal(&mut f, 0o755);
        assert_eq!(parse_octal(&f), 0o755);
        // Trailing byte should remain NUL.
        assert_eq!(f.get(7), Some(&0));
    }

    #[test]
    fn set_octal_zero() {
        let mut f = [0u8; 8];
        TarHeader::set_octal(&mut f, 0);
        assert_eq!(parse_octal(&f), 0);
    }

    #[test]
    fn set_octal_large_value_round_trips() {
        let mut f = [0u8; 12];
        TarHeader::set_octal(&mut f, 1_234_567);
        assert_eq!(parse_octal(&f), 1_234_567);
    }

    #[test]
    fn set_octal_empty_field_noop() {
        let mut f: [u8; 0] = [];
        TarHeader::set_octal(&mut f, 0o755); // must not panic
    }

    // ---------------- TarHeader::compute_checksum ----------------

    #[test]
    fn checksum_is_stable() {
        let mut h1 = TarHeader::new();
        h1.set_name(b"foo").unwrap();
        TarHeader::set_octal(&mut h1.mode, 0o644);
        h1.compute_checksum();

        let mut h2 = TarHeader::new();
        h2.set_name(b"foo").unwrap();
        TarHeader::set_octal(&mut h2.mode, 0o644);
        h2.compute_checksum();

        assert_eq!(h1.checksum, h2.checksum);
    }

    #[test]
    fn checksum_changes_with_name() {
        let mut h1 = TarHeader::new();
        h1.set_name(b"foo").unwrap();
        h1.compute_checksum();

        let mut h2 = TarHeader::new();
        h2.set_name(b"bar").unwrap();
        h2.compute_checksum();

        assert_ne!(h1.checksum, h2.checksum);
    }

    // ---------------- list_archive ----------------

    #[test]
    fn list_empty_archive_writes_nothing() {
        let mut input: Vec<u8> = Vec::new();
        // Two zero blocks = empty archive.
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert!(out.is_empty());
        assert!(matches!(stop, Stop::End), "{stop:?}");
    }

    #[test]
    fn list_single_zero_byte_file() {
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header(b"hello.txt", 0, b'0'));
        // No data blocks (size = 0).
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "hello.txt\n");
        assert!(matches!(stop, Stop::End), "{stop:?}");
    }

    #[test]
    fn list_single_file_with_data() {
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header(b"data.bin", 100, b'0'));
        // 100-byte file occupies 1 data block.
        input.extend_from_slice(&[b'x'; BLOCK_SIZE]);
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "data.bin\n");
        assert!(matches!(stop, Stop::End), "{stop:?}");
    }

    #[test]
    fn list_multiple_files() {
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header(b"a.txt", 0, b'0'));
        input.extend_from_slice(&make_header(b"b.txt", 600, b'0'));
        // 600-byte file = ceil(600/512) = 2 data blocks.
        input.extend_from_slice(&[b'y'; BLOCK_SIZE]);
        input.extend_from_slice(&[b'y'; BLOCK_SIZE]);
        input.extend_from_slice(&make_header(b"c.txt", 0, b'0'));
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        let listing = String::from_utf8(out).unwrap();
        assert_eq!(listing, "a.txt\nb.txt\nc.txt\n");
        assert!(matches!(stop, Stop::End), "{stop:?}");
    }

    #[test]
    fn list_escapes_a_byte_that_is_not_a_character() {
        // Through `String::from_utf8_lossy` and `writeln!` this printed
        // `caf<U+FFFD>.txt` -- three bytes where one belongs, and the same
        // three for every distinct bad byte, so two different members could
        // list under one name. GNU's answer is the octal escape, which is
        // unambiguous and stays on one line. Measured.
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header(b"caf\xe9.txt", 0, b'0'));
        input.extend_from_slice(&[0u8; BLOCK_SIZE * 2]);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "caf\\351.txt\n");
        assert!(matches!(stop, Stop::End), "{stop:?}");
    }

    #[test]
    fn list_leaves_a_valid_multibyte_name_alone() {
        // The other half of the rule: `escape` escapes what is not a character,
        // not what is not ASCII. GNU under a UTF-8 locale prints `café.txt`
        // whole and only falls back to octal for the bytes that decode to
        // nothing.
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header("café.txt".as_bytes(), 0, b'0'));
        input.extend_from_slice(&[0u8; BLOCK_SIZE * 2]);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "café.txt\n");
        assert!(matches!(stop, Stop::End), "{stop:?}");
    }

    /// `t/` + `n`×`d` + `/` + `m`×`f`, the shape every split test uses.
    fn deep(dirlen: usize, filelen: usize) -> Vec<u8> {
        let mut v = b"t/".to_vec();
        v.extend(std::iter::repeat_n(b'd', dirlen));
        v.push(b'/');
        v.extend(std::iter::repeat_n(b'f', filelen));
        v
    }

    #[test]
    fn a_name_that_exactly_fills_the_field_keeps_its_last_byte() {
        // The bug this replaced: `name[..99]`, so a 100-byte name lost a byte.
        // The field has no terminator when it is full, which is legal ustar and
        // is what GNU writes.
        let full = deep(96, 1);
        assert_eq!(full.len(), 100);
        assert_eq!(split_ustar_name(&full), Ok((&b""[..], &full[..])));
        let mut h = TarHeader::new();
        h.set_name(&full).unwrap();
        assert_eq!(&h.name[..], &full[..]);
        assert_eq!(h.prefix[0], 0);
    }

    #[test]
    fn one_byte_past_the_field_moves_the_directory_into_the_prefix() {
        // Measured against GNU: `t/` + 96×`d` + `/fff` -> prefix 98, name 3.
        let full = deep(96, 3);
        assert_eq!(full.len(), 102);
        let (prefix, name) = split_ustar_name(&full).unwrap();
        assert_eq!(prefix.len(), 98);
        assert_eq!(name, b"fff");
        // The `/` at the seam is not stored in either field.
        assert_eq!(prefix.last(), Some(&b'd'));
    }

    #[test]
    fn the_remainder_may_be_a_hundred_bytes_but_not_a_hundred_and_one() {
        // GNU accepts the first and refuses the second. The boundary matters
        // because it is the one place a name that *fits in 256 bytes* is still
        // rejected, and getting it wrong either drops a file GNU keeps or
        // writes a header GNU would not.
        let ok = deep(3, 100);
        assert_eq!(split_ustar_name(&ok).unwrap().1.len(), 100);
        let refused = deep(3, 101);
        assert_eq!(split_ustar_name(&refused), Err(NameTooLong::CannotSplit));
    }

    #[test]
    fn a_single_component_too_long_cannot_be_split() {
        let mut full = b"t/".to_vec();
        full.extend(std::iter::repeat_n(b'f', 200));
        assert_eq!(split_ustar_name(&full), Err(NameTooLong::CannotSplit));
    }

    #[test]
    fn past_two_hundred_and_fifty_six_bytes_is_the_other_refusal() {
        // Two different GNU messages, so two different errors: "max 256" when
        // no split could ever work, "cannot be split" when the pieces do not
        // land where ustar needs them.
        let full = deep(200, 100);
        assert_eq!(full.len(), 303);
        assert_eq!(split_ustar_name(&full), Err(NameTooLong::Max));
    }

    #[test]
    fn a_directory_name_that_only_a_zero_length_name_would_fit_is_refused() {
        // 153 bytes: `t/` + 150×`d` + `/`. A 152-byte prefix and an empty name
        // would hold it, and GNU still refuses -- the backward search starts at
        // the trailing slash, skips it, and then finds only the `/` at offset 1,
        // leaving 151 bytes for a 100-byte field. Measured; this is the case
        // that proves the search is capped-and-backward rather than "last `/`".
        let mut full = b"t/".to_vec();
        full.extend(std::iter::repeat_n(b'd', 150));
        full.push(b'/');
        assert_eq!(full.len(), 153);
        assert_eq!(split_ustar_name(&full), Err(NameTooLong::CannotSplit));
    }

    #[test]
    #[cfg(unix)]
    fn a_leading_slash_run_is_removed_whole_not_one_slash_at_a_time() {
        // The message quotes the prefix it removed, so `//a` must report `//`.
        assert_eq!(strip_leading(b"/a/b"), (&b"/"[..], &b"a/b"[..]));
        assert_eq!(strip_leading(b"//a/b"), (&b"//"[..], &b"a/b"[..]));
        assert_eq!(strip_leading(b"///a"), (&b"///"[..], &b"a"[..]));
    }

    #[test]
    #[cfg(unix)]
    fn a_leading_dotdot_component_is_removed_but_a_leading_dot_is_not() {
        // Measured: GNU stores `./t` unchanged and says nothing, but turns
        // `../t` into `t` with a message. A `.` takes an extractor nowhere it
        // was not already; a `..` takes it out of the destination.
        assert_eq!(strip_leading(b"../t"), (&b"../"[..], &b"t"[..]));
        assert_eq!(strip_leading(b"../../t"), (&b"../../"[..], &b"t"[..]));
        assert_eq!(strip_leading(b"/../t"), (&b"/../"[..], &b"t"[..]));
        assert_eq!(strip_leading(b"./t"), (&b""[..], &b"./t"[..]));
        assert_eq!(strip_leading(b".t"), (&b""[..], &b".t"[..]));
        // `...` is a legal file name and is not two dots followed by anything.
        assert_eq!(strip_leading(b".../t"), (&b""[..], &b".../t"[..]));
        // A `..` in the middle is not a *leading* prefix and is left alone; the
        // extractor is what refuses those.
        assert_eq!(strip_leading(b"a/../b"), (&b""[..], &b"a/../b"[..]));
    }

    #[test]
    #[cfg(unix)]
    fn a_name_that_is_nothing_but_prefix_is_left_with_nothing() {
        // `tar -c ..` -- the caller asked for the parent directory, and every
        // byte of the name is a prefix tar removes. GNU stores `.` (listed as
        // `./`), which is the only name left that means anything.
        assert_eq!(strip_leading(b".."), (&b".."[..], &b""[..]));
        assert_eq!(strip_leading(b"/"), (&b"/"[..], &b""[..]));
        assert_eq!(strip_leading(b""), (&b""[..], &b""[..]));
    }

    #[test]
    fn a_link_target_that_does_not_fit_is_cut_rather_than_refused() {
        // GNU says "not dumped" and dumps it anyway with the target cut to 100
        // bytes. Measured -- a 101-byte symlink target warns, exits 2, and is
        // in the archive. The boolean is what drives the warning.
        let mut h = TarHeader::new();
        assert!(h.set_linkname(&[b'y'; 100]));
        assert_eq!(&h.linkname[..], &[b'y'; 100][..]);
        let mut h = TarHeader::new();
        assert!(!h.set_linkname(&[b'y'; 101]));
        assert_eq!(&h.linkname[..], &[b'y'; 100][..]);
    }

    #[test]
    #[cfg(unix)]
    fn device_numbers_split_the_way_the_kernel_packs_them() {
        // /dev/null is 1,3 -- measured, GNU lists it as `crw-rw-rw- 0/0 1,3`.
        assert_eq!(split_dev(0x0103), (1, 3));
        // A minor past 255 spills into the high half rather than into major.
        assert_eq!(split_dev((8 << 8) | 0x11), (8, 0x11));
    }

    #[test]
    fn list_keeps_a_name_holding_a_newline_on_one_line() {
        // The reason escaping is not merely cosmetic: a member called `a\nb`
        // printed raw makes `tar -t` report two files, one of them named `b`.
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header(b"a\nb.txt", 0, b'0'));
        input.extend_from_slice(&[0u8; BLOCK_SIZE * 2]);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "a\\nb.txt\n");
        assert!(matches!(stop, Stop::End), "{stop:?}");
    }

    #[test]
    fn a_non_utf8_name_survives_the_header_round_trip() {
        // Store and read back, since `set_name` and `field_bytes` are the two
        // ends of the conversion and either alone could be lossless while the
        // pair is not.
        let name = b"\xff/dir\x80/\xe9\xe9";
        let block = make_header(name, 0, b'0');
        assert_eq!(field_bytes(block.get(..100).unwrap()), name);
        // And the sanitizer, which stands between the two, does not touch it.
        assert_eq!(
            sanitize_member_name(field_bytes(block.get(..100).unwrap())).unwrap(),
            name
        );
    }

    #[test]
    fn list_reports_a_truncated_archive_rather_than_succeeding() {
        // Header announces a 1024-byte file but no data follows. The name was
        // already printed, so the listing is not empty -- but the walk must end
        // in `Truncated`, which is what turns into GNU's `Unexpected EOF in
        // archive` and a status of 2. This returned `Ok(())` and exited 0.
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header(b"liar.bin", 1024, b'0'));
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "liar.bin\n");
        assert!(matches!(stop, Stop::Truncated), "{stop:?}");
    }

    #[test]
    fn list_treats_a_short_later_header_as_a_clean_end() {
        // The counterpart to the test above, and the distinction GNU actually
        // draws: a partial block where a *header* would start is an ending, not
        // a truncation. Measured -- `head -c 700` of a 3584-byte archive (one
        // full member, then 188 bytes of the next header) exits 0 in silence.
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header(b"whole.txt", 0, b'0'));
        input.extend_from_slice(&[0xab; 188]);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "whole.txt\n");
        assert!(matches!(stop, Stop::End), "{stop:?}");
    }

    #[test]
    fn list_warns_about_a_lone_zero_block_but_still_succeeds() {
        // The end-of-archive marker is two zero blocks. One is accepted -- GNU
        // exits 0 -- but it warns and names the block, counting from 1: here the
        // header is block 1 and the zero block is block 2.
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header(b"a.txt", 0, b'0'));
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "a.txt\n");
        assert!(matches!(stop, Stop::LoneZeroBlock(2)), "{stop:?}");
    }

    #[test]
    fn list_is_silent_about_a_proper_two_block_marker() {
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header(b"a.txt", 0, b'0'));
        input.extend_from_slice(&[0u8; BLOCK_SIZE * 2]);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "a.txt\n");
        assert!(matches!(stop, Stop::End), "{stop:?}");
    }

    #[test]
    fn list_refuses_a_short_first_block() {
        // Less than one full header block: nothing is written, and the file is
        // reported as not being an archive at all.
        let input = vec![0u8; 100];
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert!(out.is_empty());
        assert!(matches!(stop, Stop::NotAnArchive), "{stop:?}");
    }

    #[test]
    fn list_refuses_a_file_that_is_not_an_archive() {
        // The defect this whole reader exists to fix: 512 bytes of text have a
        // NUL-free "name" in the first 100, so the old listing printed a line of
        // the file's own contents and exited 0.
        let input = vec![b'A'; BLOCK_SIZE * 2];
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert!(out.is_empty(), "{:?}", String::from_utf8_lossy(&out));
        assert!(matches!(stop, Stop::NotAnArchive), "{stop:?}");
    }

    #[test]
    fn list_reports_a_bad_checksum_on_a_later_header() {
        // A good first member proves the file is an archive, so a corrupt
        // second header is a *different* complaint from "not an archive" -- GNU
        // says `Skipping to next header` for one and `This does not look like a
        // tar archive` for the other.
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header(b"good.txt", 0, b'0'));
        let mut bad = make_header(b"bad.txt", 0, b'0');
        bad[148..156].copy_from_slice(b"000000\0 ");
        input.extend_from_slice(&bad);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "good.txt\n");
        assert!(matches!(stop, Stop::BadHeader), "{stop:?}");
    }

    #[test]
    fn list_joins_the_ustar_prefix_to_the_name() {
        // The `prefix` field is how ustar stores a name longer than 100 bytes,
        // and it was never read -- so `long/dd.../ff...` listed (and extracted)
        // as just `ff...`, in the top-level directory.
        let mut block = make_header(b"leaf.txt", 0, b'0');
        block[345..345 + 8].copy_from_slice(b"deep/dir");
        // The checksum covers the prefix, so it has to be recomputed.
        let sum: u32 = block
            .iter()
            .enumerate()
            .map(|(i, &b)| u32::from(if (148..156).contains(&i) { b' ' } else { b }))
            .sum();
        let cs = format!("{sum:06o}\0 ");
        block[148..156].copy_from_slice(cs.as_bytes());

        let mut input = block.to_vec();
        input.extend_from_slice(&[0u8; BLOCK_SIZE * 2]);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "deep/dir/leaf.txt\n");
        assert!(matches!(stop, Stop::End), "{stop:?}");
    }

    #[test]
    fn list_ignores_the_prefix_field_in_a_v7_header() {
        // In v7 those 155 bytes are padding, so reading them would invent a
        // directory out of whatever happened to be there.
        let mut block = make_header(b"leaf.txt", 0, b'0');
        block[257..263].copy_from_slice(&[0u8; 6]); // no `ustar` magic
        block[345..345 + 8].copy_from_slice(b"deep/dir");
        let sum: u32 = block
            .iter()
            .enumerate()
            .map(|(i, &b)| u32::from(if (148..156).contains(&i) { b' ' } else { b }))
            .sum();
        let cs = format!("{sum:06o}\0 ");
        block[148..156].copy_from_slice(cs.as_bytes());

        let mut input = block.to_vec();
        input.extend_from_slice(&[0u8; BLOCK_SIZE * 2]);
        let mut out = Vec::new();
        let stop = list_names(&input, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "leaf.txt\n");
        assert!(matches!(stop, Stop::End), "{stop:?}");
    }

    // ---------------- the -tv long format ----------------

    /// GNU, measured, for an archive of one 6-byte 0755 file owned by 1000:1000
    /// with mtime 2020-01-02 03:04:05 UTC:
    ///
    /// ```text
    /// -rwxr-xr-x 1000/1000         6 2020-01-02 03:04 t/a.txt
    /// ```
    #[test]
    fn long_format_matches_gnu_for_numeric_owners() {
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_full_header(
            b"t/a.txt",
            0o755,
            1000,
            1000,
            6,
            1_577_934_245,
            b'0',
            b"",
            b"",
            b"",
        ));
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        let mut out = Vec::new();
        list_long(&input, &mut out);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "-rwxr-xr-x 1000/1000         6 2020-01-02 03:04 t/a.txt\n"
        );
    }

    /// And with names stored, where `pad` is 14 rather than 10 and the gap
    /// narrows from nine spaces to five:
    ///
    /// ```text
    /// -rwxr-xr-x inhahe/inhahe     6 2020-01-02 03:04 t/a.txt
    /// ```
    #[test]
    fn long_format_prefers_the_stored_owner_names() {
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_full_header(
            b"t/a.txt",
            0o755,
            1000,
            1000,
            6,
            1_577_934_245,
            b'0',
            b"",
            b"inhahe",
            b"inhahe",
        ));
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        let mut out = Vec::new();
        list_long(&input, &mut out);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "-rwxr-xr-x inhahe/inhahe     6 2020-01-02 03:04 t/a.txt\n"
        );
    }

    /// A 20 MiB member takes eight digits, so `pad` is 17 and the gap is two.
    /// The *next*, narrower line still uses the same width-18 column, which is
    /// what makes a listing's columns line up.
    #[test]
    fn long_format_column_is_a_running_maximum() {
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_full_header(
            b"big",
            0o644,
            1000,
            1000,
            20_971_520,
            1_577_934_245,
            b'0',
            b"",
            b"",
            b"",
        ));
        // 20 MiB of data blocks, heap-allocated: a 20 MiB array literal is a
        // stack overflow, not a test.
        input.extend_from_slice(&vec![0u8; BLOCK_SIZE * 40_960]);
        input.extend_from_slice(&make_full_header(
            b"t/a.txt",
            0o755,
            1000,
            1000,
            6,
            1_577_934_245,
            b'0',
            b"",
            b"",
            b"",
        ));
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        let mut out = Vec::new();
        list_long(&input, &mut out);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "-rw-r--r-- 1000/1000  20971520 2020-01-02 03:04 big\n\
             -rwxr-xr-x 1000/1000         6 2020-01-02 03:04 t/a.txt\n"
        );
    }

    /// Past 18 the column grows to fit, and the gap collapses to the single
    /// space the `+ 1` in the formula guarantees. Measured against GNU with
    /// `--owner=averyverylongusername --group=averyverylonggroupname`.
    #[test]
    fn long_format_column_grows_past_the_minimum() {
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_full_header(
            b"t/a.txt",
            0o644,
            1000,
            1000,
            6,
            1_577_934_245,
            b'0',
            b"",
            b"averyverylongusername",
            b"averyverylonggroupname",
        ));
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        let mut out = Vec::new();
        list_long(&input, &mut out);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "-rw-r--r-- averyverylongusername/averyverylonggroupname 6 \
             2020-01-02 03:04 t/a.txt\n"
        );
    }

    /// The two suffixes, the directory's trailing slash, and the type letters.
    #[test]
    fn long_format_renders_every_member_type() {
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_full_header(
            b"t/", 0o755, 1000, 1000, 0, 1_577_934_245, b'5', b"", b"", b"",
        ));
        input.extend_from_slice(&make_full_header(
            b"t/fifo",
            0o644,
            1000,
            1000,
            0,
            1_577_934_245,
            b'6',
            b"",
            b"",
            b"",
        ));
        input.extend_from_slice(&make_full_header(
            b"t/hard",
            0o755,
            1000,
            1000,
            0,
            1_577_934_245,
            b'1',
            b"t/a.txt",
            b"",
            b"",
        ));
        input.extend_from_slice(&make_full_header(
            b"t/link",
            0o777,
            1000,
            1000,
            0,
            1_577_934_245,
            b'2',
            b"a.txt",
            b"",
            b"",
        ));
        input.extend_from_slice(&make_full_header(
            b"t/su", 0o4755, 1000, 1000, 3, 1_577_934_245, b'0', b"", b"", b"",
        ));
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        let mut out = Vec::new();
        list_long(&input, &mut out);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "drwxr-xr-x 1000/1000         0 2020-01-02 03:04 t/\n\
             prw-r--r-- 1000/1000         0 2020-01-02 03:04 t/fifo\n\
             hrwxr-xr-x 1000/1000         0 2020-01-02 03:04 t/hard link to t/a.txt\n\
             lrwxrwxrwx 1000/1000         0 2020-01-02 03:04 t/link -> a.txt\n\
             -rwsr-xr-x 1000/1000         3 2020-01-02 03:04 t/su\n"
        );
    }

    // ---------------- mode_string ----------------

    #[test]
    fn mode_string_renders_the_override_bits() {
        assert_eq!(mode_string(0o755, b'0'), b"-rwxr-xr-x");
        assert_eq!(mode_string(0o644, b'0'), b"-rw-r--r--");
        assert_eq!(mode_string(0o000, b'0'), b"----------");
        assert_eq!(mode_string(0o777, b'5'), b"drwxrwxrwx");
        // setuid/setgid/sticky replace the execute letter, and their capital
        // form is how you tell "set, and not executable" from "not set".
        assert_eq!(mode_string(0o4755, b'0'), b"-rwsr-xr-x");
        assert_eq!(mode_string(0o4644, b'0'), b"-rwSr--r--");
        assert_eq!(mode_string(0o2755, b'0'), b"-rwxr-sr-x");
        assert_eq!(mode_string(0o2745, b'0'), b"-rwxr-Sr-x");
        assert_eq!(mode_string(0o1777, b'5'), b"drwxrwxrwt");
        assert_eq!(mode_string(0o1776, b'5'), b"drwxrwxrwT");
    }

    #[test]
    fn mode_string_names_every_type() {
        for (flag, letter) in [
            (b'0', b'-'),
            (b'\0', b'-'),
            (b'1', b'h'),
            (b'2', b'l'),
            (b'3', b'c'),
            (b'4', b'b'),
            (b'5', b'd'),
            (b'6', b'p'),
            (b'7', b'C'),
            (b'x', b'?'),
        ] {
            assert_eq!(mode_string(0, flag).first(), Some(&letter), "flag {flag:?}");
        }
    }

    // ---------------- extraction_mode ----------------

    /// Measured against GNU as a non-root user: the stored mode is masked by
    /// the umask and stripped of setuid/setgid/sticky, unless `-p` is given.
    #[test]
    fn extraction_mode_applies_the_umask_by_default() {
        assert_eq!(extraction_mode(0o777, false, 0o022), 0o755);
        assert_eq!(extraction_mode(0o777, false, 0o077), 0o700);
        assert_eq!(extraction_mode(0o644, false, 0o022), 0o644);
        assert_eq!(extraction_mode(0o755, false, 0o000), 0o755);
    }

    #[test]
    fn extraction_mode_drops_setuid_unless_asked() {
        // An archive is an untrusted input; honouring its setuid bit would let
        // anyone who can hand you a tarball hand you a setuid binary.
        assert_eq!(extraction_mode(0o4755, false, 0o022), 0o755);
        assert_eq!(extraction_mode(0o2755, false, 0o022), 0o755);
        assert_eq!(extraction_mode(0o1777, false, 0o022), 0o755);
        // `-p` is the caller saying they know where the archive came from.
        assert_eq!(extraction_mode(0o4755, true, 0o022), 0o4755);
        assert_eq!(extraction_mode(0o777, true, 0o077), 0o777);
    }

    // ---------------- Selector ----------------

    #[test]
    fn selector_with_no_operands_wants_everything() {
        let mut sel = Selector::new(&[]);
        assert!(sel.wants(b"anything"));
        assert!(sel.wants(b"a/b/c"));
        assert_eq!(sel.report_missing(), 0);
    }

    #[test]
    fn selector_matches_a_named_member_and_its_subtree() {
        // `tar -xf a.tar dir` unpacks the subtree, not the bare entry -- and
        // with no selector at all this used to unpack the whole archive.
        let mut sel = Selector::new(&s(&["t/sub"]));
        assert!(sel.wants(b"t/sub/"));
        assert!(sel.wants(b"t/sub/b.bin"));
        assert!(!sel.wants(b"t/a.txt"));
        // Not a prefix match on bytes: `t/subterranean` is a different name.
        assert!(!sel.wants(b"t/subterranean"));
        assert_eq!(sel.report_missing(), 0);
    }

    #[test]
    fn selector_ignores_trailing_slashes_on_either_side() {
        let mut sel = Selector::new(&s(&["t/sub/"]));
        assert!(sel.wants(b"t/sub"));
        assert!(sel.wants(b"t/sub/b.bin"));
    }

    #[test]
    fn selector_reports_an_operand_that_matched_nothing() {
        let mut sel = Selector::new(&s(&["present", "absent"]));
        assert!(sel.wants(b"present"));
        assert_eq!(sel.report_missing(), EXIT_FATAL);
    }

    #[test]
    fn selector_takes_an_operand_that_is_not_utf8() {
        let mut sel = Selector::new(&b(&[b"caf\xe9"]));
        assert!(sel.wants(b"caf\xe9"));
        assert!(sel.wants(b"caf\xe9/inside"));
        assert!(!sel.wants(b"cafe"));
        assert_eq!(sel.report_missing(), 0);
    }

    #[test]
    fn trim_slashes_never_empties_a_name() {
        assert_eq!(trim_slashes(b"a/"), b"a");
        assert_eq!(trim_slashes(b"a///"), b"a");
        assert_eq!(trim_slashes(b"a"), b"a");
        // `/` alone would otherwise become the empty string, which prefixes
        // every member name there is.
        assert_eq!(trim_slashes(b"/"), b"/");
        assert_eq!(trim_slashes(b""), b"");
    }
}
