//! SlateOS cpio Archive Utility
//!
//! Creates, extracts, lists, and copies files using the cpio newc (SVR4/new
//! ASCII) archive format.
//!
//! # Usage
//!
//! ```text
//! find . -print | cpio -o -F archive.cpio    Create archive
//! cpio -i -F archive.cpio                    Extract archive
//! cpio -i -t -F archive.cpio                 List archive contents
//! find . -print | cpio -p /dest              Copy files to /dest
//! cpio -i -d -v -F archive.cpio              Extract with dirs, verbose
//! ```
//!
//! # Archive Format (newc / SVR4)
//!
//! Each entry consists of a 110-byte hex ASCII header followed by the filename
//! (padded to a 4-byte boundary) and then file data (also padded to a 4-byte
//! boundary). The archive is terminated by a trailer entry whose filename is
//! `TRAILER!!!`.

use std::env;
use std::fs::{self, File, Metadata};
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process;

// ============================================================================
// Constants
// ============================================================================

/// The newc (SVR4 new ASCII) magic number.
const NEWC_MAGIC: &str = "070701";

/// Length of a newc header in bytes.
const HEADER_LEN: usize = 110;

/// Alignment boundary for filename and data.
const ALIGNMENT: usize = 4;

/// Trailer filename marking end of archive.
const TRAILER_NAME: &str = "TRAILER!!!";

/// Block size for block counting (matches traditional cpio behavior).
const BLOCK_SIZE: usize = 512;

// File type bits from the mode field (matching POSIX S_IFMT).
const S_IFMT: u32 = 0o170000;
const S_IFDIR: u32 = 0o040000;
const S_IFREG: u32 = 0o100000;
const S_IFLNK: u32 = 0o120000;

// ============================================================================
// Newc header structure
// ============================================================================

/// Parsed representation of a newc cpio header.
#[derive(Clone, Debug)]
struct CpioHeader {
    ino: u32,
    mode: u32,
    uid: u32,
    gid: u32,
    nlink: u32,
    mtime: u32,
    filesize: u32,
    devmajor: u32,
    devminor: u32,
    rdevmajor: u32,
    rdevminor: u32,
    namesize: u32,
    checksum: u32,
}

/// A full cpio entry: header + filename + data.
struct CpioEntry {
    header: CpioHeader,
    filename: String,
    data: Vec<u8>,
}

// ============================================================================
// CLI options
// ============================================================================

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Mode {
    CopyOut,     // -o: create archive
    CopyIn,      // -i: extract archive
    PassThrough, // -p: copy to directory
}

struct Options {
    mode: Mode,
    make_directories: bool,
    preserve_mtime: bool,
    unconditional: bool,
    verbose: bool,
    list_only: bool,
    archive_file: Option<String>,
    input_file: Option<String>,
    output_file: Option<String>,
    no_absolute_filenames: bool,
    quiet: bool,
    patterns: Vec<String>,
    dest_dir: Option<String>,
}

// ============================================================================
// Argument parsing
// ============================================================================

fn parse_args() -> Result<Options, String> {
    let args: Vec<String> = env::args().collect();

    let mut mode: Option<Mode> = None;
    let mut make_directories = false;
    let mut preserve_mtime = false;
    let mut unconditional = false;
    let mut verbose = false;
    let mut list_only = false;
    let mut archive_file: Option<String> = None;
    let mut input_file: Option<String> = None;
    let mut output_file: Option<String> = None;
    let mut no_absolute_filenames = false;
    let mut quiet = false;
    let mut patterns: Vec<String> = Vec::new();
    let mut dest_dir: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-o" | "--create" => mode = Some(Mode::CopyOut),
            "-i" | "--extract" => mode = Some(Mode::CopyIn),
            "-p" | "--pass-through" => {
                mode = Some(Mode::PassThrough);
                // Next non-flag argument is the destination directory.
                i += 1;
                if i >= args.len() {
                    return Err("-p requires a destination directory".to_string());
                }
                dest_dir = Some(args[i].clone());
            }
            "-d" | "--make-directories" => make_directories = true,
            "-m" | "--preserve-modification-time" => preserve_mtime = true,
            "-u" | "--unconditional" => unconditional = true,
            "-v" | "--verbose" => verbose = true,
            "-t" | "--list" => list_only = true,
            "--no-absolute-filenames" => no_absolute_filenames = true,
            "--quiet" => quiet = true,
            "-F" => {
                i += 1;
                if i >= args.len() {
                    return Err("-F requires a filename argument".to_string());
                }
                archive_file = Some(args[i].clone());
            }
            "-I" => {
                i += 1;
                if i >= args.len() {
                    return Err("-I requires a filename argument".to_string());
                }
                input_file = Some(args[i].clone());
            }
            "-O" => {
                i += 1;
                if i >= args.len() {
                    return Err("-O requires a filename argument".to_string());
                }
                output_file = Some(args[i].clone());
            }
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            other => {
                if let Some(rest) = other.strip_prefix("--file=") {
                    archive_file = Some(rest.to_string());
                } else if other.starts_with('-') && !other.starts_with("--") && other.len() > 2 {
                    // Bundled short flags like -idmv.
                    let chars: Vec<char> = other[1..].chars().collect();
                    let mut ci = 0;
                    while ci < chars.len() {
                        match chars[ci] {
                            'o' => mode = Some(Mode::CopyOut),
                            'i' => mode = Some(Mode::CopyIn),
                            'd' => make_directories = true,
                            'm' => preserve_mtime = true,
                            'u' => unconditional = true,
                            'v' => verbose = true,
                            't' => list_only = true,
                            'p' => {
                                mode = Some(Mode::PassThrough);
                                // Rest of bundled flags are consumed; next arg
                                // is dest dir.
                                i += 1;
                                if i >= args.len() {
                                    return Err(
                                        "-p requires a destination directory".to_string(),
                                    );
                                }
                                dest_dir = Some(args[i].clone());
                                ci = chars.len(); // break inner loop
                                continue;
                            }
                            'F' => {
                                i += 1;
                                if i >= args.len() {
                                    return Err("-F requires a filename argument".to_string());
                                }
                                archive_file = Some(args[i].clone());
                                ci = chars.len();
                                continue;
                            }
                            'I' => {
                                i += 1;
                                if i >= args.len() {
                                    return Err("-I requires a filename argument".to_string());
                                }
                                input_file = Some(args[i].clone());
                                ci = chars.len();
                                continue;
                            }
                            'O' => {
                                i += 1;
                                if i >= args.len() {
                                    return Err("-O requires a filename argument".to_string());
                                }
                                output_file = Some(args[i].clone());
                                ci = chars.len();
                                continue;
                            }
                            c => {
                                return Err(format!("unknown option: -{}", c));
                            }
                        }
                        ci += 1;
                    }
                } else if other.starts_with('-') {
                    return Err(format!("unknown option: {}", other));
                } else {
                    // Positional argument: extraction pattern or pass-through dest.
                    patterns.push(other.to_string());
                }
            }
        }
        i += 1;
    }

    let mode = mode.ok_or_else(|| {
        "no mode specified (use -o to create, -i to extract, -p to pass-through)".to_string()
    })?;

    if mode == Mode::PassThrough && dest_dir.is_none() {
        return Err("-p requires a destination directory argument".to_string());
    }

    Ok(Options {
        mode,
        make_directories,
        preserve_mtime,
        unconditional,
        verbose,
        list_only,
        archive_file,
        input_file,
        output_file,
        no_absolute_filenames,
        quiet,
        patterns,
        dest_dir,
    })
}

