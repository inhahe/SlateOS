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
use coreutils::quote::{quoteaf, quotef_os};
use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use std::process;

/// GNU tar's exit status for "a fatal error occurred". Used for every failure
/// that leaves the archive or the extracted tree incomplete, because a caller
/// that only sees 0 has no way to discover that half its files are missing.
const EXIT_FATAL: i32 = 2;

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
    archive_file: Option<String>,
    directory: Option<String>,
    files: Vec<String>,
}

/// Parse tar's argv.  Supports clustered short flags; `f` and `C`
/// consume the following argv element as their value (even when
/// clustered as e.g. `-xvf`, in which case the next argv is the value
/// of `f`).  Unknown short flags return an error.
fn parse_args(args: &[String]) -> Result<TarArgs, String> {
    let mut out = TarArgs::default();
    let mut i: usize = 0;

    while let Some(arg) = args.get(i) {
        if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") {
            let rest = arg.get(1..).unwrap_or("");
            for c in rest.chars() {
                match c {
                    'c' => out.create = true,
                    'x' => out.extract = true,
                    't' => out.list = true,
                    'v' => out.verbose = true,
                    'f' => {
                        i = i.saturating_add(1);
                        let v = args
                            .get(i)
                            .ok_or_else(|| "option -f requires an argument".to_string())?;
                        out.archive_file = Some(v.clone());
                    }
                    'C' => {
                        i = i.saturating_add(1);
                        let v = args
                            .get(i)
                            .ok_or_else(|| "option -C requires an argument".to_string())?;
                        out.directory = Some(v.clone());
                    }
                    other => {
                        return Err(format!("unknown option: -{other}"));
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
    let args: Vec<String> = env::args().skip(1).collect();
    let parsed = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            diag!("tar: {e}");
            process::exit(1);
        }
    };

    // Every mode returns its own status rather than exiting inline, so that
    // "some members failed" survives to the caller. A tool that reports 0
    // after writing half an archive is worse than one that fails outright:
    // the script that invoked it deletes the source and moves on.
    let status = if parsed.create {
        #[cfg(unix)]
        {
            do_create(&parsed.archive_file, &parsed.files, parsed.verbose)
        }
        #[cfg(not(unix))]
        {
            diag!("tar: create mode is unix-only on this build");
            EXIT_FATAL
        }
    } else if parsed.extract {
        do_extract(
            &parsed.archive_file,
            parsed.directory.as_deref(),
            parsed.verbose,
        )
    } else if parsed.list {
        do_list_main(&parsed.archive_file)
    } else {
        diag!("tar: must specify -c, -x, or -t");
        1
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

    fn set_name(&mut self, name: &str) {
        let bytes = name.as_bytes();
        let len = bytes.len().min(99);
        if let (Some(dst), Some(src)) = (self.name.get_mut(..len), bytes.get(..len)) {
            dst.copy_from_slice(src);
        }
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
fn sanitize_member_name(raw: &str) -> Result<String, String> {
    let mut parts: Vec<&str> = Vec::new();
    for component in raw.split('/') {
        if component.is_empty() || component == "." {
            // A leading empty component is the absolute-path `/`; an interior
            // one is a doubled slash. Both are dropped.
            continue;
        }
        if component == ".." || component.split('\\').any(|p| p == "..") {
            return Err(format!(
                "refusing to extract '{raw}': member name escapes the destination directory"
            ));
        }
        parts.push(component);
    }
    if parts.is_empty() {
        return Err(format!("refusing to extract '{raw}': empty member name"));
    }
    Ok(parts.join("/"))
}

#[cfg(unix)]
fn do_create(archive_file: &Option<String>, files: &[String], verbose: bool) -> i32 {
    let mut out: Box<dyn Write> = match archive_file {
        Some(path) => match File::create(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                diag!("tar: {}: {e}", quotef_os(path));
                return EXIT_FATAL;
            }
        },
        None => Box::new(io::stdout()),
    };

    /// Report a write failure once and give up on the archive. There is no
    /// point continuing: every later member would land at the wrong offset,
    /// producing a file that looks like an archive and is not one.
    fn write_or_fail(out: &mut dyn Write, buf: &[u8], status: &mut i32) -> bool {
        match out.write_all(buf) {
            Ok(()) => true,
            Err(e) => {
                diag!("tar: write error: {e}");
                *status = EXIT_FATAL;
                false
            }
        }
    }

    fn add_file(path: &Path, name: &str, out: &mut dyn Write, verbose: bool, status: &mut i32) {
        use std::os::unix::fs::MetadataExt;
        let meta = match fs::metadata(path) {
            Ok(m) => m,
            Err(e) => {
                diag!("tar: {}: {e}", quotef_os(name));
                *status = EXIT_FATAL;
                return;
            }
        };

        // The header commits to a length, so the body must be exactly that
        // many bytes however the read goes. Writing fewer would not merely
        // truncate this member: the extractor reads a fixed number of blocks
        // per header, so every subsequent member would be read from the wrong
        // offset and the whole archive after this point would be garbage.
        let declared = meta.len();

        let mut f = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                diag!("tar: {}: {e}", quotef_os(name));
                *status = EXIT_FATAL;
                return;
            }
        };

        let mut header = TarHeader::new();
        header.set_name(name);
        TarHeader::set_octal(&mut header.mode, u64::from(meta.mode()) & 0o7777);
        TarHeader::set_octal(&mut header.uid, u64::from(meta.uid()));
        TarHeader::set_octal(&mut header.gid, u64::from(meta.gid()));
        TarHeader::set_octal(&mut header.size, declared);
        TarHeader::set_octal(&mut header.mtime, meta.mtime().unsigned_abs());
        header.typeflag = b'0';
        header.magic = *b"ustar\0";
        header.version = *b"00";
        header.compute_checksum();
        if !write_or_fail(out, header.as_bytes(), status) {
            return;
        }

        if verbose {
            diag!("{name}");
        }

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
                        diag!("tar: {}: {e}", quotef_os(name));
                        *status = EXIT_FATAL;
                        short = true;
                    }
                }
            }
            if let Some(pad) = buf.get_mut(filled..) {
                pad.fill(0);
            }
            if !write_or_fail(out, &buf, status) {
                return;
            }
            remaining = remaining.saturating_sub(want as u64);
        }
        if short {
            // The file shrank between `metadata` and the read, or never had
            // the length it claimed. The archive stays well-formed because the
            // remaining blocks were padded, but it no longer holds the file.
            diag!(
                "tar: {}: file shorter than expected; padded with zeros",
                quotef_os(name)
            );
            *status = EXIT_FATAL;
        }
    }

    fn add_directory_recursive(
        dir: &Path,
        prefix: &str,
        out: &mut dyn Write,
        verbose: bool,
        status: &mut i32,
    ) {
        let mut header = TarHeader::new();
        let name = format!("{prefix}/");
        header.set_name(&name);
        TarHeader::set_octal(&mut header.mode, 0o755);
        TarHeader::set_octal(&mut header.uid, 0);
        TarHeader::set_octal(&mut header.gid, 0);
        TarHeader::set_octal(&mut header.size, 0);
        TarHeader::set_octal(&mut header.mtime, 0);
        header.typeflag = b'5';
        header.magic = *b"ustar\0";
        header.version = *b"00";
        header.compute_checksum();
        if !write_or_fail(out, header.as_bytes(), status) {
            return;
        }

        if verbose {
            diag!("{name}");
        }

        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) => {
                // Previously `if let Ok(entries)`, so an unreadable directory
                // produced an archive silently missing its whole subtree.
                diag!("tar: {}: {e}", quotef_os(prefix));
                *status = EXIT_FATAL;
                return;
            }
        };
        // Sorted so that archiving the same tree twice produces the same
        // bytes; `read_dir` order is whatever the filesystem feels like.
        let mut children: Vec<(String, std::path::PathBuf)> = Vec::new();
        for entry in entries {
            match entry {
                Ok(e) => children.push((e.file_name().to_string_lossy().into_owned(), e.path())),
                Err(e) => {
                    diag!("tar: {}: {e}", quotef_os(prefix));
                    *status = EXIT_FATAL;
                }
            }
        }
        children.sort();
        for (file_name, entry_path) in children {
            let entry_name = format!("{prefix}/{file_name}");
            if entry_path.is_dir() {
                add_directory_recursive(&entry_path, &entry_name, out, verbose, status);
            } else {
                add_file(&entry_path, &entry_name, out, verbose, status);
            }
        }
    }

    let mut status = 0;
    for path_str in files {
        let path = Path::new(path_str);
        if path.is_dir() {
            add_directory_recursive(path, path_str, &mut out, verbose, &mut status);
        } else {
            add_file(path, path_str, &mut out, verbose, &mut status);
        }
    }

    let zero_block = [0u8; BLOCK_SIZE];
    let _ = write_or_fail(&mut out, &zero_block, &mut status)
        && write_or_fail(&mut out, &zero_block, &mut status);
    // The end-of-archive marker is the last thing written, so a flush that
    // fails here loses precisely the bytes that make the file a valid archive.
    if let Err(e) = out.flush() {
        diag!("tar: write error: {e}");
        status = EXIT_FATAL;
    }
    status
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

