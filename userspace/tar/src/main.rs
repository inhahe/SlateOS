//! Slate OS Tape Archive Utility
//!
//! Creates, extracts, and lists POSIX ustar tar archives.
//!
//! # Usage
//!
//! ```text
//! tar -c -f archive.tar file1 file2 dir/    Create archive
//! tar -x -f archive.tar                     Extract archive
//! tar -t -f archive.tar                     List archive contents
//! tar -x -f archive.tar -C /tmp             Extract to specific directory
//! tar -c -f archive.tar --exclude '*.o' .   Create with exclusions
//! tar -t -v -f archive.tar                  Detailed listing
//! tar -t --json -f archive.tar              JSON listing
//! ```
//!
//! # Format
//!
//! Implements the POSIX ustar format with 512-byte block headers and data
//! padded to 512-byte boundaries. Archives are terminated by two consecutive
//! zero blocks.

use quoting::{escape_os, quoteaf};
use std::env;
use std::ffi::OsStr;
use std::fs::{self, File, Metadata};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process;

// ============================================================================
// Constants
// ============================================================================

/// Size of a single tar block.
const BLOCK_SIZE: usize = 512;

/// ustar magic value (6 bytes including NUL).
const USTAR_MAGIC: &[u8; 6] = b"ustar\0";

/// ustar version field.
const USTAR_VERSION: &[u8; 2] = b"00";

// Type flag constants.
const TYPEFLAG_REGULAR: u8 = b'0';
const TYPEFLAG_DIRECTORY: u8 = b'5';
const TYPEFLAG_SYMLINK: u8 = b'2';

// ============================================================================
// Tar header (512 bytes, ustar format)
// ============================================================================

/// POSIX ustar tar header. Laid out at exactly 512 bytes matching the on-disk
/// format so we can transmute safely between `[u8; 512]` and this struct.
#[repr(C)]
struct TarHeader {
    /// File name (NUL-terminated if shorter than 100).
    name: [u8; 100],
    /// File mode in octal ASCII.
    mode: [u8; 8],
    /// Owner user ID in octal ASCII.
    uid: [u8; 8],
    /// Owner group ID in octal ASCII.
    gid: [u8; 8],
    /// File size in octal ASCII.
    size: [u8; 12],
    /// Modification time (seconds since epoch) in octal ASCII.
    mtime: [u8; 12],
    /// Header checksum in octal ASCII.
    checksum: [u8; 8],
    /// Type flag byte.
    typeflag: u8,
    /// Linked file name for symlinks.
    linkname: [u8; 100],
    /// Must be `"ustar\0"` for ustar archives.
    magic: [u8; 6],
    /// Must be `"00"`.
    version: [u8; 2],
    /// Owner user name.
    uname: [u8; 32],
    /// Owner group name.
    gname: [u8; 32],
    /// Device major number (octal ASCII).
    devmajor: [u8; 8],
    /// Device minor number (octal ASCII).
    devminor: [u8; 8],
    /// Filename prefix for paths longer than 100 bytes.
    prefix: [u8; 155],
    /// Padding to reach 512 bytes.
    _pad: [u8; 12],
}

// Compile-time guarantee: header is exactly one block.
const _: () = assert!(size_of::<TarHeader>() == BLOCK_SIZE);

// ============================================================================
// Parsed entry (for listing / extraction)
// ============================================================================

/// A decoded tar entry with owned strings, ready for display or extraction.
struct TarEntry {
    path: String,
    mode: u32,
    uid: u64,
    gid: u64,
    size: u64,
    mtime: u64,
    typeflag: u8,
    linkname: String,
    uname: String,
    gname: String,
}

// ============================================================================
// CLI options
// ============================================================================

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Mode {
    Create,
    Extract,
    List,
}

/// One item from the operand stream, kept in the order it appeared.
///
/// **`-C` cannot be stored as a setting, because it is not one.** It was an
/// `Option<String>` here, last-one-wins, applied once before anything began.
/// GNU applies it *positionally*: it takes effect where it appears and governs
/// the operands that follow it, so `-C one a -C ../two b` archives `one/a` and
/// `two/b` under the member names `a` and `b`. Collapsing that to a single
/// value silently archived the wrong files — measured against GNU tar 1.35,
/// which is also where the two properties below come from.
///
/// Two consequences worth stating, because both are easy to get backwards:
///
/// * **The chdirs are cumulative**, each relative to wherever the previous ones
///   left us, not to the original directory. GNU refuses `-C one a -C two b`
///   with "two: Cannot open" precisely because `two` is not inside `one`.
/// * **The archive path is *not* subject to them.** `-C one -cf ../o.tar a`
///   writes `../o.tar` relative to the original directory. That is why every
///   mode opens its archive before it walks this list, and it is observable:
///   GNU leaves an empty archive behind when a later `-C` fails.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Operand {
    /// `-C dir` / `--directory=dir`.
    Chdir(String),
    /// A file to archive (create), or a member name (extract/list).
    Member(String),
}

struct Options {
    mode: Mode,
    archive: String,
    verbose: bool,
    /// `-C` directives and file operands interleaved, in command-line order.
    operands: Vec<Operand>,
    excludes: Vec<String>,
    preserve_permissions: bool,
    strip_components: usize,
    keep_old_files: bool,
    json: bool,
    // Ownership setting not yet implemented (requires chown support).
    _no_same_owner: bool,
    // Compression flags acknowledged but not implemented.
    _gzip: bool,
    _bzip2: bool,
}

// ============================================================================
// Argument parsing
// ============================================================================

fn parse_args() -> Result<Options, String> {
    let args: Vec<String> = env::args().collect();

    let mut mode: Option<Mode> = None;
    let mut archive: Option<String> = None;
    let mut verbose = false;
    let mut operands: Vec<Operand> = Vec::new();
    let mut excludes: Vec<String> = Vec::new();
    let mut preserve_permissions = false;
    let mut strip_components: usize = 0;
    let mut keep_old_files = false;
    let mut no_same_owner = false;
    let mut json = false;
    let mut gzip = false;
    let mut bzip2 = false;

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-c" | "--create" => mode = Some(Mode::Create),
            "-x" | "--extract" | "--get" => mode = Some(Mode::Extract),
            "-t" | "--list" => mode = Some(Mode::List),
            "-v" | "--verbose" => verbose = true,
            "-f" => {
                i += 1;
                if i >= args.len() {
                    return Err("-f requires an argument".to_string());
                }
                archive = Some(args[i].clone());
            }
            "-p" | "--preserve-permissions" => preserve_permissions = true,
            "-k" | "--keep-old-files" => keep_old_files = true,
            "-j" | "--bzip2" => {
                bzip2 = true;
                eprintln!("tar: warning: bzip2 compression not implemented, flag acknowledged");
            }
            "-z" | "--gzip" | "--gunzip" => {
                gzip = true;
                eprintln!("tar: warning: gzip compression not implemented, flag acknowledged");
            }
            "--json" => json = true,
            "--no-same-owner" => no_same_owner = true,
            "--exclude" => {
                i += 1;
                if i >= args.len() {
                    return Err("--exclude requires an argument".to_string());
                }
                excludes.push(args[i].clone());
            }
            "-C" | "--directory" => {
                i += 1;
                if i >= args.len() {
                    return Err("-C/--directory requires an argument".to_string());
                }
                operands.push(Operand::Chdir(args[i].clone()));
            }
            other => {
                if let Some(rest) = other.strip_prefix("--strip-components=") {
                    strip_components = rest.parse::<usize>().map_err(|e| {
                        format!("--strip-components: invalid number '{}': {}", rest, e)
                    })?;
                } else if let Some(rest) = other.strip_prefix("--exclude=") {
                    excludes.push(rest.to_string());
                } else if let Some(rest) = other.strip_prefix("--directory=") {
                    operands.push(Operand::Chdir(rest.to_string()));
                } else if let Some(rest) = other.strip_prefix("-f") {
                    // Combined form: -farchive.tar
                    if rest.is_empty() {
                        return Err("-f requires an argument".to_string());
                    }
                    archive = Some(rest.to_string());
                } else if other.starts_with('-') && !other.starts_with("--") && other.len() > 2 {
                    // Bundled short flags like -cvf or -xvf.
                    // The last character might consume the next arg if it's 'f'.
                    let chars: Vec<char> = other[1..].chars().collect();
                    for (ci, &ch) in chars.iter().enumerate() {
                        match ch {
                            'c' => mode = Some(Mode::Create),
                            'x' => mode = Some(Mode::Extract),
                            't' => mode = Some(Mode::List),
                            'v' => verbose = true,
                            'p' => preserve_permissions = true,
                            'k' => keep_old_files = true,
                            'j' => {
                                bzip2 = true;
                                eprintln!(
                                    "tar: warning: bzip2 compression not implemented, flag acknowledged"
                                );
                            }
                            'z' => {
                                gzip = true;
                                eprintln!(
                                    "tar: warning: gzip compression not implemented, flag acknowledged"
                                );
                            }
                            'f' => {
                                // 'f' consumes the rest of the bundled string,
                                // or the next argument if at end.
                                let remainder: String = chars[ci + 1..].iter().collect();
                                if !remainder.is_empty() {
                                    archive = Some(remainder);
                                } else {
                                    i += 1;
                                    if i >= args.len() {
                                        return Err("-f requires an argument".to_string());
                                    }
                                    archive = Some(args[i].clone());
                                }
                                break;
                            }
                            _ => {
                                return Err(format!("unknown option: -{}", ch));
                            }
                        }
                    }
                } else if other.starts_with('-') {
                    return Err(format!("unknown option: {}", other));
                } else {
                    operands.push(Operand::Member(other.to_string()));
                }
            }
        }
        i += 1;
    }

    let mode = mode.ok_or_else(|| {
        "no mode specified (use -c to create, -x to extract, -t to list)".to_string()
    })?;
    let archive = archive.ok_or_else(|| "no archive file specified (use -f <file>)".to_string())?;

    if mode == Mode::Create && !operands.iter().any(|o| matches!(o, Operand::Member(_))) {
        // GNU's own wording for this, including the case where the only
        // operands are `-C` directives (`tar -cf o.tar -C one`).
        return Err("Cowardly refusing to create an empty archive".to_string());
    }

    Ok(Options {
        mode,
        archive,
        verbose,
        operands,
        excludes,
        preserve_permissions,
        strip_components,
        keep_old_files,
        _no_same_owner: no_same_owner,
        json,
        _gzip: gzip,
        _bzip2: bzip2,
    })
}

// ============================================================================
// Octal encoding / decoding helpers
// ============================================================================

/// Encode `val` as a NUL-terminated octal ASCII string into `buf`.
/// Returns an error if the value does not fit in `buf.len() - 1` octal digits.
fn encode_octal(buf: &mut [u8], val: u64) -> Result<(), String> {
    if buf.is_empty() {
        return Err("octal buffer too small".to_string());
    }
    // We need at most buf.len()-1 digits plus NUL terminator.
    let width = buf.len() - 1;
    let s = format!("{:0>width$o}", val, width = width);
    if s.len() > width {
        return Err(format!(
            "value {} too large for {}-digit octal field",
            val, width
        ));
    }
    buf[..width].copy_from_slice(s.as_bytes());
    buf[width] = 0;
    Ok(())
}