// ============================================================================
// Hex encoding / decoding helpers
// ============================================================================

/// Encode a u32 value as an 8-character zero-padded uppercase hex string.
fn encode_hex8(val: u32) -> [u8; 8] {
    let mut buf = [b'0'; 8];
    let hex_chars = b"0123456789ABCDEF";
    for (idx, slot) in buf.iter_mut().enumerate() {
        let shift = (7 - idx) * 4;
        let nibble = ((val >> shift) & 0xF) as usize;
        *slot = hex_chars[nibble];
    }
    buf
}

/// Decode an 8-character hex string to a u32 value.
fn decode_hex8(buf: &[u8]) -> Result<u32, String> {
    if buf.len() < 8 {
        return Err(format!("hex field too short: {} bytes", buf.len()));
    }
    let mut val: u32 = 0;
    for &b in &buf[..8] {
        let nibble = match b {
            b'0'..=b'9' => u32::from(b - b'0'),
            b'a'..=b'f' => u32::from(b - b'a') + 10,
            b'A'..=b'F' => u32::from(b - b'A') + 10,
            _ => return Err(format!("invalid hex character: 0x{:02X}", b)),
        };
        val = val.checked_shl(4).unwrap_or(0) | nibble;
    }
    Ok(val)
}

// ============================================================================
// Padding helpers
// ============================================================================

/// Calculate the number of padding bytes needed to align `offset` to `ALIGNMENT`.
fn pad_to_alignment(offset: usize) -> usize {
    let remainder = offset % ALIGNMENT;
    if remainder == 0 {
        0
    } else {
        ALIGNMENT - remainder
    }
}

// ============================================================================
// Header serialization / deserialization
// ============================================================================

/// Serialize a newc header into 110 bytes.
fn serialize_header(hdr: &CpioHeader) -> [u8; HEADER_LEN] {
    let mut buf = [0u8; HEADER_LEN];

    // Magic: "070701" (6 bytes)
    buf[0..6].copy_from_slice(NEWC_MAGIC.as_bytes());

    // 13 fields, each 8 hex characters.
    let fields = [
        hdr.ino,
        hdr.mode,
        hdr.uid,
        hdr.gid,
        hdr.nlink,
        hdr.mtime,
        hdr.filesize,
        hdr.devmajor,
        hdr.devminor,
        hdr.rdevmajor,
        hdr.rdevminor,
        hdr.namesize,
        hdr.checksum,
    ];

    for (idx, &field_val) in fields.iter().enumerate() {
        let offset = 6 + idx * 8;
        buf[offset..offset + 8].copy_from_slice(&encode_hex8(field_val));
    }

    buf
}

/// Parse a 110-byte buffer into a CpioHeader.
fn parse_header(buf: &[u8]) -> Result<CpioHeader, String> {
    if buf.len() < HEADER_LEN {
        return Err(format!(
            "header too short: {} bytes (need {})",
            buf.len(),
            HEADER_LEN
        ));
    }

    let magic = std::str::from_utf8(&buf[0..6]).map_err(|_| "invalid magic bytes")?;
    if magic != NEWC_MAGIC {
        return Err(format!(
            "bad magic: expected '{}', got '{}'",
            NEWC_MAGIC, magic
        ));
    }

    Ok(CpioHeader {
        ino: decode_hex8(&buf[6..14])?,
        mode: decode_hex8(&buf[14..22])?,
        uid: decode_hex8(&buf[22..30])?,
        gid: decode_hex8(&buf[30..38])?,
        nlink: decode_hex8(&buf[38..46])?,
        mtime: decode_hex8(&buf[46..54])?,
        filesize: decode_hex8(&buf[54..62])?,
        devmajor: decode_hex8(&buf[62..70])?,
        devminor: decode_hex8(&buf[70..78])?,
        rdevmajor: decode_hex8(&buf[78..86])?,
        rdevminor: decode_hex8(&buf[86..94])?,
        namesize: decode_hex8(&buf[94..102])?,
        checksum: decode_hex8(&buf[102..110])?,
    })
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

/// Check if a filename matches any of the given patterns.
/// An empty pattern list means "match everything".
fn matches_patterns(filename: &str, patterns: &[String]) -> bool {
    if patterns.is_empty() {
        return true;
    }
    for pat in patterns {
        if glob_matches(pat, filename) {
            return true;
        }
        // Also try matching against just the final component.
        if let Some(basename) = Path::new(filename).file_name()
            && let Some(name_str) = basename.to_str()
                && glob_matches(pat, name_str) {
                    return true;
                }
    }
    false
}

// ============================================================================
// Metadata helpers
// ============================================================================

/// Get the Unix mode bits from file metadata.
#[cfg(unix)]
fn get_mode(meta: &Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    meta.mode()
}

#[cfg(not(unix))]
fn get_mode(meta: &Metadata) -> u32 {
    if meta.is_dir() {
        S_IFDIR | 0o755
    } else if meta.is_symlink() {
        S_IFLNK | 0o777
    } else if meta.permissions().readonly() {
        S_IFREG | 0o444
    } else {
        S_IFREG | 0o644
    }
}

/// Get the modification time as seconds since epoch.
fn get_mtime(meta: &Metadata) -> u32 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0)
}

/// Get inode number.
#[cfg(unix)]
fn get_ino(meta: &Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    meta.ino() as u32
}

#[cfg(not(unix))]
fn get_ino(_meta: &Metadata) -> u32 {
    0
}

/// Get nlink count.
#[cfg(unix)]
fn get_nlink(meta: &Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    meta.nlink() as u32
}

#[cfg(not(unix))]
fn get_nlink(meta: &Metadata) -> u32 {
    if meta.is_dir() {
        2
    } else {
        1
    }
}

/// Get uid.
#[cfg(unix)]
fn get_uid(meta: &Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    meta.uid()
}

#[cfg(not(unix))]
fn get_uid(_meta: &Metadata) -> u32 {
    0
}

/// Get gid.
#[cfg(unix)]
fn get_gid(meta: &Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    meta.gid()
}

#[cfg(not(unix))]
fn get_gid(_meta: &Metadata) -> u32 {
    0
}

/// Get device major number.
#[cfg(unix)]
fn get_devmajor(meta: &Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    // major = dev >> 8 (simplified; real major/minor extraction varies)
    (meta.dev() >> 8) as u32 & 0xFF
}

#[cfg(not(unix))]
fn get_devmajor(_meta: &Metadata) -> u32 {
    0
}

/// Get device minor number.
#[cfg(unix)]
fn get_devminor(meta: &Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    meta.dev() as u32 & 0xFF
}

#[cfg(not(unix))]
fn get_devminor(_meta: &Metadata) -> u32 {
    0
}

// ============================================================================
// Symlink target reading
// ============================================================================

/// Read the target of a symlink. Returns the target as bytes.
fn read_symlink_target(path: &Path) -> Result<Vec<u8>, String> {
    let target = fs::read_link(path)
        .map_err(|e| format!("read symlink '{}': {}", path.display(), e))?;
    Ok(target
        .to_str()
        .unwrap_or("")
        .as_bytes()
        .to_vec())
}

// ============================================================================
// Permission string for verbose listing
// ============================================================================