fn do_extract(archive_file: &Option<String>, directory: Option<&str>, verbose: bool) -> i32 {
    // The archive is opened before the `-C` chdir, so its own path is resolved
    // against the directory the user was standing in, as GNU does.
    let mut input: Box<dyn Read> = match archive_file {
        Some(path) => match File::open(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                diag!("tar: {}: {e}", quotef_os(path));
                return EXIT_FATAL;
            }
        },
        None => Box::new(io::stdin()),
    };

    if let Some(dir) = directory
        && let Err(e) = env::set_current_dir(dir)
    {
        diag!("tar: {}: {e}", quotef_os(dir));
        return EXIT_FATAL;
    }

    let mut status = 0;
    let mut warned_absolute = false;

    loop {
        let mut header_buf = [0u8; BLOCK_SIZE];
        if input.read_exact(&mut header_buf).is_err() {
            break;
        }

        if header_buf.iter().all(|&b| b == 0) {
            break;
        }

        let raw_name = extract_string(header_buf.get(..100).unwrap_or(&[]));
        let size = parse_octal(header_buf.get(124..136).unwrap_or(&[]));
        let typeflag = header_buf.get(156).copied().unwrap_or(0);

        if raw_name.is_empty() {
            break;
        }

        // GNU prints this once, not once per member, and only when it applies.
        if raw_name.starts_with('/') && !warned_absolute {
            diag!("tar: Removing leading '/' from member names");
            warned_absolute = true;
        }

        // Nothing below may use `raw_name` as a path. It is attacker-chosen.
        let name = match sanitize_member_name(&raw_name) {
            Ok(n) => n,
            Err(e) => {
                diag!("tar: {e}");
                status = EXIT_FATAL;
                if !skip_data(input.as_mut(), size) {
                    break;
                }
                continue;
            }
        };

        if verbose {
            diag!("{name}");
        }

        match typeflag {
            b'5' | b'\0' if raw_name.ends_with('/') => {
                if let Err(e) = fs::create_dir_all(&name) {
                    diag!("tar: {}: {e}", quotef_os(&name));
                    status = EXIT_FATAL;
                }
            }
            b'0' | b'\0' => {
                if !extract_regular_file(input.as_mut(), &name, size, &mut status) {
                    break;
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
                    quotef_os(&name),
                    quoteaf(&[other])
                );
                status = EXIT_FATAL;
                if !skip_data(input.as_mut(), size) {
                    break;
                }
            }
        }
    }
    status
}

