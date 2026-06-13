//! SlateOS Tape Archive Utility
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

use std::env;
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

struct Options {
    mode: Mode,
    archive: String,
    verbose: bool,
    directory: Option<String>,
    files: Vec<String>,
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
    let mut directory: Option<String> = None;
    let mut files: Vec<String> = Vec::new();
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
                directory = Some(args[i].clone());
            }
            other => {
                if let Some(rest) = other.strip_prefix("--strip-components=") {
                    strip_components = rest.parse::<usize>().map_err(|e| {
                        format!("--strip-components: invalid number '{}': {}", rest, e)
                    })?;
                } else if let Some(rest) = other.strip_prefix("--exclude=") {
                    excludes.push(rest.to_string());
                } else if let Some(rest) = other.strip_prefix("--directory=") {
                    directory = Some(rest.to_string());
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
                                eprintln!("tar: warning: bzip2 compression not implemented, flag acknowledged");
                            }
                            'z' => {
                                gzip = true;
                                eprintln!("tar: warning: gzip compression not implemented, flag acknowledged");
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
                    files.push(other.to_string());
                }
            }
        }
        i += 1;
    }

    let mode = mode.ok_or_else(|| {
        "no mode specified (use -c to create, -x to extract, -t to list)".to_string()
    })?;
    let archive = archive
        .ok_or_else(|| "no archive file specified (use -f <file>)".to_string())?;

    if mode == Mode::Create && files.is_empty() {
        return Err("create mode requires at least one file argument".to_string());
    }

    Ok(Options {
        mode,
        archive,
        verbose,
        directory,
        files,
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
                && glob_matches(pattern, fname_str) {
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
    if meta.is_dir() {
        0
    } else {
        meta.len()
    }
}

// ============================================================================
// Strip path components
// ============================================================================

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

    if mode & 0o400 != 0 { perms[1] = b'r'; }
    if mode & 0o200 != 0 { perms[2] = b'w'; }
    if mode & 0o100 != 0 { perms[3] = b'x'; }
    if mode & 0o040 != 0 { perms[4] = b'r'; }
    if mode & 0o020 != 0 { perms[5] = b'w'; }
    if mode & 0o010 != 0 { perms[6] = b'x'; }
    if mode & 0o004 != 0 { perms[7] = b'r'; }
    if mode & 0o002 != 0 { perms[8] = b'w'; }
    if mode & 0o001 != 0 { perms[9] = b'x'; }

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

/// Write a single file or directory entry (header + data) to the archive.
fn write_entry<W: Write>(
    writer: &mut W,
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

    let (prefix, name) = split_path(&archive_path)?;

    // Fill header fields.
    copy_str_to_field(&mut header[..100], &name);
    encode_octal(&mut header[100..108], u64::from(get_mode(meta)))?;
    encode_octal(&mut header[108..116], 0)?; // uid
    encode_octal(&mut header[116..124], 0)?; // gid
    encode_octal(&mut header[124..136], get_size(meta))?;
    encode_octal(&mut header[136..148], get_mtime(meta))?;
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

    // Write header.
    writer
        .write_all(&header)
        .map_err(|e| format!("write header for '{}': {}", rel_path, e))?;

    // Write file data if it is a regular file with content.
    if meta.is_file() {
        let size = meta.len();
        if size > 0 {
            let mut file = File::open(full_path)
                .map_err(|e| format!("open '{}': {}", full_path.display(), e))?;
            let mut remaining = size;
            let mut buf = [0u8; 8192];
            while remaining > 0 {
                let to_read = (remaining as usize).min(buf.len());
                let n = file
                    .read(&mut buf[..to_read])
                    .map_err(|e| format!("read '{}': {}", full_path.display(), e))?;
                if n == 0 {
                    break;
                }
                writer
                    .write_all(&buf[..n])
                    .map_err(|e| format!("write data for '{}': {}", rel_path, e))?;
                remaining = remaining.saturating_sub(n as u64);
            }

            // Pad to 512-byte boundary.
            let pad_len = (BLOCK_SIZE - (size as usize % BLOCK_SIZE)) % BLOCK_SIZE;
            if pad_len > 0 {
                let zeros = [0u8; BLOCK_SIZE];
                writer
                    .write_all(&zeros[..pad_len])
                    .map_err(|e| format!("write padding for '{}': {}", rel_path, e))?;
            }
        }
    }

    Ok(())
}

/// Recursively collect all files under `base_path` with paths relative to
/// `prefix`, writing each entry to the archive.
fn archive_path_recursive<W: Write>(
    writer: &mut W,
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
            base_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
        )
    };

    if is_excluded(&rel, excludes) {
        return;
    }

    let meta = match fs::metadata(base_path) {
        Ok(m) => m,
        Err(e) => {
            let msg = format!("tar: {}: {}", base_path.display(), e);
            eprintln!("{}", msg);
            errors.push(msg);
            return;
        }
    };

    if meta.is_file() || meta.is_dir() {
        if verbose {
            eprintln!("{}", rel);
        }
        if let Err(e) = write_entry(writer, &rel, base_path, &meta) {
            let msg = format!("tar: {}: {}", rel, e);
            eprintln!("{}", msg);
            errors.push(msg);
        }
    }

    if meta.is_dir() {
        let entries = match fs::read_dir(base_path) {
            Ok(rd) => rd,
            Err(e) => {
                let msg = format!("tar: {}: {}", base_path.display(), e);
                eprintln!("{}", msg);
                errors.push(msg);
                return;
            }
        };

        // Collect and sort entries for deterministic archives.
        let mut children: Vec<PathBuf> = Vec::new();
        for entry in entries {
            match entry {
                Ok(e) => children.push(e.path()),
                Err(e) => {
                    let msg = format!("tar: reading directory '{}': {}", base_path.display(), e);
                    eprintln!("{}", msg);
                    errors.push(msg);
                }
            }
        }
        children.sort();

        for child in &children {
            archive_path_recursive(writer, child, &rel, excludes, verbose, errors);
        }
    }
}