/// Format a cpio mode into a type+permission string like `drwxr-xr-x`.
fn format_permissions(mode: u32) -> String {
    let mut perms = [b'-'; 10];

    // File type character.
    let ftype = mode & S_IFMT;
    perms[0] = match ftype {
        S_IFDIR => b'd',
        S_IFLNK => b'l',
        S_IFREG => b'-',
        _ => b'?',
    };

    // Owner.
    if mode & 0o400 != 0 { perms[1] = b'r'; }
    if mode & 0o200 != 0 { perms[2] = b'w'; }
    if mode & 0o100 != 0 { perms[3] = b'x'; }
    // Group.
    if mode & 0o040 != 0 { perms[4] = b'r'; }
    if mode & 0o020 != 0 { perms[5] = b'w'; }
    if mode & 0o010 != 0 { perms[6] = b'x'; }
    // Other.
    if mode & 0o004 != 0 { perms[7] = b'r'; }
    if mode & 0o002 != 0 { perms[8] = b'w'; }
    if mode & 0o001 != 0 { perms[9] = b'x'; }

    String::from_utf8(perms.to_vec()).unwrap_or_else(|_| "----------".to_string())
}

// ============================================================================
// Strip leading slash
// ============================================================================

/// Remove leading '/' from a path if `--no-absolute-filenames` is set.
fn strip_leading_slash(path: &str) -> &str {
    path.strip_prefix('/').unwrap_or(path)
}

// ============================================================================
// COPY-OUT mode (create archive)
// ============================================================================

/// Build a cpio entry for a file.
fn build_entry(filepath: &str) -> Result<CpioEntry, String> {
    let path = Path::new(filepath);
    let symlink_meta = fs::symlink_metadata(path)
        .map_err(|e| format!("stat '{}': {}", filepath, e))?;

    let is_symlink = symlink_meta.is_symlink();

    let file_data = if is_symlink {
        read_symlink_target(path)?
    } else if symlink_meta.is_file() {
        fs::read(path).map_err(|e| format!("read '{}': {}", filepath, e))?
    } else {
        Vec::new()
    };

    // The filename stored in the archive: strip leading "./" for cleanliness
    // but preserve the path otherwise.
    let stored_name = if let Some(stripped) = filepath.strip_prefix("./") {
        if stripped.is_empty() { "." } else { stripped }
    } else {
        filepath
    };

    // namesize includes the NUL terminator.
    let namesize = (stored_name.len() + 1) as u32;

    let header = CpioHeader {
        ino: get_ino(&symlink_meta),
        mode: get_mode(&symlink_meta),
        uid: get_uid(&symlink_meta),
        gid: get_gid(&symlink_meta),
        nlink: get_nlink(&symlink_meta),
        mtime: get_mtime(&symlink_meta),
        filesize: file_data.len() as u32,
        devmajor: get_devmajor(&symlink_meta),
        devminor: get_devminor(&symlink_meta),
        rdevmajor: 0,
        rdevminor: 0,
        namesize,
        checksum: 0,
    };

    Ok(CpioEntry {
        header,
        filename: stored_name.to_string(),
        data: file_data,
    })
}

/// Build the trailer entry that marks end of archive.
fn build_trailer() -> CpioEntry {
    let namesize = (TRAILER_NAME.len() + 1) as u32;
    CpioEntry {
        header: CpioHeader {
            ino: 0,
            mode: 0,
            uid: 0,
            gid: 0,
            nlink: 1,
            mtime: 0,
            filesize: 0,
            devmajor: 0,
            devminor: 0,
            rdevmajor: 0,
            rdevminor: 0,
            namesize,
            checksum: 0,
        },
        filename: TRAILER_NAME.to_string(),
        data: Vec::new(),
    }
}

/// Write a single cpio entry (header + name + padding + data + padding) to the
/// writer. Returns the number of bytes written.
fn write_entry<W: Write>(writer: &mut W, entry: &CpioEntry) -> Result<usize, String> {
    let mut bytes_written: usize = 0;

    // Serialize and write header.
    let header_bytes = serialize_header(&entry.header);
    writer
        .write_all(&header_bytes)
        .map_err(|e| format!("write header: {}", e))?;
    bytes_written += HEADER_LEN;

    // Write filename + NUL.
    writer
        .write_all(entry.filename.as_bytes())
        .map_err(|e| format!("write filename: {}", e))?;
    writer
        .write_all(&[0])
        .map_err(|e| format!("write NUL: {}", e))?;
    bytes_written += entry.filename.len() + 1;

    // Pad filename to 4-byte boundary (relative to start of header).
    let name_total = HEADER_LEN + entry.filename.len() + 1;
    let name_pad = pad_to_alignment(name_total);
    if name_pad > 0 {
        let zeros = [0u8; 4];
        writer
            .write_all(&zeros[..name_pad])
            .map_err(|e| format!("write name padding: {}", e))?;
        bytes_written += name_pad;
    }

    // Write file data.
    if !entry.data.is_empty() {
        writer
            .write_all(&entry.data)
            .map_err(|e| format!("write data: {}", e))?;
        bytes_written += entry.data.len();

        // Pad data to 4-byte boundary.
        let data_pad = pad_to_alignment(entry.data.len());
        if data_pad > 0 {
            let zeros = [0u8; 4];
            writer
                .write_all(&zeros[..data_pad])
                .map_err(|e| format!("write data padding: {}", e))?;
            bytes_written += data_pad;
        }
    }

    Ok(bytes_written)
}

/// Create a cpio archive from file paths read from stdin.
fn copy_out(opts: &Options) -> Result<(), String> {
    // Determine output: -F, -O, or stdout.
    let output_path = opts
        .archive_file
        .as_ref()
        .or(opts.output_file.as_ref());

    let mut writer: Box<dyn Write> = if let Some(path) = output_path {
        Box::new(
            File::create(path).map_err(|e| format!("cannot create '{}': {}", path, e))?,
        )
    } else {
        Box::new(io::stdout().lock())
    };

    let stdin = io::stdin();
    let reader = stdin.lock();

    let mut total_bytes: usize = 0;
    let mut errors: Vec<String> = Vec::new();

    for line_result in reader.lines() {
        let line = line_result.map_err(|e| format!("read stdin: {}", e))?;
        let filepath = line.trim();
        if filepath.is_empty() {
            continue;
        }

        match build_entry(filepath) {
            Ok(entry) => {
                if opts.verbose {
                    eprintln!("{}", entry.filename);
                }
                match write_entry(&mut writer, &entry) {
                    Ok(n) => total_bytes += n,
                    Err(e) => {
                        let msg = format!("cpio: {}: {}", filepath, e);
                        eprintln!("{}", msg);
                        errors.push(msg);
                    }
                }
            }
            Err(e) => {
                let msg = format!("cpio: {}", e);
                eprintln!("{}", msg);
                errors.push(msg);
            }
        }
    }

    // Write trailer.
    let trailer = build_trailer();
    match write_entry(&mut writer, &trailer) {
        Ok(n) => total_bytes += n,
        Err(e) => return Err(format!("write trailer: {}", e)),
    }

    writer
        .flush()
        .map_err(|e| format!("flush output: {}", e))?;

    if !opts.quiet {
        let blocks = total_bytes.div_ceil(BLOCK_SIZE);
        eprintln!("{} blocks", blocks);
    }

    if !errors.is_empty() {
        return Err(format!("cpio: completed with {} error(s)", errors.len()));
    }

    Ok(())
}