/// Stream one regular member out of `input` into `name`. Returns false when
/// the archive ended mid-member, which means the outer loop must stop.
///
/// This streams rather than buffering. The previous version did
/// `Vec::with_capacity(size)` from the header's own size field, so an archive
/// whose header claimed 2^40 bytes made this program try to reserve a
/// terabyte before reading a single block — a one-line denial of service
/// costing the attacker 512 bytes of file.
fn extract_regular_file(input: &mut dyn Read, name: &str, size: u64, status: &mut i32) -> bool {
    if let Some(parent) = Path::new(name).parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = fs::create_dir_all(parent)
    {
        diag!("tar: {}: {e}", quotef_os(parent));
        *status = EXIT_FATAL;
        return skip_data(input, size);
    }

    let mut file = match File::create(name) {
        Ok(f) => Some(f),
        Err(e) => {
            // Still consume the data: the archive may hold members after this
            // one, and abandoning the stream would lose them too.
            diag!("tar: {}: {e}", quotef_os(name));
            *status = EXIT_FATAL;
            None
        }
    };

    let mut remaining = size;
    let mut block = [0u8; BLOCK_SIZE];
    for _ in 0..data_blocks(size) {
        if input.read_exact(&mut block).is_err() {
            diag!("tar: {}: unexpected end of archive", quotef_os(name));
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
            diag!("tar: {}: {e}", quotef_os(name));
            *status = EXIT_FATAL;
            // Drop the handle so the rest of the member is only skipped, but
            // keep reading so the following headers stay aligned.
            file = None;
        }
    }
    // Buffered data is not the issue here (`File` is unbuffered), but a
    // filesystem that reports a write error at close would otherwise be
    // ignored, which is the same defect as the discarded `write_all` above.
    if let Some(mut f) = file
        && let Err(e) = f.flush()
    {
        diag!("tar: {}: {e}", quotef_os(name));
        *status = EXIT_FATAL;
    }
    true
}