/// Create a tar archive from the listed files/directories.
fn create_archive(opts: &Options) -> Result<(), String> {
    let mut writer: Box<dyn Write> = if opts.archive == "-" {
        Box::new(io::stdout().lock())
    } else {
        Box::new(
            File::create(&opts.archive)
                .map_err(|e| format!("cannot create '{}': {}", opts.archive, e))?,
        )
    };

    let mut errors: Vec<String> = Vec::new();

    for file_arg in &opts.files {
        let path = Path::new(file_arg);
        if !path.exists() {
            let msg = format!("tar: {}: No such file or directory", file_arg);
            eprintln!("{}", msg);
            errors.push(msg);
            continue;
        }

        // Use empty prefix for top-level entries.
        let parent_prefix = path
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or("");
        let prefix = if parent_prefix == "." || parent_prefix.is_empty() {
            String::new()
        } else {
            parent_prefix.to_string()
        };

        archive_path_recursive(
            &mut writer,
            path,
            &prefix,
            &opts.excludes,
            opts.verbose,
            &mut errors,
        );
    }

    // Write two zero blocks to mark end of archive.
    let zero_block = [0u8; BLOCK_SIZE];
    writer
        .write_all(&zero_block)
        .map_err(|e| format!("write end-of-archive: {}", e))?;
    writer
        .write_all(&zero_block)
        .map_err(|e| format!("write end-of-archive: {}", e))?;

    writer
        .flush()
        .map_err(|e| format!("flush archive: {}", e))?;

    if !errors.is_empty() {
        return Err(format!(
            "tar: completed with {} error(s)",
            errors.len()
        ));
    }

    Ok(())
}

// ============================================================================
// READ / EXTRACT / LIST helpers
// ============================================================================