/// Decode a NUL- or space-terminated octal ASCII string from `buf`.
fn decode_octal(buf: &[u8]) -> u64 {
    let mut val: u64 = 0;
    for &b in buf {
        if b == 0 || b == b' ' {
            break;
        }
        if (b'0'..=b'7').contains(&b) {
            val = val.saturating_mul(8).saturating_add(u64::from(b - b'0'));
        }
    }
    val
}

// ============================================================================
// String helpers for header fields
// ============================================================================

/// Copy a string into a fixed-size byte buffer, NUL-terminated if shorter.
fn copy_str_to_field(field: &mut [u8], s: &str) {
    let bytes = s.as_bytes();
    let len = bytes.len().min(field.len());
    field[..len].copy_from_slice(&bytes[..len]);
    // Zero the rest.
    for b in &mut field[len..] {
        *b = 0;
    }
}

/// Read a NUL-terminated string from a header field.
fn field_to_string(field: &[u8]) -> String {
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    String::from_utf8_lossy(&field[..end]).into_owned()
}

// ============================================================================
// Glob pattern matching (simple)
// ============================================================================

/// Match a simple glob pattern against a string.
/// Supports `*` (any sequence of chars) and `?` (any single char).
fn glob_matches(pattern: &str, text: &str) -> bool {
    glob_match_inner(pattern.as_bytes(), text.as_bytes())
}

fn glob_match_inner(pattern: &[u8], text: &[u8]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_pi: Option<usize> = None;
    let mut star_ti: usize = 0;

    while ti < text.len() {
        if pi < pattern.len() && (pattern[pi] == b'?' || pattern[pi] == text[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pattern.len() && pattern[pi] == b'*' {
            star_pi = Some(pi);
            star_ti = ti;
            pi += 1;
        } else if let Some(sp) = star_pi {
            pi = sp + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }

    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }

    pi == pattern.len()
}

/// Check if a path should be excluded based on the exclude patterns.
/// Matches the pattern against each component of the path as well as the full
/// path string.
fn is_excluded(path: &str, excludes: &[String]) -> bool {
    for pattern in excludes {
        if glob_matches(pattern, path) {
            return true;
        }
        // Also match against just the filename component.
        if let Some(fname) = Path::new(path).file_name()
            && let Some(fname_str) = fname.to_str()
            && glob_matches(pattern, fname_str)
        {
            return true;
        }
    }
    false
}

// ============================================================================
// Path splitting for ustar prefix/name
// ============================================================================

/// Split a path into (prefix, name) for the ustar header. The name field
/// holds up to 100 bytes and the prefix field up to 155 bytes. The full path
/// is reconstructed as `prefix/name` if prefix is non-empty.
///
/// Returns an error if the path cannot fit in the combined fields.
fn split_path(path: &str) -> Result<(String, String), String> {
    if path.len() <= 100 {
        return Ok((String::new(), path.to_string()));
    }

    // Try to find a '/' split point such that the part after the split fits
    // in 100 bytes and the part before fits in 155.
    for (i, _) in path.char_indices().rev() {
        if path.as_bytes().get(i) == Some(&b'/') {
            let prefix = &path[..i];
            let name = &path[i + 1..];
            if prefix.len() <= 155 && name.len() <= 100 {
                return Ok((prefix.to_string(), name.to_string()));
            }
        }
    }

    Err(format!(
        "path too long for ustar format (max 256 chars): {}",
        path
    ))
}

// ============================================================================
// Header checksum
// ============================================================================

/// Compute the ustar header checksum. The checksum field itself is treated
/// as eight space (0x20) bytes during computation.
fn compute_checksum(header_bytes: &[u8; BLOCK_SIZE]) -> u32 {
    let mut sum: u32 = 0;
    for (i, &b) in header_bytes.iter().enumerate() {
        // The checksum field occupies bytes 148..156.
        if (148..156).contains(&i) {
            sum += 0x20_u32;
        } else {
            sum += u32::from(b);
        }
    }
    sum
}

// ============================================================================
// Metadata helpers
// ============================================================================

/// Extract the Unix mode bits from file metadata. On our OS this maps to
/// the standard permission bits (owner/group/other read/write/execute).
#[cfg(unix)]
fn get_mode(meta: &Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    meta.mode()
}

#[cfg(not(unix))]
fn get_mode(meta: &Metadata) -> u32 {
    if meta.is_dir() {
        0o755
    } else if meta.permissions().readonly() {
        0o444
    } else {
        0o644
    }
}

/// Extract the modification time as seconds since the Unix epoch.
fn get_mtime(meta: &Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Get the file size (0 for directories).
fn get_size(meta: &Metadata) -> u64 {
    if meta.is_dir() { 0 } else { meta.len() }
}

// ============================================================================
// Strip path components
// ============================================================================

/// Reduce an archive member name to a path that cannot escape the destination
/// directory, or refuse it.
///
/// **The member names in a tar file are chosen by whoever made it.** Extraction
/// here used to do `base_dir.join(&entry.path)` with the name exactly as
/// stored, so an archive containing `../../etc/passwd` wrote there, and one
/// containing `/etc/shadow` wrote there too — `Path::join` with an absolute
/// right-hand side *discards* the base, so `-C` was no protection at all. That
/// is the "tar slip" vulnerability class. See `known-issues.md` →
/// `B-tar-EXTRACTS-OUTSIDE-THE-DESTINATION-DIRECTORY`, and keep this function
/// with whichever of the two `tar` implementations survives **B-Q7** — it is a
/// security boundary, not a convenience like `--strip-components`.
///
/// The two escapes are handled differently on purpose:
///
/// * **A leading `/` is stripped**, as GNU does ("Removing leading `/' from
///   member names"). Archives of system trees are routinely absolute and are
///   safe to unpack somewhere else, so refusing them would break a common case
///   for no gain.
/// * **A `..` component is refused** and the member skipped. It cannot be
///   stripped safely: `a/../b` equals `b` only if `a` is a real directory
///   rather than a symlink, and the archive is exactly the thing we will not
///   trust about that.
///
/// `.` components and doubled slashes are dropped. The `..` test also splits on
/// `\`, which is not a separator in this OS but is on the hosts these tests run
/// on; names are rebuilt with `/` alone so a filename legitimately containing a
/// backslash survives.
///
/// This runs *after* `--strip-components`, so a stripped name is re-checked
/// rather than trusted — stripping can expose a `..` that was not leading.
fn sanitize_member_name(raw: &str) -> Result<String, String> {
    let mut parts: Vec<&str> = Vec::new();
    for component in raw.split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." || component.split('\\').any(|p| p == "..") {
            return Err(format!(
                "refusing to extract '{}': member name escapes the destination directory",
                raw
            ));
        }
        parts.push(component);
    }
    if parts.is_empty() {
        return Err(format!("refusing to extract '{}': empty member name", raw));
    }
    Ok(parts.join("/"))
}

/// Remove the first `n` path components from a path string.
/// Returns `None` if stripping removes all components.
fn strip_components(path: &str, n: usize) -> Option<String> {
    if n == 0 {
        return Some(path.to_string());
    }
    let parts: Vec<&str> = path.split('/').collect();
    if n >= parts.len() {
        return None;
    }
    Some(parts[n..].join("/"))
}

// ============================================================================
// Permission string for verbose listing
// ============================================================================

/// Format a Unix mode into an `ls -l`-style permission string like `drwxr-xr-x`.
fn format_permissions(mode: u32, typeflag: u8) -> String {
    let mut perms = [b'-'; 10];

    perms[0] = match typeflag {
        TYPEFLAG_DIRECTORY => b'd',
        TYPEFLAG_SYMLINK => b'l',
        _ => b'-',
    };

    if mode & 0o400 != 0 {
        perms[1] = b'r';
    }
    if mode & 0o200 != 0 {
        perms[2] = b'w';
    }
    if mode & 0o100 != 0 {
        perms[3] = b'x';
    }
    if mode & 0o040 != 0 {
        perms[4] = b'r';
    }
    if mode & 0o020 != 0 {
        perms[5] = b'w';
    }
    if mode & 0o010 != 0 {
        perms[6] = b'x';
    }
    if mode & 0o004 != 0 {
        perms[7] = b'r';
    }
    if mode & 0o002 != 0 {
        perms[8] = b'w';
    }
    if mode & 0o001 != 0 {
        perms[9] = b'x';
    }

    // SAFETY: all bytes are valid ASCII.
    String::from_utf8(perms.to_vec()).unwrap_or_else(|_| "----------".to_string())
}

/// Format a Unix timestamp into a human-readable date string.
/// Simple implementation: YYYY-MM-DD HH:MM.
fn format_timestamp(epoch_secs: u64) -> String {
    // Days in each month for non-leap and leap years.
    const DAYS_IN_MONTH: [[u64; 12]; 2] = [
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31],
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31],
    ];

    fn is_leap(y: u64) -> bool {
        (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
    }

    let mut remaining = epoch_secs;
    let secs = remaining % 60;
    remaining /= 60;
    let mins = remaining % 60;
    remaining /= 60;
    let hours = remaining % 24;
    let mut days = remaining / 24;

    let mut year: u64 = 1970;
    loop {
        let days_in_year: u64 = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let leap_idx = if is_leap(year) { 1 } else { 0 };
    let mut month: u64 = 0;
    while month < 12 && days >= DAYS_IN_MONTH[leap_idx][month as usize] {
        days -= DAYS_IN_MONTH[leap_idx][month as usize];
        month += 1;
    }
    let day = days + 1;
    month += 1;

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        year, month, day, hours, mins, secs
    )
}

// ============================================================================
// CREATE mode
// ============================================================================

/// [`encode_octal`] with the `tar: ` prefix its message needs to be printable.
fn octal_field(buf: &mut [u8], val: u64) -> Result<(), String> {
    encode_octal(buf, val).map_err(|e| format!("tar: {}", e))
}

