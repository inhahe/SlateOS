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
//! `list_archive`) are exercised by unit tests on every host.

use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use std::process;

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
            eprintln!("tar: {e}");
            process::exit(1);
        }
    };

    if parsed.create {
        #[cfg(unix)]
        {
            do_create(&parsed.archive_file, &parsed.files, parsed.verbose);
        }
        #[cfg(not(unix))]
        {
            eprintln!("tar: create mode is unix-only on this build");
            process::exit(1);
        }
    } else if parsed.extract {
        do_extract(&parsed.archive_file, parsed.directory.as_deref(), parsed.verbose);
    } else if parsed.list {
        do_list_main(&parsed.archive_file);
    } else {
        eprintln!("tar: must specify -c, -x, or -t");
        process::exit(1);
    }
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

#[cfg(unix)]
fn do_create(archive_file: &Option<String>, files: &[String], verbose: bool) {
    use std::os::unix::fs::MetadataExt;

    let mut out: Box<dyn Write> = match archive_file {
        Some(path) => match File::create(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("tar: {path}: {e}");
                process::exit(1);
            }
        },
        None => Box::new(io::stdout()),
    };

    fn add_file(path: &Path, name: &str, out: &mut dyn Write, verbose: bool) {
        use std::os::unix::fs::MetadataExt;
        let meta = match fs::metadata(path) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("tar: {name}: {e}");
                return;
            }
        };

        let mut header = TarHeader::new();
        header.set_name(name);
        TarHeader::set_octal(&mut header.mode, u64::from(meta.mode()) & 0o7777);
        TarHeader::set_octal(&mut header.uid, u64::from(meta.uid()));
        TarHeader::set_octal(&mut header.gid, u64::from(meta.gid()));
        TarHeader::set_octal(&mut header.size, meta.len());
        TarHeader::set_octal(&mut header.mtime, meta.mtime() as u64);
        header.typeflag = b'0';
        header.magic = *b"ustar\0";
        header.version = *b"00";
        header.compute_checksum();
        let _ = out.write_all(header.as_bytes());

        if verbose {
            eprintln!("{name}");
        }

        if let Ok(mut f) = File::open(path) {
            let mut buf = [0u8; BLOCK_SIZE];
            loop {
                let n = f.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                if let Some(zero) = buf.get_mut(n..) {
                    zero.fill(0);
                }
                let _ = out.write_all(&buf);
            }
        }
    }

    fn add_directory_recursive(dir: &Path, prefix: &str, out: &mut dyn Write, verbose: bool) {
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
        let _ = out.write_all(header.as_bytes());

        if verbose {
            eprintln!("{name}");
        }

        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let entry_path = entry.path();
                let entry_name =
                    format!("{prefix}/{}", entry.file_name().to_string_lossy());
                if entry_path.is_dir() {
                    add_directory_recursive(&entry_path, &entry_name, out, verbose);
                } else {
                    add_file(&entry_path, &entry_name, out, verbose);
                }
            }
        }
    }

    // Silence unused-import warning when no files are dirs/files.
    let _ = MetadataExt::mode;

    for path_str in files {
        let path = Path::new(path_str);
        if path.is_dir() {
            add_directory_recursive(path, path_str, &mut out, verbose);
        } else {
            add_file(path, path_str, &mut out, verbose);
        }
    }

    let zero_block = [0u8; BLOCK_SIZE];
    let _ = out.write_all(&zero_block);
    let _ = out.write_all(&zero_block);
}

fn do_extract(archive_file: &Option<String>, directory: Option<&str>, verbose: bool) {
    let mut input: Box<dyn Read> = match archive_file {
        Some(path) => match File::open(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("tar: {path}: {e}");
                process::exit(1);
            }
        },
        None => Box::new(io::stdin()),
    };

    if let Some(dir) = directory
        && let Err(e) = env::set_current_dir(dir)
    {
        eprintln!("tar: {dir}: {e}");
        process::exit(1);
    }

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
        let typeflag = header_buf.get(156).copied().unwrap_or(0);

        if name.is_empty() {
            break;
        }

        if verbose {
            eprintln!("{name}");
        }

        match typeflag {
            b'5' | b'\0' if name.ends_with('/') => {
                let _ = fs::create_dir_all(&name);
            }
            b'0' | b'\0' => {
                if let Some(parent) = Path::new(&name).parent() {
                    let _ = fs::create_dir_all(parent);
                }

                let blocks = size
                    .saturating_add(BLOCK_SIZE as u64 - 1)
                    .saturating_div(BLOCK_SIZE as u64);
                let mut file_data = Vec::with_capacity(usize::try_from(size).unwrap_or(0));

                for _ in 0..blocks {
                    let mut block = [0u8; BLOCK_SIZE];
                    if input.read_exact(&mut block).is_err() {
                        break;
                    }
                    file_data.extend_from_slice(&block);
                }

                file_data.truncate(usize::try_from(size).unwrap_or(0));
                if let Err(e) = fs::write(&name, &file_data) {
                    eprintln!("tar: {name}: {e}");
                }
            }
            _ => {
                let blocks = size
                    .saturating_add(BLOCK_SIZE as u64 - 1)
                    .saturating_div(BLOCK_SIZE as u64);
                for _ in 0..blocks {
                    let mut block = [0u8; BLOCK_SIZE];
                    let _ = input.read_exact(&mut block);
                }
            }
        }
    }
}

fn do_list_main(archive_file: &Option<String>) {
    let input: Box<dyn Read> = match archive_file {
        Some(path) => match File::open(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("tar: {path}: {e}");
                process::exit(1);
            }
        },
        None => Box::new(io::stdin()),
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let _ = list_archive(input, &mut out);
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

        writeln!(out, "{name}")?;

        let blocks = size
            .saturating_add(BLOCK_SIZE as u64 - 1)
            .saturating_div(BLOCK_SIZE as u64);
        for _ in 0..blocks {
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