fn do_list_main(archive_file: &Option<String>) -> i32 {
    let input: Box<dyn Read> = match archive_file {
        Some(path) => match File::open(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                diag!("tar: {}: {e}", quotef_os(path));
                return EXIT_FATAL;
            }
        },
        None => Box::new(io::stdin()),
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();
    if let Err(e) = list_archive(input, &mut out).and_then(|()| out.flush()) {
        // `tar -tf big.tar | head -5` closes the pipe on purpose; that is how
        // a pipeline ends, not a failure of this program.
        if e.kind() == io::ErrorKind::BrokenPipe {
            return 0;
        }
        diag!("tar: 'standard output': {e}");
        return EXIT_FATAL;
    }
    0
}

/// List the names of all archive members read from `input` to `out`.
/// Pure I/O over `Read`/`Write` so it can be tested with in-memory
/// buffers and synthetic tar blocks.  Returns an io::Result so test
/// callers can verify error propagation, though in practice malformed
/// data just causes a clean stop at the first short read.
fn list_archive(mut input: impl Read, out: &mut impl Write) -> io::Result<()> {
    loop {
        let mut header_buf = [0u8; BLOCK_SIZE];
        if input.read_exact(&mut header_buf).is_err() {
            break;
        }

        if header_buf.iter().all(|&b| b == 0) {
            break;
        }

        let name = extract_string(header_buf.get(..100).unwrap_or(&[]));
        let size = parse_octal(header_buf.get(124..136).unwrap_or(&[]));

        if name.is_empty() {
            break;
        }

        // Listing shows the name as stored, not the sanitized one: the point
        // of `tar -t` is to tell you what is in the archive, and a member
        // called `../../etc/passwd` is exactly what you want to be shown.
        writeln!(out, "{name}")?;

        for _ in 0..data_blocks(size) {
            let mut block = [0u8; BLOCK_SIZE];
            if input.read_exact(&mut block).is_err() {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Decode a NUL-terminated string out of a fixed-size header field.
fn extract_string(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(buf.get(..end).unwrap_or(&[])).to_string()
}

/// Parse a NUL/space-padded octal field into a `u64`.  Non-octal input
/// silently parses as 0 (matching common tar implementations on
/// malformed archives).
fn parse_octal(buf: &[u8]) -> u64 {
    let s = extract_string(buf);
    u64::from_str_radix(s.trim(), 8).unwrap_or(0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    /// Build a single tar header block with the given name and size.
    fn make_header(name: &str, size: u64, typeflag: u8) -> [u8; BLOCK_SIZE] {
        let mut h = TarHeader::new();
        h.set_name(name);
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
        assert_eq!(a.archive_file.as_deref(), Some("out.tar"));
        assert_eq!(a.files, vec!["a", "b"]);
    }

    #[test]
    fn parse_clustered_create_verbose_file() {
        // -cvf out.tar a -- the f consumes the next argv element.
        let a = parse_args(&s(&["-cvf", "out.tar", "a"])).unwrap();
        assert!(a.create);
        assert!(a.verbose);
        assert_eq!(a.archive_file.as_deref(), Some("out.tar"));
        assert_eq!(a.files, vec!["a"]);
    }

    #[test]
    fn parse_extract_with_directory() {
        let a = parse_args(&s(&["-x", "-C", "/tmp", "-f", "in.tar"])).unwrap();
        assert!(a.extract);
        assert_eq!(a.directory.as_deref(), Some("/tmp"));
        assert_eq!(a.archive_file.as_deref(), Some("in.tar"));
    }

    #[test]
    fn parse_list() {
        let a = parse_args(&s(&["-tf", "x.tar"])).unwrap();
        assert!(a.list);
        assert_eq!(a.archive_file.as_deref(), Some("x.tar"));
    }

    #[test]
    fn parse_unknown_flag_errors() {
        let err = parse_args(&s(&["-Z"])).unwrap_err();
        assert!(err.contains("unknown option"));
        assert!(err.contains("-Z"));
    }

    #[test]
    fn parse_missing_f_value_errors() {
        let err = parse_args(&s(&["-f"])).unwrap_err();
        assert!(err.contains("-f requires"));
    }

    #[test]
    fn parse_missing_c_value_errors() {
        let err = parse_args(&s(&["-C"])).unwrap_err();
        assert!(err.contains("-C requires"));
    }

    #[test]
    fn parse_files_with_dashes_handled() {
        // Bare positional arg starting with non-dash is a file.
        let a = parse_args(&s(&["-c", "f1", "f2"])).unwrap();
        assert!(a.create);
        assert_eq!(a.files, vec!["f1", "f2"]);
    }

    // ---------------- sanitize_member_name ----------------
    //
    // Every case here was a file written outside the destination directory
    // before the sanitizer existed. See known-issues.md ->
    // B-tar-EXTRACTS-OUTSIDE-THE-DESTINATION-DIRECTORY.

    #[test]
    fn sanitize_plain_relative_name_unchanged() {
        assert_eq!(sanitize_member_name("a/b/c.txt").unwrap(), "a/b/c.txt");
        assert_eq!(sanitize_member_name("file.txt").unwrap(), "file.txt");
    }

    #[test]
    fn sanitize_strips_leading_slash() {
        // Critical: `Path::join` with an absolute path throws the base away,
        // so an unstripped name would ignore `-C` entirely.
        assert_eq!(sanitize_member_name("/etc/passwd").unwrap(), "etc/passwd");
        assert_eq!(sanitize_member_name("///etc/passwd").unwrap(), "etc/passwd");
    }

    #[test]
    fn sanitize_drops_dot_and_doubled_slashes() {
        assert_eq!(sanitize_member_name("./a//b/./c").unwrap(), "a/b/c");
    }

    #[test]
    fn sanitize_refuses_leading_dotdot() {
        assert!(sanitize_member_name("../../etc/passwd").is_err());
    }

    #[test]
    fn sanitize_refuses_interior_dotdot() {
        // `a/../b` is only equivalent to `b` if `a` is a real directory and
        // not a symlink -- which the archive is the last thing to trust about.
        assert!(sanitize_member_name("a/../../etc/passwd").is_err());
        assert!(sanitize_member_name("a/../b").is_err());
    }

    #[test]
    fn sanitize_refuses_absolute_dotdot_combination() {
        assert!(sanitize_member_name("/../../root/.ssh/authorized_keys").is_err());
    }

    #[test]
    fn sanitize_refuses_backslash_traversal() {
        // Not a separator on this OS, but this file's tests -- and any host
        // build -- run where it is.
        assert!(sanitize_member_name("..\\..\\windows\\system32\\x").is_err());
        assert!(sanitize_member_name("a/..\\b").is_err());
    }

    #[test]
    fn sanitize_keeps_a_lone_backslash_in_a_name() {
        // A slateos filename may contain a backslash; only a `..` component
        // is refused, and the name is rebuilt with `/` alone.
        assert_eq!(sanitize_member_name("a/b\\c").unwrap(), "a/b\\c");
    }

    #[test]
    fn sanitize_refuses_names_that_reduce_to_nothing() {
        assert!(sanitize_member_name("").is_err());
        assert!(sanitize_member_name("/").is_err());
        assert!(sanitize_member_name("./.").is_err());
    }

    #[test]
    fn sanitize_allows_dotfiles_and_names_starting_with_dots() {
        // `..foo` and `...` are ordinary names, not traversal.
        assert_eq!(sanitize_member_name(".bashrc").unwrap(), ".bashrc");
        assert_eq!(sanitize_member_name("a/..foo").unwrap(), "a/..foo");
        assert_eq!(sanitize_member_name("a/...").unwrap(), "a/...");
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

    // ---------------- extract_string / parse_octal ----------------

    #[test]
    fn extract_string_stops_at_nul() {
        let buf = b"hello\0\0\0world";
        assert_eq!(extract_string(buf), "hello");
    }

    #[test]
    fn extract_string_no_nul_uses_all() {
        let buf = b"hello";
        assert_eq!(extract_string(buf), "hello");
    }

    #[test]
    fn extract_string_empty() {
        assert_eq!(extract_string(&[]), "");
        assert_eq!(extract_string(&[0u8; 8]), "");
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
        h1.set_name("foo");
        TarHeader::set_octal(&mut h1.mode, 0o644);
        h1.compute_checksum();

        let mut h2 = TarHeader::new();
        h2.set_name("foo");
        TarHeader::set_octal(&mut h2.mode, 0o644);
        h2.compute_checksum();

        assert_eq!(h1.checksum, h2.checksum);
    }

    #[test]
    fn checksum_changes_with_name() {
        let mut h1 = TarHeader::new();
        h1.set_name("foo");
        h1.compute_checksum();

        let mut h2 = TarHeader::new();
        h2.set_name("bar");
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
        list_archive(input.as_slice(), &mut out).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn list_single_zero_byte_file() {
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header("hello.txt", 0, b'0'));
        // No data blocks (size = 0).
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        let mut out = Vec::new();
        list_archive(input.as_slice(), &mut out).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "hello.txt\n");
    }

    #[test]
    fn list_single_file_with_data() {
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header("data.bin", 100, b'0'));
        // 100-byte file occupies 1 data block.
        input.extend_from_slice(&[b'x'; BLOCK_SIZE]);
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        let mut out = Vec::new();
        list_archive(input.as_slice(), &mut out).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "data.bin\n");
    }

    #[test]
    fn list_multiple_files() {
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header("a.txt", 0, b'0'));
        input.extend_from_slice(&make_header("b.txt", 600, b'0'));
        // 600-byte file = ceil(600/512) = 2 data blocks.
        input.extend_from_slice(&[b'y'; BLOCK_SIZE]);
        input.extend_from_slice(&[b'y'; BLOCK_SIZE]);
        input.extend_from_slice(&make_header("c.txt", 0, b'0'));
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        input.extend_from_slice(&[0u8; BLOCK_SIZE]);
        let mut out = Vec::new();
        list_archive(input.as_slice(), &mut out).unwrap();
        let listing = String::from_utf8(out).unwrap();
        assert_eq!(listing, "a.txt\nb.txt\nc.txt\n");
    }

    #[test]
    fn list_truncated_input_does_not_panic() {
        // Header announces a 1024-byte file but no data follows: must
        // exit cleanly, not loop or panic.
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(&make_header("liar.bin", 1024, b'0'));
        let mut out = Vec::new();
        list_archive(input.as_slice(), &mut out).unwrap();
        // We still recorded the name before discovering the truncation.
        assert_eq!(String::from_utf8(out).unwrap(), "liar.bin\n");
    }

    #[test]
    fn list_short_header_stops_cleanly() {
        // Less than one full header block: list_archive should bail
        // immediately without writing anything.
        let input = vec![0u8; 100];
        let mut out = Vec::new();
        list_archive(input.as_slice(), &mut out).unwrap();
        assert!(out.is_empty());
    }
}