/// Write a single file or directory entry (header + data) to the archive.
///
/// `archive` is the name `-f` was given, needed only to report a write failure
/// the way GNU does — `tar: /dev/full: Cannot write: No space left on device`
/// names the *archive*, not the member being written into it, and is fatal
/// rather than accumulated. Every other failure here concerns `full_path` and
/// is returned for the caller to accumulate.
///
/// The returned string is a complete diagnostic line, `tar: ` prefix included.
/// It has to be: some failures name the member, some (a name too long for the
/// ustar fields) name a path that is neither the member nor its argument, and a
/// caller that added the prefix and a name of its own would double up on the
/// first kind.
fn write_entry<W: Write>(
    writer: &mut W,
    archive: &str,
    rel_path: &str,
    full_path: &Path,
    meta: &Metadata,
) -> Result<(), String> {
    let mut header = [0u8; BLOCK_SIZE];

    // Determine path fields.
    let archive_path = if meta.is_dir() && !rel_path.ends_with('/') {
        format!("{}/", rel_path)
    } else {
        rel_path.to_string()
    };

    // These two carry their own context — `split_path` names the over-long
    // path, `encode_octal` names the value and the field width — so they get
    // the `tar: ` prefix and nothing else.
    let (prefix, name) = split_path(&archive_path).map_err(|e| format!("tar: {}", e))?;

    // Fill header fields.
    copy_str_to_field(&mut header[..100], &name);
    octal_field(&mut header[100..108], u64::from(get_mode(meta)))?;
    octal_field(&mut header[108..116], 0)?; // uid
    octal_field(&mut header[116..124], 0)?; // gid
    octal_field(&mut header[124..136], get_size(meta))?;
    octal_field(&mut header[136..148], get_mtime(meta))?;
    // Checksum placeholder: 8 spaces.
    header[148..156].copy_from_slice(b"        ");

    if meta.is_dir() {
        header[156] = TYPEFLAG_DIRECTORY;
    } else {
        header[156] = TYPEFLAG_REGULAR;
    }

    // Symlink target: leave empty for now (symlink support is minimal).
    // linkname: header[157..257] already zeroed.

    // Magic and version.
    header[257..263].copy_from_slice(USTAR_MAGIC);
    header[263..265].copy_from_slice(USTAR_VERSION);

    // uname/gname: leave as empty for now.
    // devmajor/devminor: leave as zeroed.

    // Prefix.
    copy_str_to_field(&mut header[345..500], &prefix);

    // Compute and write checksum.
    let cksum = compute_checksum(
        // SAFETY: `header` is exactly BLOCK_SIZE (512) bytes, matching the
        // expected array size.
        <&[u8; BLOCK_SIZE]>::try_from(header.as_slice())
            .map_err(|_| "internal error: header size mismatch".to_string())?,
    );
    let cksum_str = format!("{:06o}\0 ", cksum);
    let cksum_bytes = cksum_str.as_bytes();
    let copy_len = cksum_bytes.len().min(8);
    header[148..148 + copy_len].copy_from_slice(&cksum_bytes[..copy_len]);

    // Write header. A failed archive write is not an error about this member —
    // it is the archive going away underneath the whole run — so it is fatal
    // and names the archive, which is what GNU does: writing to /dev/full gives
    // "tar: /dev/full: Cannot write: No space left on device" followed by
    // "Error is not recoverable: exiting now", and no per-member line at all.
    if let Err(e) = writer.write_all(&header) {
        fail_cannot(archive, "write", &e);
    }

    // Write file data if it is a regular file with content.
    if meta.is_file() {
        let size = meta.len();
        if size > 0 {
            let mut file = File::open(full_path).map_err(|e| cannot(full_path, "open", &e))?;
            let mut remaining = size;
            let mut buf = [0u8; 8192];
            while remaining > 0 {
                let to_read = (remaining as usize).min(buf.len());
                let n = file
                    .read(&mut buf[..to_read])
                    .map_err(|e| cannot(full_path, "read", &e))?;
                if n == 0 {
                    break;
                }
                if let Err(e) = writer.write_all(&buf[..n]) {
                    fail_cannot(archive, "write", &e);
                }
                remaining = remaining.saturating_sub(n as u64);
            }

            // Pad to 512-byte boundary.
            let pad_len = (BLOCK_SIZE - (size as usize % BLOCK_SIZE)) % BLOCK_SIZE;
            if pad_len > 0 {
                let zeros = [0u8; BLOCK_SIZE];
                if let Err(e) = writer.write_all(&zeros[..pad_len]) {
                    fail_cannot(archive, "write", &e);
                }
            }
        }
    }

    Ok(())
}

/// Recursively collect all files under `base_path` with paths relative to
/// `prefix`, writing each entry to the archive.
fn archive_path_recursive<W: Write>(
    writer: &mut W,
    archive: &str,
    base_path: &Path,
    prefix: &str,
    excludes: &[String],
    verbose: bool,
    errors: &mut Vec<String>,
) {
    let rel = if prefix.is_empty() {
        base_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(".")
            .to_string()
    } else {
        format!(
            "{}/{}",
            prefix,
            base_path.file_name().and_then(|n| n.to_str()).unwrap_or("")
        )
    };

    if is_excluded(&rel, excludes) {
        return;
    }

    // "Cannot stat", measured: a directory readable but not searchable makes
    // GNU say `tar: src/dir/b.txt: Cannot stat: Permission denied` for each
    // child and carry on, exiting 2 at the end.
    let meta = match fs::metadata(base_path) {
        Ok(m) => m,
        Err(e) => {
            report(cannot(base_path, "stat", &e), errors);
            return;
        }
    };

    if meta.is_file() || meta.is_dir() {
        if verbose {
            eprintln!("{}", rel);
        }
        if let Err(msg) = write_entry(writer, archive, &rel, base_path, &meta) {
            report(msg, errors);
        }
    }

    if meta.is_dir() {
        // Also measured: an unreadable directory is `Cannot open`, not
        // `Cannot read` and not `Cannot opendir` — GNU routes both a failed
        // `open` and a failed directory scan through the same verb.
        let entries = match fs::read_dir(base_path) {
            Ok(rd) => rd,
            Err(e) => {
                report(cannot(base_path, "open", &e), errors);
                return;
            }
        };

        // Collect and sort entries for deterministic archives.
        let mut children: Vec<PathBuf> = Vec::new();
        for entry in entries {
            match entry {
                Ok(e) => children.push(e.path()),
                Err(e) => report(cannot(base_path, "read", &e), errors),
            }
        }
        children.sort();

        for child in &children {
            archive_path_recursive(writer, archive, child, &rel, excludes, verbose, errors);
        }
    }
}

/// One `-C` directive applied to the directory the previous ones landed on.
///
/// Cumulative, and an absolute directive restarts the chain — both matching
/// GNU: `-C d1 -C ../d2` lands on `d1/../d2`, and `-C d1 -C /tmp` lands on
/// `/tmp`. The result is deliberately *not* normalised; `d1/../d2` is left for
/// the filesystem to resolve, because collapsing `..` textually is wrong when a
/// component is a symlink — the same reason `sanitize_member_name` refuses to
/// fold `..` rather than rejecting it.
fn chdir_step(base: &Path, dir: &str) -> PathBuf {
    let d = Path::new(dir);
    if d.is_absolute() {
        d.to_path_buf()
    } else {
        base.join(d)
    }
}

/// What an extract or list will do, worked out from the operand list alone.
///
/// Extract and list do not chdir the process at all: they hold a destination
/// and join member names onto it, and `sanitize_member_name` is what keeps a
/// member from escaping that destination. Rebuilding the destination by path
/// arithmetic keeps that boundary exactly where it was, whereas an
/// `env::set_current_dir` would move the ground underneath it.
struct Plan<'a> {
    /// Each `-C` that governs something, as `(directive, directory it lands
    /// on)`, in order. Both halves are needed: the directory to check, and the
    /// directive to *name* if the check fails. Folding the chain first and
    /// checking only the result would report a composite path (`./d1/../d2`)
    /// that appears nowhere on the command line, and would silently accept
    /// `-C nosuch -C ..` — a chain whose ends exist but whose middle does not,
    /// which GNU rejects.
    chdirs: Vec<(&'a str, PathBuf)>,
    /// Each member name from the command line, with the directory in force
    /// where it appeared, because that is the one its members extract into:
    /// `tar -xf a.tar -C d1 one -C ../d2 two` puts `one` under `d1` and `two`
    /// under `d2`. Empty when no names were given, which means "everything".
    filters: Vec<(&'a str, PathBuf)>,
    /// Where members go when `filters` is empty.
    fallback: PathBuf,
}

/// Read an operand list as a [`Plan`]. Pure: nothing is checked or opened.
fn plan(operands: &[Operand]) -> Plan<'_> {
    // A `-C` that no operand follows governs nothing, and GNU never performs
    // it — `tar -xf a.tar one -C nosuch` succeeds, while `tar -xf a.tar -C
    // nosuch` fails. The two are not inconsistent: with no names at all, the
    // implicit "the whole archive" operand sits at the end of the list, so
    // every directive is in front of something. Hence the walk stops at the
    // last name, and covers everything when there is none.
    let end = match operands
        .iter()
        .rposition(|o| matches!(o, Operand::Member(_)))
    {
        Some(i) => i.saturating_add(1),
        None => operands.len(),
    };

    let mut chdirs = Vec::new();
    let mut filters = Vec::new();
    let mut base = PathBuf::from(".");
    for op in operands.get(..end).unwrap_or(operands) {
        match op {
            Operand::Chdir(dir) => {
                base = chdir_step(&base, dir);
                chdirs.push((dir.as_str(), base.clone()));
            }
            Operand::Member(name) => filters.push((name.as_str(), base.clone())),
        }
    }

    Plan {
        chdirs,
        filters,
        fallback: base,
    }
}

/// Which directory an archive member extracts into, or `None` to skip it.
///
/// Also records that the name responsible has matched something, so that the
/// ones that never did can be reported at the end. Attribution is to the
/// *first* name that matches, which is GNU's rule and is the reason
/// `tar -x dir dir/b.txt` reports "dir/b.txt: Not found in archive" even
/// though the archive plainly contains it: `dir` claims every member beneath
/// it, so the more specific name never gets to be first. Reversing the two
/// works, because then `dir/b.txt` claims its member and `dir` claims the
/// rest. The same rule explains a name repeated on the command line being
/// reported missing, without that needing a case of its own.
fn select_destination<'a>(
    plan: &'a Plan<'_>,
    matched: &mut [bool],
    member: &str,
) -> Option<&'a Path> {
    if plan.filters.is_empty() {
        return Some(&plan.fallback);
    }
    for ((name, dest), hit) in plan.filters.iter().zip(matched.iter_mut()) {
        if member_matches(member, name) {
            *hit = true;
            return Some(dest);
        }
    }
    None
}

/// Report every command-line name that matched nothing in the archive.
///
/// Appended to `errors` as well as printed, because a name that selected
/// nothing means the caller did not get what it asked for — silence here would
/// be a successful-looking run that extracted or listed less than requested.
fn report_unmatched(plan: &Plan<'_>, matched: &[bool], errors: &mut Vec<String>) {
    for ((name, _), hit) in plan.filters.iter().zip(matched.iter()) {
        if !*hit {
            report(
                format!("tar: {}: Not found in archive", escape_os(name)),
                errors,
            );
        }
    }
}

/// Refuse a `-C` that does not name a usable directory, before extracting.
///
/// Up front rather than lazily, so a mistyped destination halfway down a long
/// operand list is not discovered after half the archive has been written.
fn check_chdirs(chdirs: &[(&str, PathBuf)]) {
    for (dir, landed) in chdirs {
        match fs::metadata(landed) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => fail_cannot_open(dir, &io::Error::from(io::ErrorKind::NotADirectory)),
            Err(e) => fail_cannot_open(dir, &e),
        }
    }
}