// ============================================================================
// COPY-IN mode (extract archive)
// ============================================================================

/// Read exactly `n` bytes from the reader.
fn read_exact<R: Read>(reader: &mut R, n: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; n];
    reader
        .read_exact(&mut buf)
        .map_err(|e| format!("read error: {}", e))?;
    Ok(buf)
}

/// Read the next cpio entry from the reader. Returns None at the trailer.
fn read_entry<R: Read>(reader: &mut R, total_bytes: &mut usize) -> Result<Option<CpioEntry>, String> {
    // Read header.
    let header_buf = read_exact(reader, HEADER_LEN)?;
    *total_bytes += HEADER_LEN;

    let header = parse_header(&header_buf)?;

    // Read filename (namesize includes the NUL terminator).
    let name_buf = read_exact(reader, header.namesize as usize)?;
    *total_bytes += header.namesize as usize;

    // Strip NUL terminator.
    let filename = {
        let end = name_buf
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(name_buf.len());
        String::from_utf8_lossy(&name_buf[..end]).into_owned()
    };

    // Skip name padding.
    let name_total = HEADER_LEN + header.namesize as usize;
    let name_pad = pad_to_alignment(name_total);
    if name_pad > 0 {
        let _ = read_exact(reader, name_pad)?;
        *total_bytes += name_pad;
    }

    // Check for trailer.
    if filename == TRAILER_NAME {
        // Still need to skip any data (should be 0) and padding.
        if header.filesize > 0 {
            let skip_total = header.filesize as usize + pad_to_alignment(header.filesize as usize);
            let _ = read_exact(reader, skip_total)?;
            *total_bytes += skip_total;
        }
        return Ok(None);
    }

    // Read file data.
    let data = if header.filesize > 0 {
        let d = read_exact(reader, header.filesize as usize)?;
        *total_bytes += header.filesize as usize;

        // Skip data padding.
        let data_pad = pad_to_alignment(header.filesize as usize);
        if data_pad > 0 {
            let _ = read_exact(reader, data_pad)?;
            *total_bytes += data_pad;
        }
        d
    } else {
        Vec::new()
    };

    Ok(Some(CpioEntry {
        header,
        filename,
        data,
    }))
}

/// Set file permissions.
#[cfg(unix)]
fn set_permissions(path: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(mode & 0o7777);
    fs::set_permissions(path, perms)
        .map_err(|e| format!("set permissions on '{}': {}", path.display(), e))
}

#[cfg(not(unix))]
fn set_permissions(_path: &Path, _mode: u32) -> Result<(), String> {
    Ok(())
}

/// Set file modification time.
fn set_mtime(path: &Path, mtime: u32) -> Result<(), String> {
    use std::time::{Duration, UNIX_EPOCH};
    let time = UNIX_EPOCH + Duration::from_secs(u64::from(mtime));
    // std::fs::File does not directly support setting mtime on all platforms.
    // Use filetime-like approach via the File metadata.
    let file = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|e| format!("open '{}' for mtime: {}", path.display(), e))?;
    file.set_modified(time)
        .map_err(|e| format!("set mtime on '{}': {}", path.display(), e))
}

/// Extract or list the cpio archive.
fn copy_in(opts: &Options) -> Result<(), String> {
    // Determine input source: -F, -I, or stdin.
    let input_path = opts
        .archive_file
        .as_ref()
        .or(opts.input_file.as_ref());

    let mut reader: Box<dyn Read> = if let Some(path) = input_path {
        Box::new(
            File::open(path).map_err(|e| format!("cannot open '{}': {}", path, e))?,
        )
    } else {
        Box::new(io::stdin().lock())
    };

    let mut total_bytes: usize = 0;
    let mut errors: Vec<String> = Vec::new();

    loop {
        let entry = match read_entry(&mut reader, &mut total_bytes) {
            Ok(Some(e)) => e,
            Ok(None) => break, // trailer reached
            Err(e) => return Err(e),
        };

        let filename = if opts.no_absolute_filenames {
            strip_leading_slash(&entry.filename).to_string()
        } else {
            entry.filename.clone()
        };

        // Apply pattern filters.
        if !matches_patterns(&filename, &opts.patterns) {
            continue;
        }

        // List-only mode.
        if opts.list_only {
            if opts.verbose {
                let perms = format_permissions(entry.header.mode);
                println!(
                    "{} {:>3} {:>5} {:>5} {:>8} {} {}",
                    perms,
                    entry.header.nlink,
                    entry.header.uid,
                    entry.header.gid,
                    entry.header.filesize,
                    format_mtime(entry.header.mtime),
                    filename
                );
            } else {
                println!("{}", filename);
            }
            continue;
        }

        // Extract the entry.
        if opts.verbose {
            eprintln!("{}", filename);
        }

        let file_type = entry.header.mode & S_IFMT;
        let dest = PathBuf::from(&filename);

        match file_type {
            S_IFDIR => {
                if opts.make_directories || filename == "." {
                    if let Err(e) = fs::create_dir_all(&dest) {
                        let msg = format!("cpio: mkdir '{}': {}", dest.display(), e);
                        eprintln!("{}", msg);
                        errors.push(msg);
                    } else {
                        // Ignore permission errors during extraction since we
                        // may not have the required privileges.
                        let _ = set_permissions(&dest, entry.header.mode);
                    }
                }
            }
            S_IFREG | 0 => {
                // Regular file (mode 0 is treated as regular for compat).
                // Create parent directories if -d is set.
                if opts.make_directories
                    && let Some(parent) = dest.parent()
                        && !parent.as_os_str().is_empty() && !parent.exists()
                            && let Err(e) = fs::create_dir_all(parent) {
                                let msg = format!(
                                    "cpio: mkdir '{}': {}",
                                    parent.display(),
                                    e
                                );
                                eprintln!("{}", msg);
                                errors.push(msg);
                                continue;
                            }

                // Check if file exists and -u is not set.
                if dest.exists() && !opts.unconditional {
                    // Only overwrite if archive entry is newer.
                    if let Ok(existing_meta) = fs::metadata(&dest) {
                        let existing_mtime = get_mtime(&existing_meta);
                        if entry.header.mtime <= existing_mtime {
                            continue; // skip: existing is newer or same
                        }
                    }
                }

                match File::create(&dest) {
                    Ok(mut file) => {
                        if let Err(e) = file.write_all(&entry.data) {
                            let msg =
                                format!("cpio: write '{}': {}", dest.display(), e);
                            eprintln!("{}", msg);
                            errors.push(msg);
                        } else {
                            let _ = set_permissions(&dest, entry.header.mode);
                            if opts.preserve_mtime {
                                let _ = set_mtime(&dest, entry.header.mtime);
                            }
                        }
                    }
                    Err(e) => {
                        let msg =
                            format!("cpio: create '{}': {}", dest.display(), e);
                        eprintln!("{}", msg);
                        errors.push(msg);
                    }
                }
            }
            S_IFLNK => {
                // Symlink: data contains the target path.
                if opts.make_directories
                    && let Some(parent) = dest.parent()
                        && !parent.as_os_str().is_empty() && !parent.exists() {
                            let _ = fs::create_dir_all(parent);
                        }

                let target = String::from_utf8_lossy(&entry.data).into_owned();
                #[cfg(unix)]
                {
                    // Remove existing symlink/file first if unconditional.
                    if opts.unconditional && dest.exists() {
                        let _ = fs::remove_file(&dest);
                    }
                    if let Err(e) = std::os::unix::fs::symlink(&target, &dest) {
                        let msg = format!(
                            "cpio: symlink '{}' -> '{}': {}",
                            dest.display(),
                            target,
                            e
                        );
                        eprintln!("{}", msg);
                        errors.push(msg);
                    }
                }
                #[cfg(not(unix))]
                {
                    let _ = target;
                    eprintln!(
                        "cpio: {}: symlink extraction not supported on this platform",
                        dest.display()
                    );
                }
            }
            _ => {
                eprintln!(
                    "cpio: {}: unsupported file type 0o{:06o}, skipping",
                    filename,
                    file_type
                );
            }
        }
    }

    if !opts.quiet {
        let blocks = total_bytes.div_ceil(BLOCK_SIZE);
        eprintln!("{} blocks", blocks);
    }

    if !errors.is_empty() {
        return Err(format!("cpio: completed with {} error(s)", errors.len()));
    }

    Ok(())
}