/// Read exactly `n` bytes from the reader, returning an error on short reads.
fn read_exact<R: Read>(reader: &mut R, buf: &mut [u8]) -> Result<(), String> {
    reader
        .read_exact(buf)
        .map_err(|e| format!("read error: {}", e))
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
#[cfg(unix)]
fn set_permissions(path: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(mode);
    fs::set_permissions(path, perms)
        .map_err(|e| format!("set permissions on '{}': {}", path.display(), e))
}

#[cfg(not(unix))]
fn set_permissions(_path: &Path, _mode: u32) -> Result<(), String> {
    // Not applicable on non-Unix hosts.
    Ok(())
}

/// Extract all entries from an archive.
fn extract_archive(opts: &Options) -> Result<(), String> {
    let mut reader: Box<dyn Read> = if opts.archive == "-" {
        Box::new(io::stdin().lock())
    } else {
        Box::new(
            File::open(&opts.archive)
                .map_err(|e| format!("cannot open '{}': {}", opts.archive, e))?,
        )
    };

    let base_dir = opts
        .directory
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    // Ensure the extraction directory exists.
    if !base_dir.exists() {
        fs::create_dir_all(&base_dir)
            .map_err(|e| format!("create directory '{}': {}", base_dir.display(), e))?;
    }

    let mut errors: Vec<String> = Vec::new();
    let mut consecutive_zero_blocks = 0u32;
    let mut block = [0u8; BLOCK_SIZE];

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

        // Apply --exclude.
        if is_excluded(&output_path_str, &opts.excludes) {
            skip_data(&mut reader, entry.size)?;
            continue;
        }

        let dest = base_dir.join(&output_path_str);

        if opts.verbose {
            eprintln!("{}", output_path_str);
        }

        match entry.typeflag {
            TYPEFLAG_DIRECTORY => {
                if let Err(e) = fs::create_dir_all(&dest) {
                    let msg = format!("tar: {}: {}", dest.display(), e);
                    eprintln!("{}", msg);
                    errors.push(msg);
                }
                if opts.preserve_permissions
                    && let Err(e) = set_permissions(&dest, entry.mode) {
                        let msg = format!("tar: {}: {}", dest.display(), e);
                        eprintln!("{}", msg);
                        errors.push(msg);
                    }
            }
            TYPEFLAG_REGULAR | b'\0' => {
                // Ensure parent directory exists.
                if let Some(parent) = dest.parent()
                    && !parent.exists()
                        && let Err(e) = fs::create_dir_all(parent) {
                            let msg = format!("tar: {}: {}", parent.display(), e);
                            eprintln!("{}", msg);
                            errors.push(msg);
                            skip_data(&mut reader, entry.size)?;
                            continue;
                        }

                if opts.keep_old_files && dest.exists() {
                    eprintln!("tar: {}: already exists, skipping", dest.display());
                    skip_data(&mut reader, entry.size)?;
                    continue;
                }

                match extract_file_data(&mut reader, &dest, entry.size) {
                    Ok(()) => {
                        if opts.preserve_permissions
                            && let Err(e) = set_permissions(&dest, entry.mode) {
                                let msg = format!("tar: {}: {}", dest.display(), e);
                                eprintln!("{}", msg);
                                errors.push(msg);
                            }
                    }
                    Err(e) => {
                        let msg = format!("tar: {}: {}", dest.display(), e);
                        eprintln!("{}", msg);
                        errors.push(msg);
                    }
                }
            }
            TYPEFLAG_SYMLINK => {
                // Symlink creation: best-effort.
                if let Some(parent) = dest.parent()
                    && !parent.exists() {
                        let _ = fs::create_dir_all(parent);
                    }
                #[cfg(unix)]
                {
                    if let Err(e) =
                        std::os::unix::fs::symlink(&entry.linkname, &dest)
                    {
                        let msg = format!("tar: symlink {}: {}", dest.display(), e);
                        eprintln!("{}", msg);
                        errors.push(msg);
                    }
                }
                #[cfg(not(unix))]
                {
                    eprintln!(
                        "tar: {}: symlink extraction not supported on this platform",
                        dest.display()
                    );
                }
                skip_data(&mut reader, entry.size)?;
            }
            _ => {
                eprintln!(
                    "tar: {}: unsupported type flag '{}', skipping",
                    entry.path,
                    entry.typeflag as char
                );
                skip_data(&mut reader, entry.size)?;
            }
        }
    }

    if !errors.is_empty() {
        return Err(format!(
            "tar: extraction completed with {} error(s)",
            errors.len()
        ));
    }

    Ok(())
}

/// Read file data from the archive and write it to `dest`, then skip any
/// padding bytes to the next 512-byte boundary.
fn extract_file_data<R: Read>(reader: &mut R, dest: &Path, size: u64) -> Result<(), String> {
    let mut file = File::create(dest)
        .map_err(|e| format!("create '{}': {}", dest.display(), e))?;

    let mut remaining = size;
    let mut buf = [0u8; 8192];
    while remaining > 0 {
        let to_read = (remaining as usize).min(buf.len());
        read_exact(reader, &mut buf[..to_read])?;
        file.write_all(&buf[..to_read])
            .map_err(|e| format!("write '{}': {}", dest.display(), e))?;
        remaining = remaining.saturating_sub(to_read as u64);
    }

    // Skip padding.
    let pad = (BLOCK_SIZE - (size as usize % BLOCK_SIZE)) % BLOCK_SIZE;
    if pad > 0 {
        let mut discard = [0u8; BLOCK_SIZE];
        read_exact(reader, &mut discard[..pad])?;
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
        Box::new(
            File::open(&opts.archive)
                .map_err(|e| format!("cannot open '{}': {}", opts.archive, e))?,
        )
    };

    let mut consecutive_zero_blocks = 0u32;
    let mut block = [0u8; BLOCK_SIZE];

    loop {
        if read_exact(&mut reader, &mut block).is_err() {
            if consecutive_zero_blocks > 0 {
                break;
            }
            return Err("unexpected end of archive".to_string());
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

        if opts.json {
            print_json_entry(&entry);
        } else if opts.verbose {
            print_verbose_entry(&entry);
        } else {
            println!("{}", entry.path);
        }

        skip_data(&mut reader, entry.size)?;
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
  -C, --directory <dir>     Change to directory before operating
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
    if args.len() < 2
        || args.iter().any(|a| a == "--help" || a == "-h")
    {
        print_usage();
        if args.len() < 2 {
            process::exit(1);
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

    // Change to target directory if specified (for create mode).
    if let Some(ref dir) = opts.directory
        && opts.mode == Mode::Create
            && let Err(e) = env::set_current_dir(dir) {
                eprintln!("tar: cannot change to '{}': {}", dir, e);
                process::exit(1);
            }

    let result = match opts.mode {
        Mode::Create => create_archive(&opts),
        Mode::Extract => extract_archive(&opts),
        Mode::List => list_archive(&opts),
    };

    if let Err(e) = result {
        eprintln!("tar: {}", e);
        process::exit(1);
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
}