/// Does an archive member fall under a name given on the command line?
///
/// Literal and component-wise, which is three separate deliberate refusals,
/// all measured against GNU tar 1.35:
///
/// * **No globbing.** `dir/*` matches nothing; GNU warns and tells you to pass
///   `--wildcards`. Treating `*` as a wildcard here would be the more helpful
///   guess and the wrong one — it would silently extract more than a script
///   asking for a literally-named member had authorised.
/// * **No normalisation.** `./top.txt` does not match `top.txt`.
/// * **Component boundaries.** `di` does not match `dir`, and `dir` matches
///   `dir/b.txt` — a directory name selects everything beneath it.
///
/// A trailing slash on either side is ignored, so `-x dir/` works and the
/// archive's own `dir/` entry is recognised as the directory it names.
fn member_matches(member: &str, name: &str) -> bool {
    let member = member.trim_end_matches('/');
    let name = name.trim_end_matches('/');
    // Only reachable from a name that was nothing but slashes. Matching
    // everything would be a surprising reading of `tar -xf a.tar /`, and
    // "Not found in archive" is the honest one.
    if name.is_empty() {
        return false;
    }
    member == name
        || member
            .strip_prefix(name)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// GNU's response to something it needed and could not have: the archive, or
/// a `-C` directory.
///
/// Two lines and status 2, and the wording is GNU's rather than ours in three
/// ways worth not "improving": it says "Cannot open" for a `-C` even though
/// nothing was being opened; it calls the failure unrecoverable rather than
/// accumulating it the way a missing *file operand* is accumulated; and it
/// says it twice, once naming the thing and once announcing the exit. A script
/// that greps tar's stderr is why this text is an interface and not a message.
///
/// The distinction from [`exit_with_previous_errors`] is GNU's, and it is the
/// difference between "I could not start" and "I finished, badly": a missing
/// archive or `-C` stops everything immediately, whereas a missing operand
/// among good ones still produces an archive containing the good ones.
fn fail_cannot_open(what: &str, err: &io::Error) -> ! {
    fail_cannot(what, "open", err);
}

/// [`cannot`] as a fatal error: the diagnostic, then GNU's "not recoverable"
/// line, then status 2.
///
/// Used where the failure is not *about* one member — a lost archive, a `-C`
/// that does not exist — so there is nothing to accumulate and carry on with.
fn fail_cannot<P: AsRef<OsStr>>(name: P, call: &str, err: &io::Error) -> ! {
    eprintln!("{}", cannot(name, call, err));
    eprintln!("tar: Error is not recoverable: exiting now");
    process::exit(2);
}

/// GNU's closing line when members failed but the archive was still produced.
///
/// Status 2, the same as a fatal error — tar does not distinguish "could not
/// start" from "finished with failures" by exit code, only by wording, so a
/// caller that only checks `$?` cannot tell whether an archive exists. That is
/// GNU's design and we copy it; a script wanting the difference has to test
/// for the archive.
fn exit_with_previous_errors() -> ! {
    eprintln!("tar: Exiting with failure status due to previous errors");
    process::exit(2);
}

/// The POSIX `strerror` text for an I/O failure.
///
/// `io::Error`'s own `Display` is the *host's* wording, which is why this
/// exists: on Slate OS and Linux a missing directory reads "No such file or
/// directory", but the same error on a Windows build of these tests reads
/// "The system cannot find the file specified. (os error 2)". tar's stderr is
/// an interface — scripts grep it, and the GNU-comparison harnesses in
/// `scripts/` diff it line for line — so it must not vary with where the
/// binary was compiled. The mapped kinds are the ones tar can actually
/// produce here; anything else falls back to the host text, which is still
/// better than nothing and is visibly odd enough to prompt adding a case.
fn strerror(err: &io::Error) -> String {
    match err.kind() {
        io::ErrorKind::NotFound => "No such file or directory".to_string(),
        io::ErrorKind::PermissionDenied => "Permission denied".to_string(),
        io::ErrorKind::NotADirectory => "Not a directory".to_string(),
        io::ErrorKind::IsADirectory => "Is a directory".to_string(),
        io::ErrorKind::AlreadyExists => "File exists".to_string(),
        _ => err.to_string(),
    }
}

/// GNU's `call_arg_error` (`tar/misc.c`): `tar: NAME: Cannot CALL: STRERROR`.
///
/// Nearly every non-fatal diagnostic tar emits is this one sentence with a
/// different verb in the middle — `Cannot stat`, `Cannot open`, `Cannot mkdir`,
/// `Cannot write` — so it is worth having once rather than reassembled at each
/// of the dozen sites, where the pieces drifted apart before this existed
/// (`create '{}': {}`, `tar: symlink {}: {}`, `set permissions on '{}': {}`).
///
/// Two details are measured against GNU 1.35 rather than guessed:
///
/// * **The name is not quoted.** GNU tar's default quoting style is `escape`,
///   not the `'...'` *shell-escape* style coreutils defaults to, so a member
///   called `it's a *.txt` prints bare: `tar: src/it's a *.txt: Cannot open:
///   File exists`. Only unprintable bytes are escaped, and a newline comes out
///   as `\n`. Hence [`escape_os`] here and `quoteaf` nowhere.
/// * **The name goes through [`escape_os`], never `Path::display()`.** That
///   method is documented to substitute U+FFFD for bytes it cannot decode,
///   which for a filename is silent corruption of the one field the message
///   exists to identify — and the project forbids lossy conversion of
///   OS-boundary data outright.
fn cannot<P: AsRef<OsStr>>(name: P, call: &str, err: &io::Error) -> String {
    format!(
        "tar: {}: Cannot {}: {}",
        escape_os(name),
        call,
        strerror(err)
    )
}

/// Which of tar's two failure modes a diagnostic belongs to.
///
/// The distinction is GNU's and it is visible in the exit path: a member that
/// could not be written is reported, the run continues, and the last line is
/// "Exiting with failure status due to previous errors"; the archive itself
/// failing means there is no next member to try, so the last line is "Error is
/// not recoverable: exiting now" and nothing further is attempted.
///
/// It is an enum rather than a convention because the two were previously told
/// apart by which function had produced the `String`, and that is invisible at
/// the call site: a truncated archive was reported once by the member loop and
/// then again by the header read that followed it, so GNU's single
/// "Unexpected EOF in archive" came out twice.
enum Fail {
    /// About one member. Report it and keep going.
    Member(String),
    /// About the archive. Stop.
    Fatal(String),
}

/// Print a diagnostic and remember that it happened.
///
/// The remembering is the point: a non-fatal failure must still reach the exit
/// status, and the `Vec` being non-empty at the end is what triggers
/// [`exit_with_previous_errors`]. Printing without pushing is the bug this
/// helper exists to make hard to write — it produces a run that complains on
/// stderr and then exits 0.
fn report(msg: String, errors: &mut Vec<String>) {
    eprintln!("{}", msg);
    errors.push(msg);
}

/// Create a tar archive from the listed files/directories.
fn create_archive(opts: &Options) -> Result<(), String> {
    let mut writer: Box<dyn Write> = if opts.archive == "-" {
        Box::new(io::stdout().lock())
    } else {
        // GNU says "Cannot open" even when it is creating, and treats the
        // failure as fatal rather than reporting it at the end.
        Box::new(
            File::create(&opts.archive).unwrap_or_else(|e| fail_cannot_open(&opts.archive, &e)),
        )
    };

    let mut errors: Vec<String> = Vec::new();

    // The archive above is already open, which is the whole reason it is
    // opened before this loop rather than inside it: `-f` names a path in the
    // directory tar was invoked from, and the chdirs below must not reach it.
    // GNU makes this observable by leaving an empty archive behind when a
    // later `-C` fails, and so do we.
    for operand in &opts.operands {
        let file_arg = match operand {
            Operand::Chdir(dir) => {
                if let Err(e) = env::set_current_dir(dir) {
                    fail_cannot_open(dir, &e);
                }
                continue;
            }
            Operand::Member(name) => name,
        };

        let path = Path::new(file_arg);
        // `exists()` was the wrong probe: it collapses every reason a path
        // cannot be examined into "absent", so an operand inside a directory
        // we lack search permission on was reported as "No such file or
        // directory" — a claim about the filesystem that is false, and that
        // sends the user looking for a typo instead of a permission. Ask for
        // the metadata and report why it could not be had, which is also what
        // GNU's "Cannot stat" is naming: the call it made, not a conclusion.
        // `symlink_metadata`, not `metadata`, because tar archives a dangling
        // symlink as a symlink rather than refusing it as a missing target.
        if let Err(e) = fs::symlink_metadata(path) {
            report(cannot(file_arg, "stat", &e), &mut errors);
            continue;
        }

        // Use empty prefix for top-level entries.
        let parent_prefix = path.parent().and_then(|p| p.to_str()).unwrap_or("");
        let prefix = if parent_prefix == "." || parent_prefix.is_empty() {
            String::new()
        } else {
            parent_prefix.to_string()
        };

        archive_path_recursive(
            &mut writer,
            &opts.archive,
            path,
            &prefix,
            &opts.excludes,
            opts.verbose,
            &mut errors,
        );
    }

    // Write two zero blocks to mark end of archive. Same reasoning as the
    // header write: this is the archive failing, not a member, so it names the
    // archive and stops rather than joining `errors`.
    let zero_block = [0u8; BLOCK_SIZE];
    for _ in 0..2 {
        if let Err(e) = writer.write_all(&zero_block) {
            fail_cannot(&opts.archive, "write", &e);
        }
    }
    if let Err(e) = writer.flush() {
        fail_cannot(&opts.archive, "write", &e);
    }

    // Each failure was already reported as it happened, which is why this
    // says nothing about how many there were: GNU's summary is a verdict, not
    // a count, and the count we used to print was also prefixed "tar: " twice
    // because `main` adds its own.
    if !errors.is_empty() {
        exit_with_previous_errors();
    }

    Ok(())
}

// ============================================================================
// READ / EXTRACT / LIST helpers
// ============================================================================

/// Read exactly `n` bytes from the reader, returning an error on short reads.
///
/// Running out of archive is by far the likeliest failure here and gets GNU's
/// own sentence for it, measured on an archive cut off mid-member: `tar:
/// Unexpected EOF in archive`, followed by the unrecoverable line and status 2.
/// It names no file, because at this point tar no longer knows which member it
/// was in the middle of — and neither do we.
fn read_exact<R: Read>(reader: &mut R, buf: &mut [u8]) -> Result<(), String> {
    reader.read_exact(buf).map_err(|e| match e.kind() {
        io::ErrorKind::UnexpectedEof => "tar: Unexpected EOF in archive".to_string(),
        _ => format!("tar: Cannot read: {}", strerror(&e)),
    })
}

/// Check whether a 512-byte block is entirely zero (end-of-archive marker).
fn is_zero_block(block: &[u8; BLOCK_SIZE]) -> bool {
    block.iter().all(|&b| b == 0)
}

/// Parse a raw 512-byte header block into a `TarEntry`.
fn parse_header(block: &[u8; BLOCK_SIZE]) -> Result<TarEntry, String> {
    // Check magic.
    let magic = &block[257..263];
    if magic != USTAR_MAGIC {
        // Some implementations use "ustar " (without NUL). Accept that too.
        if &block[257..262] != b"ustar" {
            return Err("not a ustar archive (bad magic)".to_string());
        }
    }

    // Verify checksum.
    let stored_cksum = decode_octal(&block[148..156]);
    let computed_cksum = compute_checksum(block);
    if stored_cksum != u64::from(computed_cksum) {
        return Err(format!(
            "checksum mismatch: stored={}, computed={}",
            stored_cksum, computed_cksum
        ));
    }

    let prefix = field_to_string(&block[345..500]);
    let name = field_to_string(&block[..100]);
    let path = if prefix.is_empty() {
        name
    } else {
        format!("{}/{}", prefix, name)
    };

    let typeflag = block[156];
    // Some old archives use '\0' for regular files instead of '0'.
    let effective_typeflag = if typeflag == 0 {
        TYPEFLAG_REGULAR
    } else {
        typeflag
    };

    Ok(TarEntry {
        path,
        mode: decode_octal(&block[100..108]) as u32,
        uid: decode_octal(&block[108..116]),
        gid: decode_octal(&block[116..124]),
        size: decode_octal(&block[124..136]),
        mtime: decode_octal(&block[136..148]),
        typeflag: effective_typeflag,
        linkname: field_to_string(&block[157..257]),
        uname: field_to_string(&block[265..297]),
        gname: field_to_string(&block[297..329]),
    })
}

/// Print an entry in verbose `ls -l`-style format.
fn print_verbose_entry(entry: &TarEntry) {
    let perms = format_permissions(entry.mode, entry.typeflag);
    let owner = if entry.uname.is_empty() {
        format!("{}", entry.uid)
    } else {
        entry.uname.clone()
    };
    let group = if entry.gname.is_empty() {
        format!("{}", entry.gid)
    } else {
        entry.gname.clone()
    };
    let ts = format_timestamp(entry.mtime);

    println!(
        "{} {}/{} {:>8} {} {}",
        perms, owner, group, entry.size, ts, entry.path
    );
}

/// Print an entry as JSON.
fn print_json_entry(entry: &TarEntry) {
    // Minimal JSON encoding: escape backslashes and double quotes in strings.
    fn json_str(s: &str) -> String {
        s.replace('\\', "\\\\").replace('"', "\\\"")
    }

    let type_str = match entry.typeflag {
        TYPEFLAG_REGULAR => "file",
        TYPEFLAG_DIRECTORY => "directory",
        TYPEFLAG_SYMLINK => "symlink",
        _ => "other",
    };

    println!(
        "{{\"path\":\"{}\",\"type\":\"{}\",\"size\":{},\"mode\":{},\"uid\":{},\"gid\":{},\"mtime\":{},\"uname\":\"{}\",\"gname\":\"{}\"{}}}",
        json_str(&entry.path),
        type_str,
        entry.size,
        entry.mode,
        entry.uid,
        entry.gid,
        entry.mtime,
        json_str(&entry.uname),
        json_str(&entry.gname),
        if entry.typeflag == TYPEFLAG_SYMLINK {
            format!(",\"linkname\":\"{}\"", json_str(&entry.linkname))
        } else {
            String::new()
        }
    );
}

// ============================================================================
// EXTRACT mode
// ============================================================================

/// Set file permissions (Unix only).
///
/// Returns the raw `io::Error` rather than a message, because the message has
/// to name the *member* and this function only knows the destination path they
/// were joined into — see the note at the extract loop's `named`.
#[cfg(unix)]
fn set_permissions(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_permissions(_path: &Path, _mode: u32) -> io::Result<()> {
    // Not applicable on non-Unix hosts.
    Ok(())
}

/// GNU's `chmod_error_details`: `NAME: Cannot change mode to MODE`, the mode in
/// four octal digits.
///
/// The one diagnostic in this file taken from GNU's source rather than measured
/// against a running GNU tar: provoking a chmod failure needs a file we own on
/// a filesystem that refuses the change — an immutable inode — which needs
/// root, which the comparison harness does not have. It is also the one that
/// does not fit [`cannot`]'s shape, since the verb carries an argument.
fn cannot_chmod(name: &str, mode: u32, err: &io::Error) -> String {
    format!(
        "tar: {}: Cannot change mode to {:04o}: {}",
        escape_os(name),
        mode,
        strerror(err)
    )
}

/// Extract all entries from an archive.
fn extract_archive(opts: &Options) -> Result<(), String> {
    let mut reader: Box<dyn Read> = if opts.archive == "-" {
        Box::new(io::stdin().lock())
    } else {
        Box::new(File::open(&opts.archive).unwrap_or_else(|e| fail_cannot_open(&opts.archive, &e)))
    };

    // A `-C` target is not created if it is missing. GNU chdirs into it, and a
    // chdir cannot conjure a directory: `tar -xf a.tar -C nosuch` exits 2 with
    // "Cannot open", it does not helpfully make `nosuch` and extract into it.
    // We used to `create_dir_all` here, which turned a typo'd destination into
    // a silently-created one — the archive would extract, into a directory the
    // user had no reason to expect and would not think to look in.
    let plan = plan(&opts.operands);
    check_chdirs(&plan.chdirs);
    let mut matched = vec![false; plan.filters.len()];

    let mut errors: Vec<String> = Vec::new();
    let mut consecutive_zero_blocks = 0u32;
    let mut block = [0u8; BLOCK_SIZE];
    // GNU says this once per archive, not once per member.
    let mut warned_absolute = false;

    loop {
        if let Err(e) = read_exact(&mut reader, &mut block) {
            // EOF in the middle of a header is not an error if we already
            // saw at least one zero block.
            if consecutive_zero_blocks > 0 {
                break;
            }
            return Err(e);
        }

        if is_zero_block(&block) {
            consecutive_zero_blocks += 1;
            if consecutive_zero_blocks >= 2 {
                break;
            }
            continue;
        }
        consecutive_zero_blocks = 0;

        let entry = match parse_header(&block) {
            Ok(e) => e,
            Err(e) => {
                let msg = format!("tar: skipping bad header: {}", e);
                eprintln!("{}", msg);
                errors.push(msg);
                continue;
            }
        };

        // Member names select against the archive's own name, before
        // stripping: `tar -x --strip-components=1 dir` matches `dir/b.txt` and
        // then writes it as `b.txt`. Matching the stripped name instead would
        // mean the name the user typed had to be the one that survives the
        // strip, which is not the name they can see in `tar -t`.
        let base_dir = match select_destination(&plan, &mut matched, &entry.path) {
            Some(dir) => dir,
            None => {
                skip_data(&mut reader, entry.size)?;
                continue;
            }
        };

        // Apply --strip-components.
        let output_path_str = if opts.strip_components > 0 {
            match strip_components(&entry.path, opts.strip_components) {
                Some(p) if !p.is_empty() => p,
                _ => {
                    // Skip entries that are fully stripped.
                    skip_data(&mut reader, entry.size)?;
                    continue;
                }
            }
        } else {
            entry.path.clone()
        };

        // Apply --exclude. Matched against the name as the archive gives it,
        // before sanitizing, so a pattern the user wrote still means what they
        // meant when the member is absolute.
        if is_excluded(&output_path_str, &opts.excludes) {
            skip_data(&mut reader, entry.size)?;
            continue;
        }

        // Nothing below may use the archive's own name as a path. This is the
        // only thing standing between a downloaded archive and an arbitrary
        // file write; it runs last, after stripping and excluding, so neither
        // of those can hand it a name it has not checked.
        if output_path_str.starts_with('/') && !warned_absolute {
            eprintln!("tar: Removing leading '/' from member names");
            warned_absolute = true;
        }
        let output_path_str = match sanitize_member_name(&output_path_str) {
            Ok(p) => p,
            Err(msg) => {
                report(format!("tar: {}", msg), &mut errors);
                skip_data(&mut reader, entry.size)?;
                continue;
            }
        };

        let dest = base_dir.join(&output_path_str);

        if opts.verbose {
            eprintln!("{}", output_path_str);
        }

        // Every diagnostic below names `output_path_str`, never `dest`.
        //
        // They are not the same string, and GNU prints the first: `tar -xkf
        // a.tar -C dst` over an existing file reports `tar: src/top.txt:
        // Cannot open: File exists`, not `dst/src/top.txt`. GNU gets this for
        // free — it has chdir'd, so the member name *is* the path — whereas we
        // do the join arithmetic instead (see `select_destination`) and so
        // hold a `dest` that is tempting and wrong to print. It is wrong twice
        // over on a Windows build, where the join introduces a separator the
        // archive never contained and `escape_os` then dutifully escapes it:
        // `tar: .\\src/dir/b.txt: Cannot open: …`.
        let named = output_path_str.as_str();
        // The member's own parent, for the same reason: `Cannot mkdir` names
        // `src`, not `dst/src`.
        let named_parent = Path::new(named)
            .parent()
            .and_then(Path::to_str)
            .filter(|p| !p.is_empty())
            .unwrap_or(named);

        match entry.typeflag {
            TYPEFLAG_DIRECTORY => {
                if let Err(e) = fs::create_dir_all(&dest) {
                    report(cannot(named, "mkdir", &e), &mut errors);
                }
                if opts.preserve_permissions
                    && let Err(e) = set_permissions(&dest, entry.mode)
                {
                    report(cannot_chmod(named, entry.mode, &e), &mut errors);
                }
            }
            TYPEFLAG_REGULAR | b'\0' => {
                // Ensure parent directory exists.
                if let Some(parent) = dest.parent()
                    && !parent.exists()
                    && let Err(e) = fs::create_dir_all(parent)
                {
                    report(cannot(named_parent, "mkdir", &e), &mut errors);
                    skip_data(&mut reader, entry.size)?;
                    continue;
                }

                // `-k` refusing a member is a *failure*, not a note: GNU says
                // "tar: src/top.txt: Cannot open: File exists" and exits 2. We
                // used to print "already exists, skipping" and exit 0, so a
                // script could not tell a `-k` run that extracted everything
                // from one that extracted nothing.
                if opts.keep_old_files && dest.exists() {
                    let exists = io::Error::from(io::ErrorKind::AlreadyExists);
                    report(cannot(named, "open", &exists), &mut errors);
                    skip_data(&mut reader, entry.size)?;
                    continue;
                }

                match extract_file_data(&mut reader, &dest, named, entry.size) {
                    Ok(()) => {
                        if opts.preserve_permissions
                            && let Err(e) = set_permissions(&dest, entry.mode)
                        {
                            report(cannot_chmod(named, entry.mode, &e), &mut errors);
                        }
                    }
                    // A failed write to the destination is about this member
                    // and the next one may still succeed; a failed read of the
                    // archive means there is no next one.
                    Err(Fail::Member(msg)) => report(msg, &mut errors),
                    Err(Fail::Fatal(msg)) => return Err(msg),
                }
            }
            TYPEFLAG_SYMLINK => {
                // Symlink creation: best-effort.
                if let Some(parent) = dest.parent()
                    && !parent.exists()
                {
                    let _ = fs::create_dir_all(parent);
                }
                #[cfg(unix)]
                {
                    if let Err(e) = std::os::unix::fs::symlink(&entry.linkname, &dest) {
                        // The one message here that is not `Cannot VERB`:
                        // GNU names the target too, and quotes *it* — measured
                        // in the C locale as `tar: src/link: Cannot create
                        // symlink to 'top.txt': File exists`. The member is
                        // bare (escape style) and the target is not, because
                        // the target is a value being quoted into a sentence
                        // rather than the subject of the message.
                        report(
                            format!(
                                "tar: {}: Cannot create symlink to {}: {}",
                                escape_os(named),
                                quoteaf(entry.linkname.as_bytes()),
                                strerror(&e)
                            ),
                            &mut errors,
                        );
                    }
                }
                #[cfg(not(unix))]
                {
                    eprintln!(
                        "tar: {}: symlink extraction not supported on this platform",
                        escape_os(named)
                    );
                }
                skip_data(&mut reader, entry.size)?;
            }
            _ => {
                eprintln!(
                    "tar: {}: unsupported type flag {}, skipping",
                    escape_os(&entry.path),
                    // The byte, not `as char`: `u8 as char` is a Latin-1
                    // widening, so a crafted type flag of 0xE9 would be
                    // reported as an accented letter that cannot fit in the
                    // one byte the field actually holds.
                    quoteaf(&[entry.typeflag])
                );
                skip_data(&mut reader, entry.size)?;
            }
        }
    }

    report_unmatched(&plan, &matched, &mut errors);

    if !errors.is_empty() {
        exit_with_previous_errors();
    }

    Ok(())
}

/// Read file data from the archive and write it to `dest`, then skip any
/// padding bytes to the next 512-byte boundary.
///
/// `named` is how the message should spell this member; `dest` is where the
/// bytes go. See the extract loop for why those differ.
fn extract_file_data<R: Read>(
    reader: &mut R,
    dest: &Path,
    named: &str,
    size: u64,
) -> Result<(), Fail> {
    // "Cannot open", not "Cannot create": GNU reports a failed `open(…,
    // O_CREAT)` with the verb it called, which is why extracting into a
    // read-only tree says `Cannot open: No such file or directory` rather than
    // anything mentioning creation.
    let mut file = File::create(dest).map_err(|e| Fail::Member(cannot(named, "open", &e)))?;

    let mut remaining = size;
    let mut buf = [0u8; 8192];
    while remaining > 0 {
        let to_read = (remaining as usize).min(buf.len());
        read_exact(reader, &mut buf[..to_read]).map_err(Fail::Fatal)?;
        file.write_all(&buf[..to_read])
            .map_err(|e| Fail::Member(cannot(named, "write", &e)))?;
        remaining = remaining.saturating_sub(to_read as u64);
    }

    // Skip padding.
    let pad = (BLOCK_SIZE - (size as usize % BLOCK_SIZE)) % BLOCK_SIZE;
    if pad > 0 {
        let mut discard = [0u8; BLOCK_SIZE];
        read_exact(reader, &mut discard[..pad]).map_err(Fail::Fatal)?;
    }

    Ok(())
}

/// Skip `size` bytes of data plus padding without extracting.
fn skip_data<R: Read>(reader: &mut R, size: u64) -> Result<(), String> {
    let total = if size == 0 {
        0
    } else {
        // Round up to next block boundary.
        let blocks = size.div_ceil(BLOCK_SIZE as u64);
        blocks * BLOCK_SIZE as u64
    };

    let mut remaining = total;
    let mut discard = [0u8; 8192];
    while remaining > 0 {
        let to_read = (remaining as usize).min(discard.len());
        read_exact(reader, &mut discard[..to_read])?;
        remaining = remaining.saturating_sub(to_read as u64);
    }

    Ok(())
}

// ============================================================================
// LIST mode
// ============================================================================

/// List the contents of an archive.
fn list_archive(opts: &Options) -> Result<(), String> {
    let mut reader: Box<dyn Read> = if opts.archive == "-" {
        Box::new(io::stdin().lock())
    } else {
        Box::new(File::open(&opts.archive).unwrap_or_else(|e| fail_cannot_open(&opts.archive, &e)))
    };

    // Listing writes nothing to the filesystem, so a `-C` cannot change the
    // output — but GNU still chdirs, and so still refuses a directive naming
    // no directory: `tar -tf a.tar -C nosuch` exits 2 without listing a thing.
    // Checking a destination nothing will be written to looks redundant and is
    // not: it is the difference between a typo'd `-C` being reported here and
    // being discovered on the `-x` that follows.
    let plan = plan(&opts.operands);
    check_chdirs(&plan.chdirs);
    let mut matched = vec![false; plan.filters.len()];
    let mut errors: Vec<String> = Vec::new();

    let mut consecutive_zero_blocks = 0u32;
    let mut block = [0u8; BLOCK_SIZE];

    loop {
        if read_exact(&mut reader, &mut block).is_err() {
            if consecutive_zero_blocks > 0 {
                break;
            }
            return Err("tar: Unexpected EOF in archive".to_string());
        }

        if is_zero_block(&block) {
            consecutive_zero_blocks += 1;
            if consecutive_zero_blocks >= 2 {
                break;
            }
            continue;
        }
        consecutive_zero_blocks = 0;

        let entry = match parse_header(&block) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("tar: skipping bad header: {}", e);
                continue;
            }
        };

        // The destination is meaningless here; what is being consulted is
        // whether the member was selected at all, and by which name.
        if select_destination(&plan, &mut matched, &entry.path).is_some() {
            if opts.json {
                print_json_entry(&entry);
            } else if opts.verbose {
                print_verbose_entry(&entry);
            } else {
                println!("{}", entry.path);
            }
        }

        skip_data(&mut reader, entry.size)?;
    }

    report_unmatched(&plan, &matched, &mut errors);

    if !errors.is_empty() {
        exit_with_previous_errors();
    }

    Ok(())
}