// ============================================================================
// PASS-THROUGH mode (copy files to a directory)
// ============================================================================

/// Copy files directly to a destination directory without creating an archive.
fn pass_through(opts: &Options) -> Result<(), String> {
    let dest_dir = opts
        .dest_dir
        .as_ref()
        .ok_or_else(|| "pass-through mode requires a destination directory".to_string())?;

    let dest_base = PathBuf::from(dest_dir);

    // Ensure destination exists.
    if !dest_base.exists() {
        if opts.make_directories {
            fs::create_dir_all(&dest_base)
                .map_err(|e| format!("mkdir '{}': {}", dest_base.display(), e))?;
        } else {
            return Err(format!(
                "destination '{}' does not exist (use -d to create)",
                dest_dir
            ));
        }
    }

    let stdin = io::stdin();
    let reader = stdin.lock();

    let mut errors: Vec<String> = Vec::new();

    for line_result in reader.lines() {
        let line = line_result.map_err(|e| format!("read stdin: {}", e))?;
        let filepath = line.trim();
        if filepath.is_empty() {
            continue;
        }

        let src = Path::new(filepath);
        let symlink_meta = match fs::symlink_metadata(src) {
            Ok(m) => m,
            Err(e) => {
                let msg = format!("cpio: stat '{}': {}", filepath, e);
                eprintln!("{}", msg);
                errors.push(msg);
                continue;
            }
        };

        // Compute destination path: dest_dir/filepath (stripping leading ./).
        let relative = if let Some(stripped) = filepath.strip_prefix("./") {
            stripped
        } else if let Some(stripped) = filepath.strip_prefix('/') {
            stripped
        } else {
            filepath
        };

        if relative.is_empty() || relative == "." {
            continue;
        }

        let dest = dest_base.join(relative);

        if opts.verbose {
            eprintln!("{}", filepath);
        }

        if symlink_meta.is_dir() {
            if opts.make_directories
                && let Err(e) = fs::create_dir_all(&dest) {
                    let msg = format!("cpio: mkdir '{}': {}", dest.display(), e);
                    eprintln!("{}", msg);
                    errors.push(msg);
                }
        } else if symlink_meta.is_symlink() {
            // Copy symlink.
            if let Some(parent) = dest.parent()
                && opts.make_directories && !parent.exists() {
                    let _ = fs::create_dir_all(parent);
                }
            match fs::read_link(src) {
                Ok(target) => {
                    #[cfg(unix)]
                    {
                        if dest.exists() {
                            if opts.unconditional {
                                let _ = fs::remove_file(&dest);
                            } else {
                                continue;
                            }
                        }
                        if let Err(e) = std::os::unix::fs::symlink(&target, &dest) {
                            let msg = format!(
                                "cpio: symlink '{}': {}",
                                dest.display(),
                                e
                            );
                            eprintln!("{}", msg);
                            errors.push(msg);
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = target;
                        eprintln!(
                            "cpio: {}: symlink copy not supported on this platform",
                            dest.display()
                        );
                    }
                }
                Err(e) => {
                    let msg = format!("cpio: readlink '{}': {}", src.display(), e);
                    eprintln!("{}", msg);
                    errors.push(msg);
                }
            }
        } else if symlink_meta.is_file() {
            // Copy regular file.
            if let Some(parent) = dest.parent()
                && opts.make_directories && !parent.exists()
                    && let Err(e) = fs::create_dir_all(parent) {
                        let msg = format!("cpio: mkdir '{}': {}", parent.display(), e);
                        eprintln!("{}", msg);
                        errors.push(msg);
                        continue;
                    }

            // Check unconditional / newer-than logic.
            if dest.exists() && !opts.unconditional
                && let Ok(existing_meta) = fs::metadata(&dest) {
                    let existing_mtime = get_mtime(&existing_meta);
                    let src_mtime = get_mtime(&symlink_meta);
                    if src_mtime <= existing_mtime {
                        continue;
                    }
                }

            match fs::copy(src, &dest) {
                Ok(_) => {
                    if opts.preserve_mtime {
                        let _ = set_mtime(&dest, get_mtime(&symlink_meta));
                    }
                }
                Err(e) => {
                    let msg = format!("cpio: copy '{}' -> '{}': {}", filepath, dest.display(), e);
                    eprintln!("{}", msg);
                    errors.push(msg);
                }
            }
        }
    }

    if !errors.is_empty() {
        return Err(format!(
            "cpio: completed with {} error(s)",
            errors.len()
        ));
    }

    Ok(())
}

// ============================================================================
// mtime formatting for verbose listing
// ============================================================================

/// Format a Unix timestamp for cpio verbose listing.
fn format_mtime(epoch_secs: u32) -> String {
    const DAYS_IN_MONTH: [[u32; 12]; 2] = [
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31],
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31],
    ];

    fn is_leap(y: u32) -> bool {
        (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
    }

    let mut remaining = epoch_secs;
    let secs = remaining % 60;
    remaining /= 60;
    let mins = remaining % 60;
    remaining /= 60;
    let hours = remaining % 24;
    let mut days = remaining / 24;

    let mut year: u32 = 1970;
    loop {
        let days_in_year: u32 = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let leap_idx = if is_leap(year) { 1usize } else { 0usize };
    let mut month: u32 = 0;
    while month < 12 && days >= DAYS_IN_MONTH[leap_idx][month as usize] {
        days -= DAYS_IN_MONTH[leap_idx][month as usize];
        month += 1;
    }
    let day = days + 1;
    month += 1;

    let _ = secs; // cpio traditionally shows month-day-hour:minute
    format!(
        "{:>3} {:>2} {:02}:{:02}",
        match month {
            1 => "Jan",
            2 => "Feb",
            3 => "Mar",
            4 => "Apr",
            5 => "May",
            6 => "Jun",
            7 => "Jul",
            8 => "Aug",
            9 => "Sep",
            10 => "Oct",
            11 => "Nov",
            12 => "Dec",
            _ => "???",
        },
        day,
        hours,
        mins
    )
}

// ============================================================================
// Usage / help
// ============================================================================

fn print_usage() {
    eprintln!(
        "\
Usage: cpio [OPTIONS]

Modes:
  -o, --create              Copy-out: create archive from stdin file list
  -i, --extract             Copy-in: extract from archive
  -p, --pass-through DIR    Copy files to DIR (no archive)

Options:
  -d, --make-directories              Create directories as needed
  -m, --preserve-modification-time    Preserve file modification times
  -u, --unconditional                 Overwrite without checking
  -v, --verbose                       Verbose output
  -t, --list                          List contents (with -i)
  -F FILE, --file=FILE                Use FILE instead of stdin/stdout
  -I FILE                             Input archive file
  -O FILE                             Output archive file
  --no-absolute-filenames             Strip leading / from paths
  --quiet                             Suppress block count messages

Archive format: newc (SVR4 new ASCII cpio, magic 070701)

Examples:
  find . -print | cpio -o -F archive.cpio      Create archive
  cpio -i -d -F archive.cpio                   Extract with dirs
  cpio -i -t -v -F archive.cpio                Verbose listing
  find . -print | cpio -p -d /dest             Copy to /dest"
    );
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 || args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        if args.len() < 2 {
            process::exit(1);
        }
        process::exit(0);
    }

    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("cpio: {}", e);
            print_usage();
            process::exit(2);
        }
    };

    let result = match opts.mode {
        Mode::CopyOut => copy_out(&opts),
        Mode::CopyIn => copy_in(&opts),
        Mode::PassThrough => pass_through(&opts),
    };

    if let Err(e) = result {
        eprintln!("cpio: {}", e);
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Hex encoding / decoding --

    #[test]
    fn test_encode_hex8_zero() {
        assert_eq!(&encode_hex8(0), b"00000000");
    }

    #[test]
    fn test_encode_hex8_small() {
        assert_eq!(&encode_hex8(0x1A), b"0000001A");
    }

    #[test]
    fn test_encode_hex8_large() {
        assert_eq!(&encode_hex8(0xDEADBEEF), b"DEADBEEF");
    }

    #[test]
    fn test_encode_hex8_max() {
        assert_eq!(&encode_hex8(0xFFFFFFFF), b"FFFFFFFF");
    }

    #[test]
    fn test_decode_hex8_zero() {
        assert_eq!(decode_hex8(b"00000000").unwrap(), 0);
    }

    #[test]
    fn test_decode_hex8_small() {
        assert_eq!(decode_hex8(b"0000001A").unwrap(), 0x1A);
    }

    #[test]
    fn test_decode_hex8_lowercase() {
        assert_eq!(decode_hex8(b"deadbeef").unwrap(), 0xDEADBEEF);
    }

    #[test]
    fn test_decode_hex8_mixed_case() {
        assert_eq!(decode_hex8(b"DeAdBeEf").unwrap(), 0xDEADBEEF);
    }

    #[test]
    fn test_decode_hex8_too_short() {
        assert!(decode_hex8(b"00").is_err());
    }

    #[test]
    fn test_decode_hex8_invalid_char() {
        assert!(decode_hex8(b"0000GGGG").is_err());
    }

    #[test]
    fn test_hex_roundtrip() {
        for val in [0u32, 1, 255, 0o100644, 0xCAFE, 0xFFFFFFFF] {
            let encoded = encode_hex8(val);
            let decoded = decode_hex8(&encoded).unwrap();
            assert_eq!(decoded, val, "roundtrip failed for {}", val);
        }
    }

    // -- Padding --

    #[test]
    fn test_pad_to_alignment_already_aligned() {
        assert_eq!(pad_to_alignment(0), 0);
        assert_eq!(pad_to_alignment(4), 0);
        assert_eq!(pad_to_alignment(8), 0);
    }

    #[test]
    fn test_pad_to_alignment_needs_padding() {
        assert_eq!(pad_to_alignment(1), 3);
        assert_eq!(pad_to_alignment(2), 2);
        assert_eq!(pad_to_alignment(3), 1);
        assert_eq!(pad_to_alignment(5), 3);
        assert_eq!(pad_to_alignment(110), 2); // header size
    }

    // -- Header serialization / parsing --

    #[test]
    fn test_serialize_header_magic() {
        let hdr = CpioHeader {
            ino: 0,
            mode: 0,
            uid: 0,
            gid: 0,
            nlink: 0,
            mtime: 0,
            filesize: 0,
            devmajor: 0,
            devminor: 0,
            rdevmajor: 0,
            rdevminor: 0,
            namesize: 0,
            checksum: 0,
        };
        let buf = serialize_header(&hdr);
        assert_eq!(&buf[0..6], NEWC_MAGIC.as_bytes());
        assert_eq!(buf.len(), HEADER_LEN);
    }

    #[test]
    fn test_serialize_header_fields() {
        let hdr = CpioHeader {
            ino: 0x12345,
            mode: 0o100644,
            uid: 1000,
            gid: 1000,
            nlink: 1,
            mtime: 0x60000000,
            filesize: 0x100,
            devmajor: 8,
            devminor: 1,
            rdevmajor: 0,
            rdevminor: 0,
            namesize: 9,
            checksum: 0,
        };
        let buf = serialize_header(&hdr);

        // Verify magic.
        assert_eq!(&buf[0..6], b"070701");
        // Verify ino field (bytes 6..14).
        assert_eq!(&buf[6..14], b"00012345");
        // Verify mode field (bytes 14..22): 0o100644 = 0x81A4.
        assert_eq!(&buf[14..22], b"000081A4");
    }

    #[test]
    fn test_header_roundtrip() {
        let original = CpioHeader {
            ino: 999,
            mode: S_IFREG | 0o755,
            uid: 500,
            gid: 500,
            nlink: 2,
            mtime: 1700000000,
            filesize: 4096,
            devmajor: 8,
            devminor: 3,
            rdevmajor: 0,
            rdevminor: 0,
            namesize: 10,
            checksum: 0,
        };
        let buf = serialize_header(&original);
        let parsed = parse_header(&buf).unwrap();

        assert_eq!(parsed.ino, original.ino);
        assert_eq!(parsed.mode, original.mode);
        assert_eq!(parsed.uid, original.uid);
        assert_eq!(parsed.gid, original.gid);
        assert_eq!(parsed.nlink, original.nlink);
        assert_eq!(parsed.mtime, original.mtime);
        assert_eq!(parsed.filesize, original.filesize);
        assert_eq!(parsed.devmajor, original.devmajor);
        assert_eq!(parsed.devminor, original.devminor);
        assert_eq!(parsed.rdevmajor, original.rdevmajor);
        assert_eq!(parsed.rdevminor, original.rdevminor);
        assert_eq!(parsed.namesize, original.namesize);
        assert_eq!(parsed.checksum, original.checksum);
    }

    #[test]
    fn test_parse_header_bad_magic() {
        let mut buf = [0u8; HEADER_LEN];
        buf[0..6].copy_from_slice(b"BADMAG");
        assert!(parse_header(&buf).is_err());
    }

    #[test]
    fn test_parse_header_too_short() {
        let buf = [0u8; 50];
        assert!(parse_header(&buf).is_err());
    }

    // -- Glob pattern matching --

    #[test]
    fn test_glob_exact() {
        assert!(glob_matches("hello.txt", "hello.txt"));
        assert!(!glob_matches("hello.txt", "world.txt"));
    }

    #[test]
    fn test_glob_star() {
        assert!(glob_matches("*.txt", "readme.txt"));
        assert!(glob_matches("*.txt", ".txt"));
        assert!(!glob_matches("*.txt", "readme.rs"));
    }

    #[test]
    fn test_glob_question() {
        assert!(glob_matches("?.txt", "a.txt"));
        assert!(!glob_matches("?.txt", "ab.txt"));
    }

    #[test]
    fn test_glob_complex() {
        assert!(glob_matches("src/*.rs", "src/main.rs"));
        assert!(!glob_matches("src/*.rs", "lib/main.rs"));
        assert!(glob_matches("*test*", "my_test_file"));
    }

    #[test]
    fn test_glob_empty() {
        assert!(glob_matches("", ""));
        assert!(!glob_matches("", "x"));
        assert!(glob_matches("*", ""));
        assert!(glob_matches("*", "anything"));
    }

    // -- Pattern matching --

    #[test]
    fn test_matches_patterns_empty_list() {
        assert!(matches_patterns("anything.txt", &[]));
    }

    #[test]
    fn test_matches_patterns_matching() {
        let patterns = vec!["*.txt".to_string()];
        assert!(matches_patterns("readme.txt", &patterns));
        assert!(!matches_patterns("readme.rs", &patterns));
    }

    #[test]
    fn test_matches_patterns_basename() {
        let patterns = vec!["*.txt".to_string()];
        assert!(matches_patterns("src/readme.txt", &patterns));
    }

    // -- Permission formatting --

    #[test]
    fn test_format_permissions_regular_644() {
        let s = format_permissions(S_IFREG | 0o644);
        assert_eq!(s, "-rw-r--r--");
    }

    #[test]
    fn test_format_permissions_dir_755() {
        let s = format_permissions(S_IFDIR | 0o755);
        assert_eq!(s, "drwxr-xr-x");
    }

    #[test]
    fn test_format_permissions_symlink() {
        let s = format_permissions(S_IFLNK | 0o777);
        assert_eq!(s, "lrwxrwxrwx");
    }

    #[test]
    fn test_format_permissions_zero() {
        let s = format_permissions(S_IFREG);
        assert_eq!(s, "----------");
    }

    // -- Strip leading slash --

    #[test]
    fn test_strip_leading_slash_with_slash() {
        assert_eq!(strip_leading_slash("/usr/bin/foo"), "usr/bin/foo");
    }

    #[test]
    fn test_strip_leading_slash_without_slash() {
        assert_eq!(strip_leading_slash("usr/bin/foo"), "usr/bin/foo");
    }

    #[test]
    fn test_strip_leading_slash_just_slash() {
        assert_eq!(strip_leading_slash("/"), "");
    }

    // -- mtime formatting --

    #[test]
    fn test_format_mtime_epoch() {
        let s = format_mtime(0);
        assert!(s.contains("Jan"));
        assert!(s.contains("1"));
    }

    #[test]
    fn test_format_mtime_known_date() {
        // 2024-01-01 00:00:00 UTC = 1704067200
        let s = format_mtime(1704067200);
        assert!(s.contains("Jan"));
        assert!(s.contains("1"));
    }

    // -- Write entry to buffer --

    #[test]
    fn test_write_entry_trailer() {
        let trailer = build_trailer();
        let mut buf = Vec::new();
        let n = write_entry(&mut buf, &trailer).unwrap();
        assert!(n > 0);
        // Should start with magic.
        assert_eq!(&buf[0..6], NEWC_MAGIC.as_bytes());
        // Filename should be TRAILER!!!.
        let name_start = HEADER_LEN;
        let name_end = name_start + TRAILER_NAME.len();
        assert_eq!(
            std::str::from_utf8(&buf[name_start..name_end]).unwrap(),
            TRAILER_NAME
        );
    }

    #[test]
    fn test_write_entry_alignment() {
        // Build a fake entry with known data.
        let entry = CpioEntry {
            header: CpioHeader {
                ino: 1,
                mode: S_IFREG | 0o644,
                uid: 0,
                gid: 0,
                nlink: 1,
                mtime: 0,
                filesize: 5,
                devmajor: 0,
                devminor: 0,
                rdevmajor: 0,
                rdevminor: 0,
                namesize: 4, // "foo" + NUL
                checksum: 0,
            },
            filename: "foo".to_string(),
            data: vec![b'H', b'e', b'l', b'l', b'o'],
        };

        let mut buf = Vec::new();
        let n = write_entry(&mut buf, &entry).unwrap();

        // Total bytes must be 4-byte aligned at each section boundary.
        // Header (110) + name ("foo\0" = 4) = 114, pad to 116.
        // Data (5) padded to 8.
        // Total: 116 + 8 = 124.
        assert_eq!(n, 124);
        assert_eq!(buf.len(), 124);
    }

    // -- Archive read/write roundtrip --

    #[test]
    fn test_archive_roundtrip() {
        // Build an entry with data.
        let entry = CpioEntry {
            header: CpioHeader {
                ino: 42,
                mode: S_IFREG | 0o644,
                uid: 1000,
                gid: 1000,
                nlink: 1,
                mtime: 1700000000,
                filesize: 13,
                devmajor: 0,
                devminor: 0,
                rdevmajor: 0,
                rdevminor: 0,
                namesize: 9, // "test.txt" + NUL
                checksum: 0,
            },
            filename: "test.txt".to_string(),
            data: b"Hello, world!".to_vec(),
        };

        let trailer = build_trailer();

        // Write archive to buffer.
        let mut archive = Vec::new();
        write_entry(&mut archive, &entry).unwrap();
        write_entry(&mut archive, &trailer).unwrap();

        // Read it back.
        let mut cursor = io::Cursor::new(&archive);
        let mut total = 0usize;
        let read_entry_result = read_entry(&mut cursor, &mut total).unwrap();
        assert!(read_entry_result.is_some());

        let read_back = read_entry_result.unwrap();
        assert_eq!(read_back.filename, "test.txt");
        assert_eq!(read_back.data, b"Hello, world!");
        assert_eq!(read_back.header.mode, S_IFREG | 0o644);
        assert_eq!(read_back.header.uid, 1000);
        assert_eq!(read_back.header.mtime, 1700000000);

        // Next read should be the trailer (None).
        let trailer_result = read_entry(&mut cursor, &mut total).unwrap();
        assert!(trailer_result.is_none());
    }

    #[test]
    fn test_archive_multiple_entries() {
        let entries = vec![
            CpioEntry {
                header: CpioHeader {
                    ino: 1,
                    mode: S_IFDIR | 0o755,
                    uid: 0,
                    gid: 0,
                    nlink: 2,
                    mtime: 1000,
                    filesize: 0,
                    devmajor: 0,
                    devminor: 0,
                    rdevmajor: 0,
                    rdevminor: 0,
                    namesize: 4, // "dir" + NUL
                    checksum: 0,
                },
                filename: "dir".to_string(),
                data: Vec::new(),
            },
            CpioEntry {
                header: CpioHeader {
                    ino: 2,
                    mode: S_IFREG | 0o644,
                    uid: 0,
                    gid: 0,
                    nlink: 1,
                    mtime: 2000,
                    filesize: 3,
                    devmajor: 0,
                    devminor: 0,
                    rdevmajor: 0,
                    rdevminor: 0,
                    namesize: 12, // "dir/file.txt" + NUL
                    checksum: 0,
                },
                filename: "dir/file.txt".to_string(),
                data: b"abc".to_vec(),
            },
        ];

        let trailer = build_trailer();
        let mut archive = Vec::new();
        for entry in &entries {
            write_entry(&mut archive, entry).unwrap();
        }
        write_entry(&mut archive, &trailer).unwrap();

        // Read back.
        let mut cursor = io::Cursor::new(&archive);
        let mut total = 0usize;

        let e1 = read_entry(&mut cursor, &mut total).unwrap().unwrap();
        assert_eq!(e1.filename, "dir");
        assert_eq!(e1.header.mode & S_IFMT, S_IFDIR);
        assert!(e1.data.is_empty());

        let e2 = read_entry(&mut cursor, &mut total).unwrap().unwrap();
        assert_eq!(e2.filename, "dir/file.txt");
        assert_eq!(e2.data, b"abc");

        let end = read_entry(&mut cursor, &mut total).unwrap();
        assert!(end.is_none());
    }

    #[test]
    fn test_archive_empty_file() {
        let entry = CpioEntry {
            header: CpioHeader {
                ino: 10,
                mode: S_IFREG | 0o644,
                uid: 0,
                gid: 0,
                nlink: 1,
                mtime: 0,
                filesize: 0,
                devmajor: 0,
                devminor: 0,
                rdevmajor: 0,
                rdevminor: 0,
                namesize: 10, // "empty.txt" + NUL
                checksum: 0,
            },
            filename: "empty.txt".to_string(),
            data: Vec::new(),
        };

        let trailer = build_trailer();
        let mut archive = Vec::new();
        write_entry(&mut archive, &entry).unwrap();
        write_entry(&mut archive, &trailer).unwrap();

        let mut cursor = io::Cursor::new(&archive);
        let mut total = 0usize;
        let e = read_entry(&mut cursor, &mut total).unwrap().unwrap();
        assert_eq!(e.filename, "empty.txt");
        assert!(e.data.is_empty());
        assert_eq!(e.header.filesize, 0);
    }

    // -- Build trailer --

    #[test]
    fn test_build_trailer_name() {
        let t = build_trailer();
        assert_eq!(t.filename, TRAILER_NAME);
        assert_eq!(t.header.namesize, (TRAILER_NAME.len() + 1) as u32);
        assert_eq!(t.header.filesize, 0);
    }

    // -- Verbose listing format --

    #[test]
    fn test_verbose_list_format() {
        // Indirectly test by checking format_permissions and format_mtime
        // produce non-empty strings used in the listing.
        let perms = format_permissions(S_IFREG | 0o644);
        assert_eq!(perms.len(), 10);
        assert!(perms.starts_with('-'));

        let mtime = format_mtime(1704067200);
        assert!(!mtime.is_empty());
    }

    // -- Symlink entry --

    #[test]
    fn test_symlink_entry_roundtrip() {
        let target = b"/usr/bin/python3";
        let entry = CpioEntry {
            header: CpioHeader {
                ino: 100,
                mode: S_IFLNK | 0o777,
                uid: 0,
                gid: 0,
                nlink: 1,
                mtime: 5000,
                filesize: target.len() as u32,
                devmajor: 0,
                devminor: 0,
                rdevmajor: 0,
                rdevminor: 0,
                namesize: 7, // "python" + NUL
                checksum: 0,
            },
            filename: "python".to_string(),
            data: target.to_vec(),
        };

        let trailer = build_trailer();
        let mut archive = Vec::new();
        write_entry(&mut archive, &entry).unwrap();
        write_entry(&mut archive, &trailer).unwrap();

        let mut cursor = io::Cursor::new(&archive);
        let mut total = 0usize;
        let e = read_entry(&mut cursor, &mut total).unwrap().unwrap();
        assert_eq!(e.filename, "python");
        assert_eq!(e.header.mode & S_IFMT, S_IFLNK);
        assert_eq!(e.data, target);
    }

    // -- Large file size --

    #[test]
    fn test_large_filesize_header() {
        let hdr = CpioHeader {
            ino: 1,
            mode: S_IFREG | 0o644,
            uid: 0,
            gid: 0,
            nlink: 1,
            mtime: 0,
            filesize: 0xFFFFFFFF,
            devmajor: 0,
            devminor: 0,
            rdevmajor: 0,
            rdevminor: 0,
            namesize: 5,
            checksum: 0,
        };
        let buf = serialize_header(&hdr);
        let parsed = parse_header(&buf).unwrap();
        assert_eq!(parsed.filesize, 0xFFFFFFFF);
    }

    // -- Long filename --

    #[test]
    fn test_long_filename_entry() {
        let long_name = "a".repeat(200);
        let namesize = (long_name.len() + 1) as u32;
        let entry = CpioEntry {
            header: CpioHeader {
                ino: 5,
                mode: S_IFREG | 0o644,
                uid: 0,
                gid: 0,
                nlink: 1,
                mtime: 0,
                filesize: 0,
                devmajor: 0,
                devminor: 0,
                rdevmajor: 0,
                rdevminor: 0,
                namesize,
                checksum: 0,
            },
            filename: long_name.clone(),
            data: Vec::new(),
        };

        let trailer = build_trailer();
        let mut archive = Vec::new();
        write_entry(&mut archive, &entry).unwrap();
        write_entry(&mut archive, &trailer).unwrap();

        let mut cursor = io::Cursor::new(&archive);
        let mut total = 0usize;
        let e = read_entry(&mut cursor, &mut total).unwrap().unwrap();
        assert_eq!(e.filename, long_name);
    }

    // -- Edge cases for entries with specific padding --

    #[test]
    fn test_entry_namesize_alignment_exact() {
        // filename "ab" + NUL = 3 bytes. Header(110) + 3 = 113, pad to 116 (3 pad).
        let entry = CpioEntry {
            header: CpioHeader {
                ino: 1,
                mode: S_IFREG | 0o644,
                uid: 0,
                gid: 0,
                nlink: 1,
                mtime: 0,
                filesize: 1,
                devmajor: 0,
                devminor: 0,
                rdevmajor: 0,
                rdevminor: 0,
                namesize: 3,
                checksum: 0,
            },
            filename: "ab".to_string(),
            data: vec![b'X'],
        };

        let trailer = build_trailer();
        let mut archive = Vec::new();
        write_entry(&mut archive, &entry).unwrap();
        write_entry(&mut archive, &trailer).unwrap();

        let mut cursor = io::Cursor::new(&archive);
        let mut total = 0usize;
        let e = read_entry(&mut cursor, &mut total).unwrap().unwrap();
        assert_eq!(e.filename, "ab");
        assert_eq!(e.data, vec![b'X']);
    }

    #[test]
    fn test_entry_data_alignment_exact_multiple() {
        // Data size = 4 bytes -> no padding needed.
        let entry = CpioEntry {
            header: CpioHeader {
                ino: 1,
                mode: S_IFREG | 0o644,
                uid: 0,
                gid: 0,
                nlink: 1,
                mtime: 0,
                filesize: 4,
                devmajor: 0,
                devminor: 0,
                rdevmajor: 0,
                rdevminor: 0,
                namesize: 4,
                checksum: 0,
            },
            filename: "foo".to_string(),
            data: vec![b'A', b'B', b'C', b'D'],
        };

        let trailer = build_trailer();
        let mut archive = Vec::new();
        write_entry(&mut archive, &entry).unwrap();
        write_entry(&mut archive, &trailer).unwrap();

        let mut cursor = io::Cursor::new(&archive);
        let mut total = 0usize;
        let e = read_entry(&mut cursor, &mut total).unwrap().unwrap();
        assert_eq!(e.data, vec![b'A', b'B', b'C', b'D']);
    }
}