// ============================================================================
// Usage / help
// ============================================================================

fn print_usage() {
    eprintln!(
        "\
Usage: tar [OPTIONS] [FILES...]

Modes:
  -c, --create              Create a new archive
  -x, --extract, --get      Extract files from an archive
  -t, --list                List contents of an archive

Required:
  -f <file>                 Archive filename (use '-' for stdin/stdout)

Options:
  -v, --verbose             Verbose output
  -C, --directory <dir>     Change to directory; applies to the operands that
                            follow it, and may be given more than once
  -p, --preserve-permissions Preserve file permissions on extract
  -k, --keep-old-files      Don't overwrite existing files on extract
  --strip-components=N      Strip N leading path components on extract
  --exclude <pattern>       Exclude files matching glob pattern
  --no-same-owner           Don't try to set ownership on extract
  --json                    JSON output for list mode
  -z, --gzip                Filter through gzip (acknowledged, not implemented)
  -j, --bzip2               Filter through bzip2 (acknowledged, not implemented)"
    );
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    // Check for help first.
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 || args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        if args.len() < 2 {
            // GNU exits 2 for a usage error, the same as for a fatal one.
            process::exit(2);
        }
        process::exit(0);
    }

    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("tar: {}", e);
            print_usage();
            process::exit(2);
        }
    };

    // No chdir happens here any more. `-C` is positional: create mode applies
    // each directive as it reaches it in the operand list, and extract/list
    // fold the chain into a base directory. Doing it up front here was what
    // made `-C` behave like a setting — last one wins, applied to everything.
    let result = match opts.mode {
        Mode::Create => create_archive(&opts),
        Mode::Extract => extract_archive(&opts),
        Mode::List => list_archive(&opts),
    };

    // Printed verbatim, not `format!("tar: {}", e)`. Every string that reaches
    // here is already a whole diagnostic line, which is the crate's rule: a
    // message is assembled once, where the facts are, by [`cannot`] or by the
    // handful of sites with a shape of their own. The alternative — some
    // messages prefixed at the source and some at the sink — is what produced
    // `tar: tar: …` and `tar: src/x: src/x: Cannot open: …` before, because
    // whether a given `String` had been through a prefixer was not visible in
    // its type.
    if let Err(e) = result {
        eprintln!("{}", e);
        // Everything that lands here stopped the run rather than being
        // collected, so it gets GNU's fatal ending rather than the
        // "previous errors" one.
        eprintln!("tar: Error is not recoverable: exiting now");
        // 2, not 1. GNU reserves 1 for "some files differ", which only
        // `--compare` can produce, so exiting 1 for a fatal error told a
        // script the archive had been read successfully and merely disagreed
        // with the disk. Every failure tar can reach from here is a 2.
        process::exit(2);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Octal encoding/decoding --

    #[test]
    fn test_encode_octal_basic() {
        let mut buf = [0u8; 8];
        encode_octal(&mut buf, 0o755).unwrap();
        assert_eq!(&buf, b"0000755\0");
    }

    #[test]
    fn test_encode_octal_zero() {
        let mut buf = [0u8; 8];
        encode_octal(&mut buf, 0).unwrap();
        assert_eq!(&buf, b"0000000\0");
    }

    #[test]
    fn test_encode_octal_large() {
        let mut buf = [0u8; 12];
        encode_octal(&mut buf, 0o77_777_777_777).unwrap();
        // 11 octal digits
        assert_eq!(&buf, b"77777777777\0");
    }

    #[test]
    fn test_encode_octal_overflow() {
        let mut buf = [0u8; 4]; // 3 digits max
        assert!(encode_octal(&mut buf, 0o7777).is_err());
    }

    #[test]
    fn test_decode_octal_basic() {
        assert_eq!(decode_octal(b"0000755\0"), 0o755);
    }

    #[test]
    fn test_decode_octal_space_terminated() {
        assert_eq!(decode_octal(b"755 "), 0o755);
    }

    #[test]
    fn test_decode_octal_empty() {
        assert_eq!(decode_octal(b"\0\0\0"), 0);
    }

    // -- String field helpers --

    #[test]
    fn test_copy_str_to_field() {
        let mut field = [0xFFu8; 10];
        copy_str_to_field(&mut field, "hello");
        assert_eq!(&field, b"hello\0\0\0\0\0");
    }

    #[test]
    fn test_copy_str_truncation() {
        let mut field = [0u8; 4];
        copy_str_to_field(&mut field, "longstring");
        assert_eq!(&field, b"long");
    }

    #[test]
    fn test_field_to_string() {
        let field = b"hello\0\0\0\0\0";
        assert_eq!(field_to_string(field), "hello");
    }

    #[test]
    fn test_field_to_string_no_nul() {
        let field = b"hello";
        assert_eq!(field_to_string(field), "hello");
    }

    // -- Glob matching --

    #[test]
    fn test_glob_exact_match() {
        assert!(glob_matches("hello", "hello"));
        assert!(!glob_matches("hello", "world"));
    }

    #[test]
    fn test_glob_star() {
        assert!(glob_matches("*.o", "foo.o"));
        assert!(glob_matches("*.o", ".o"));
        assert!(!glob_matches("*.o", "foo.c"));
    }

    #[test]
    fn test_glob_question() {
        assert!(glob_matches("?.o", "a.o"));
        assert!(!glob_matches("?.o", "ab.o"));
    }

    #[test]
    fn test_glob_complex() {
        assert!(glob_matches("src/*.rs", "src/main.rs"));
        assert!(!glob_matches("src/*.rs", "lib/main.rs"));
    }

    #[test]
    fn test_glob_double_star() {
        // Our simple glob treats * as "any sequence" which is sufficient
        // for the pattern matching we need.
        assert!(glob_matches("*test*", "my_test_file"));
    }

    // -- Path splitting --

    #[test]
    fn test_split_path_short() {
        let (prefix, name) = split_path("short.txt").unwrap();
        assert_eq!(prefix, "");
        assert_eq!(name, "short.txt");
    }

    #[test]
    fn test_split_path_exactly_100() {
        let name = "a".repeat(100);
        let (prefix, result_name) = split_path(&name).unwrap();
        assert_eq!(prefix, "");
        assert_eq!(result_name, name);
    }

    #[test]
    fn test_split_path_long() {
        let long_prefix = "a".repeat(50);
        let long_name = "b".repeat(50);
        let path = format!("{}/{}", long_prefix, long_name);
        let (prefix, name) = split_path(&path).unwrap();
        assert_eq!(prefix, long_prefix);
        assert_eq!(name, long_name);
    }

    #[test]
    fn test_split_path_too_long() {
        let path = format!("{}/{}", "a".repeat(200), "b".repeat(200));
        assert!(split_path(&path).is_err());
    }

    // -- Checksum --

    #[test]
    fn test_checksum_all_zeros() {
        let header = [0u8; BLOCK_SIZE];
        // Checksum field (148..156) treated as spaces = 8 * 0x20 = 256
        assert_eq!(compute_checksum(&header), 256);
    }

    #[test]
    fn test_checksum_consistency() {
        let mut header = [0u8; BLOCK_SIZE];
        header[0] = b'f';
        header[1] = b'o';
        header[2] = b'o';
        let cksum = compute_checksum(&header);
        // Should be deterministic.
        assert_eq!(cksum, compute_checksum(&header));
        // And should be > 256 (the spaces contribute 256).
        assert!(cksum > 256);
    }

    // -- Strip components --

    #[test]
    fn test_strip_components_zero() {
        assert_eq!(
            strip_components("a/b/c.txt", 0),
            Some("a/b/c.txt".to_string())
        );
    }

    #[test]
    fn test_strip_components_one() {
        assert_eq!(
            strip_components("a/b/c.txt", 1),
            Some("b/c.txt".to_string())
        );
    }

    #[test]
    fn test_strip_components_all() {
        assert_eq!(strip_components("a/b", 2), None);
    }

    #[test]
    fn test_strip_components_over() {
        assert_eq!(strip_components("a", 5), None);
    }

    // -- Permission formatting --

    #[test]
    fn test_format_permissions_regular_755() {
        let s = format_permissions(0o755, TYPEFLAG_REGULAR);
        assert_eq!(s, "-rwxr-xr-x");
    }

    #[test]
    fn test_format_permissions_dir_755() {
        let s = format_permissions(0o755, TYPEFLAG_DIRECTORY);
        assert_eq!(s, "drwxr-xr-x");
    }

    #[test]
    fn test_format_permissions_readonly() {
        let s = format_permissions(0o444, TYPEFLAG_REGULAR);
        assert_eq!(s, "-r--r--r--");
    }

    #[test]
    fn test_format_permissions_symlink() {
        let s = format_permissions(0o777, TYPEFLAG_SYMLINK);
        assert_eq!(s, "lrwxrwxrwx");
    }

    // -- Timestamp formatting --

    #[test]
    fn test_format_timestamp_epoch() {
        assert_eq!(format_timestamp(0), "1970-01-01 00:00:00");
    }

    #[test]
    fn test_format_timestamp_known() {
        // 2024-01-01 00:00:00 UTC = 1704067200
        assert_eq!(format_timestamp(1704067200), "2024-01-01 00:00:00");
    }

    #[test]
    fn test_format_timestamp_leap_year() {
        // 2000-03-01 00:00:00 UTC = 951868800
        assert_eq!(format_timestamp(951868800), "2000-03-01 00:00:00");
    }

    // -- Exclusion checking --

    #[test]
    fn test_is_excluded_no_patterns() {
        assert!(!is_excluded("foo.txt", &[]));
    }

    #[test]
    fn test_is_excluded_matching() {
        let excludes = vec!["*.o".to_string()];
        assert!(is_excluded("foo.o", &excludes));
        assert!(!is_excluded("foo.c", &excludes));
    }

    #[test]
    fn test_is_excluded_path_component() {
        let excludes = vec!["*.o".to_string()];
        // Should match the filename component.
        assert!(is_excluded("src/foo.o", &excludes));
    }

    // -- Header round-trip --

    #[test]
    fn test_header_size() {
        assert_eq!(size_of::<TarHeader>(), BLOCK_SIZE);
    }

    #[test]
    fn test_parse_header_zero_block() {
        let block = [0u8; BLOCK_SIZE];
        assert!(is_zero_block(&block));
    }

    #[test]
    fn test_parse_valid_header() {
        let mut block = [0u8; BLOCK_SIZE];

        // Name: "test.txt"
        block[..8].copy_from_slice(b"test.txt");

        // Mode: 0000644
        block[100..107].copy_from_slice(b"0000644");
        block[107] = 0;

        // UID: 0001000
        block[108..115].copy_from_slice(b"0001000");
        block[115] = 0;

        // GID: 0001000
        block[116..123].copy_from_slice(b"0001000");
        block[123] = 0;

        // Size: 00000000013 (11 bytes)
        block[124..135].copy_from_slice(b"00000000013");
        block[135] = 0;

        // Mtime: 00000000000
        block[136..147].copy_from_slice(b"00000000000");
        block[147] = 0;

        // Checksum placeholder (spaces).
        block[148..156].copy_from_slice(b"        ");

        // Typeflag: regular file.
        block[156] = TYPEFLAG_REGULAR;

        // Magic.
        block[257..263].copy_from_slice(USTAR_MAGIC);
        block[263..265].copy_from_slice(USTAR_VERSION);

        // Now compute and set the real checksum.
        let cksum = compute_checksum(&block);
        let cksum_str = format!("{:06o}\0 ", cksum);
        block[148..156].copy_from_slice(&cksum_str.as_bytes()[..8]);

        let entry = parse_header(&block).expect("should parse valid header");
        assert_eq!(entry.path, "test.txt");
        assert_eq!(entry.mode, 0o644);
        assert_eq!(entry.size, 11);
        assert_eq!(entry.typeflag, TYPEFLAG_REGULAR);
    }

    #[test]
    fn test_parse_header_bad_magic() {
        let block = [0xFFu8; BLOCK_SIZE];
        assert!(parse_header(&block).is_err());
    }

    // ---------------- sanitize_member_name ----------------
    //
    // Every rejected case here was an arbitrary file write before this
    // function existed. known-issues.md ->
    // B-tar-EXTRACTS-OUTSIDE-THE-DESTINATION-DIRECTORY.

    #[test]
    fn test_sanitize_plain_name_unchanged() {
        assert_eq!(sanitize_member_name("a/b/c.txt").unwrap(), "a/b/c.txt");
    }

    #[test]
    fn test_sanitize_strips_leading_slash() {
        // Without this, `base_dir.join(name)` returns `name` and `-C` is void.
        assert_eq!(sanitize_member_name("/etc/passwd").unwrap(), "etc/passwd");
        assert_eq!(sanitize_member_name("///etc/passwd").unwrap(), "etc/passwd");
    }

    #[test]
    fn test_sanitize_drops_dot_components() {
        assert_eq!(sanitize_member_name("./a//b/./c").unwrap(), "a/b/c");
    }

    #[test]
    fn test_sanitize_refuses_dotdot() {
        assert!(sanitize_member_name("../../etc/passwd").is_err());
        assert!(sanitize_member_name("a/../b").is_err());
        assert!(sanitize_member_name("/../../root/.ssh/authorized_keys").is_err());
    }

    #[test]
    fn test_sanitize_refuses_backslash_traversal() {
        assert!(sanitize_member_name("..\\..\\x").is_err());
        assert!(sanitize_member_name("a/..\\b").is_err());
    }

    #[test]
    fn test_sanitize_keeps_backslash_inside_a_name() {
        assert_eq!(sanitize_member_name("a/b\\c").unwrap(), "a/b\\c");
    }

    #[test]
    fn test_sanitize_refuses_empty_result() {
        assert!(sanitize_member_name("").is_err());
        assert!(sanitize_member_name("/").is_err());
        assert!(sanitize_member_name("./.").is_err());
    }

    #[test]
    fn test_sanitize_allows_leading_dots_in_a_name() {
        assert_eq!(sanitize_member_name(".bashrc").unwrap(), ".bashrc");
        assert_eq!(sanitize_member_name("a/..foo").unwrap(), "a/..foo");
    }

    #[test]
    fn test_sanitize_runs_after_stripping() {
        // `--strip-components=1` on `x/../../etc/passwd` yields
        // `../../etc/passwd`, which is why the check must come last.
        let stripped = strip_components("x/../../etc/passwd", 1).unwrap();
        assert_eq!(stripped, "../../etc/passwd");
        assert!(sanitize_member_name(&stripped).is_err());
    }

    fn chdir(d: &str) -> Operand {
        Operand::Chdir(d.to_string())
    }

    fn member(d: &str) -> Operand {
        Operand::Member(d.to_string())
    }

    /// The `.` is not cosmetic: it is what lets extract and list join member
    /// names onto the destination unconditionally, with no "was `-C` given?"
    /// branch at either call site.
    #[test]
    fn test_plan_for_nothing_is_here() {
        let p = plan(&[]);
        assert!(p.chdirs.is_empty());
        assert!(p.filters.is_empty());
        assert_eq!(p.fallback, PathBuf::from("."));
    }

    /// The bug the `Operand` list exists to fix: `-C` used to be one value, so
    /// the second directive replaced the first instead of being applied inside
    /// it. Both must appear, in order, and each name must carry the directory
    /// that was in force where *it* appeared.
    #[test]
    fn test_chdir_directives_accumulate_rather_than_replacing() {
        let ops = [
            chdir("one"),
            member("a"),
            chdir("../two"),
            member("b"),
            member("c"),
        ];
        let p = plan(&ops);
        assert_eq!(
            p.chdirs,
            vec![
                ("one", PathBuf::from("./one")),
                ("../two", PathBuf::from("./one/../two")),
            ]
        );
        assert_eq!(
            p.filters,
            vec![
                ("a", PathBuf::from("./one")),
                ("b", PathBuf::from("./one/../two")),
                ("c", PathBuf::from("./one/../two")),
            ]
        );
    }

    /// GNU refuses `-C one a -C two b` with "two: Cannot open" — because the
    /// second chdir happens *inside* `one`, so it looks for `one/two`. Getting
    /// this backwards (resolving each directive from the invocation directory)
    /// would silently use a different directory rather than erroring, so the
    /// joined path is worth asserting on directly.
    #[test]
    fn test_second_chdir_is_relative_to_the_first() {
        let ops = [chdir("one"), member("a"), chdir("two"), member("b")];
        assert_eq!(plan(&ops).chdirs[1].1, PathBuf::from("./one/two"));
    }

    #[test]
    fn test_absolute_directive_restarts_the_chain() {
        let ops = [chdir("one"), chdir("/tmp"), chdir("sub"), member("a")];
        let p = plan(&ops);
        assert_eq!(p.chdirs[1].1, PathBuf::from("/tmp"));
        assert_eq!(p.chdirs[2].1, PathBuf::from("/tmp/sub"));
        assert_eq!(p.filters[0].1, PathBuf::from("/tmp/sub"));
    }

    /// Deliberate: `..` is left in the path for the filesystem to resolve.
    /// Collapsing `./one/../two` to `./two` textually is wrong whenever `one`
    /// is a symlink, which is the same reason `sanitize_member_name` rejects
    /// `..` instead of folding it.
    #[test]
    fn test_chain_is_not_textually_normalised() {
        let ops = [chdir("one"), chdir(".."), member("a")];
        assert_eq!(plan(&ops).chdirs[1].1, PathBuf::from("./one/.."));
    }

    /// Each pair carries the string the *user typed*, not the path it folds
    /// to, because that string is what the error message has to name.
    #[test]
    fn test_chain_reports_the_directive_not_the_folded_path() {
        let ops = [chdir("one"), chdir("../two"), member("a")];
        assert_eq!(plan(&ops).chdirs[1].0, "../two");
    }

    /// A `-C` after the last name governs nothing, so GNU never performs it
    /// and never complains about it: `tar -xf a.tar one -C nosuch` succeeds.
    /// Checking it anyway would fail a command GNU accepts.
    #[test]
    fn test_trailing_chdir_after_the_last_name_is_dropped() {
        let ops = [chdir("one"), member("a"), chdir("nosuch")];
        let p = plan(&ops);
        assert_eq!(p.chdirs, vec![("one", PathBuf::from("./one"))]);
        assert_eq!(p.filters, vec![("a", PathBuf::from("./one"))]);
    }

    /// ...but with no names at all the implicit "the whole archive" operand
    /// sits at the end, so every directive is in front of something and every
    /// one is checked. `tar -xf a.tar -C nosuch` does fail.
    #[test]
    fn test_with_no_names_every_directive_still_counts() {
        let ops = [chdir("one"), chdir("two")];
        let p = plan(&ops);
        assert_eq!(p.chdirs.len(), 2);
        assert_eq!(p.fallback, PathBuf::from("./one/two"));
    }

    #[test]
    fn test_member_matches_exact_and_beneath() {
        assert!(member_matches("top.txt", "top.txt"));
        assert!(member_matches("dir/b.txt", "dir"));
        assert!(member_matches("dir/sub/c.txt", "dir"));
        assert!(member_matches("dir/sub/c.txt", "dir/sub"));
    }

    /// The archive spells a directory `dir/`; the user spells it `dir` or
    /// `dir/`. All four combinations have to line up.
    #[test]
    fn test_member_matches_ignores_trailing_slashes() {
        assert!(member_matches("dir/", "dir"));
        assert!(member_matches("dir/", "dir/"));
        assert!(member_matches("dir", "dir/"));
        assert!(member_matches("dir/b.txt", "dir/"));
    }

    /// A prefix that stops mid-component is not a match — `di` must not
    /// select `dir`, or a name would silently pull in its alphabetical
    /// neighbours.
    #[test]
    fn test_member_matches_respects_component_boundaries() {
        assert!(!member_matches("dir", "di"));
        assert!(!member_matches("dir/b.txt", "di"));
        assert!(!member_matches("dirty/b.txt", "dir"));
    }

    /// No normalisation and no globbing, both measured against GNU 1.35.
    /// `./top.txt` really does not match `top.txt` there, and `dir/*` is
    /// treated as a literal name that is not in the archive (with a warning
    /// pointing at `--wildcards`).
    #[test]
    fn test_member_matches_is_literal() {
        assert!(!member_matches("top.txt", "./top.txt"));
        assert!(!member_matches("dir/b.txt", "dir/*"));
        assert!(!member_matches("dir/b.txt", "*"));
    }

    /// Guard for a name that is nothing but slashes: matching everything
    /// would be a very surprising reading of `tar -xf a.tar /`.
    #[test]
    fn test_member_matches_refuses_an_empty_name() {
        assert!(!member_matches("top.txt", ""));
        assert!(!member_matches("top.txt", "/"));
    }

    /// With no names given, every member is selected and goes to the fallback.
    #[test]
    fn test_no_names_selects_everything() {
        let ops = [chdir("/tmp")];
        let p = plan(&ops);
        let mut hit = vec![];
        assert_eq!(
            select_destination(&p, &mut hit, "anything/at/all"),
            Some(Path::new("/tmp"))
        );
    }

    /// Each name sends its own members to the directory in force where it
    /// appeared — the whole point of the operand list being ordered.
    #[test]
    fn test_each_name_extracts_into_its_own_destination() {
        let ops = [chdir("d1"), member("one"), chdir("../d2"), member("two")];
        let p = plan(&ops);
        let mut hit = vec![false; 2];
        assert_eq!(
            select_destination(&p, &mut hit, "one/x"),
            Some(Path::new("./d1"))
        );
        assert_eq!(
            select_destination(&p, &mut hit, "two"),
            Some(Path::new("./d1/../d2"))
        );
        assert_eq!(select_destination(&p, &mut hit, "three"), None);
        assert_eq!(hit, vec![true, true]);
    }

    /// GNU attributes each member to the *first* name that matches it, which
    /// is why `tar -x dir dir/b.txt` reports `dir/b.txt` as not found: `dir`
    /// claims every member beneath it first. Reversing the two works. This is
    /// surprising enough that it is worth pinning rather than rediscovering.
    #[test]
    fn test_first_matching_name_claims_the_member() {
        let broad_first = [member("dir"), member("dir/b.txt")];
        let p = plan(&broad_first);
        let mut hit = vec![false; 2];
        select_destination(&p, &mut hit, "dir/");
        select_destination(&p, &mut hit, "dir/b.txt");
        assert_eq!(hit, vec![true, false], "the specific name never matched");

        let specific_first = [member("dir/b.txt"), member("dir")];
        let p = plan(&specific_first);
        let mut hit = vec![false; 2];
        select_destination(&p, &mut hit, "dir/");
        select_destination(&p, &mut hit, "dir/b.txt");
        assert_eq!(hit, vec![true, true], "both names matched something");
    }

    /// Falls out of the same rule with no special case: the first copy claims
    /// every member, so the second matched nothing and GNU says so.
    #[test]
    fn test_a_repeated_name_is_reported_missing() {
        let ops = [member("top.txt"), member("top.txt")];
        let p = plan(&ops);
        let mut hit = vec![false; 2];
        select_destination(&p, &mut hit, "top.txt");
        assert_eq!(hit, vec![true, false]);

        let mut errors = Vec::new();
        report_unmatched(&p, &hit, &mut errors);
        assert_eq!(errors, vec!["tar: top.txt: Not found in archive"]);
    }

    // -- Diagnostic wording --

    /// GNU tar's default quoting style is `escape`, not the *shell-escape*
    /// style coreutils defaults to, so a name with a space, an apostrophe or a
    /// glob character in it is printed exactly as it is. Measured on GNU 1.35:
    /// `tar -kxf w.tar` over an existing `src/it's a *.txt` prints
    /// `tar: src/it's a *.txt: Cannot open: File exists` — no quotes anywhere.
    /// Reaching for `quotef`/`quoteaf` here, as the rest of the tree's
    /// utilities correctly do, would make every one of tar's messages differ
    /// from GNU's on exactly the names a user is most likely to get wrong.
    #[test]
    fn test_names_are_not_quoted_the_way_coreutils_quotes_them() {
        let err = io::Error::from(io::ErrorKind::AlreadyExists);
        assert_eq!(
            cannot("src/it's a *.txt", "open", &err),
            "tar: src/it's a *.txt: Cannot open: File exists"
        );
    }

    /// The other half of `escape` style: what it *does* escape. A control
    /// character becomes a C escape rather than being emitted raw, so a member
    /// name containing a newline cannot forge an extra line of tar output — a
    /// script reading stderr line by line would otherwise see an invented
    /// diagnostic. This is also the assertion that pins the rendering away from
    /// `Path::display()`, which would print the newline as a newline.
    #[test]
    fn test_a_newline_in_a_name_cannot_forge_a_line_of_output() {
        let err = io::Error::from(io::ErrorKind::NotFound);
        let msg = cannot("two\nlines", "stat", &err);
        assert_eq!(
            msg,
            "tar: two\\nlines: Cannot stat: No such file or directory"
        );
        assert_eq!(msg.lines().count(), 1);
    }

    /// The errno text is ours, not the host's. Built on Windows,
    /// `io::Error`'s own `Display` for a missing file reads "The system cannot
    /// find the file specified. (os error 2)", which would make tar's stderr —
    /// an interface that scripts grep and that `scripts/` diffs against GNU
    /// line for line — depend on where the binary was compiled.
    #[test]
    fn test_errno_text_does_not_follow_the_build_host() {
        for (kind, text) in [
            (io::ErrorKind::NotFound, "No such file or directory"),
            (io::ErrorKind::PermissionDenied, "Permission denied"),
            (io::ErrorKind::NotADirectory, "Not a directory"),
            (io::ErrorKind::AlreadyExists, "File exists"),
        ] {
            assert_eq!(strerror(&io::Error::from(kind)), text, "{:?}", kind);
        }
    }

    /// Every verb GNU uses is the *call it made*, not a conclusion about the
    /// file, which is why an unreadable directory is `Cannot open` and a
    /// searchless one is `Cannot stat` on its children. All measured.
    #[test]
    fn test_the_verb_is_the_call_that_failed() {
        let denied = io::Error::from(io::ErrorKind::PermissionDenied);
        assert_eq!(
            cannot("src/dir", "open", &denied),
            "tar: src/dir: Cannot open: Permission denied"
        );
        assert_eq!(
            cannot("src/dir/b.txt", "stat", &denied),
            "tar: src/dir/b.txt: Cannot stat: Permission denied"
        );
        assert_eq!(
            cannot(
                "src/dir",
                "mkdir",
                &io::Error::from(io::ErrorKind::AlreadyExists)
            ),
            "tar: src/dir: Cannot mkdir: File exists"
        );
    }

    /// Running off the end of an archive is one specific GNU sentence, and it
    /// names no file on purpose — see [`read_exact`].
    #[test]
    fn test_a_truncated_archive_gets_gnus_sentence() {
        let mut short: &[u8] = b"only seven";
        let mut buf = [0u8; 512];
        assert_eq!(
            read_exact(&mut short, &mut buf),
            Err("tar: Unexpected EOF in archive".to_string())
        );
    }

    /// A message that is printed but not collected is a run that complains and
    /// then exits 0. [`report`] exists so the two cannot come apart.
    #[test]
    fn test_reporting_a_failure_also_records_it() {
        let mut errors = Vec::new();
        report("tar: x: Cannot open: File exists".to_string(), &mut errors);
        assert_eq!(errors, vec!["tar: x: Cannot open: File exists"]);
    }
}
