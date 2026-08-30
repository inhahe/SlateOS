//! Slate OS Hexadecimal File Dumper
//!
//! Display file contents in hexadecimal, decimal, octal, or ASCII formats.
//! Combines `hexdump`, `xxd`, and `od` functionality in a single binary.
//!
//! # Usage
//!
//! ```text
//! hexdump [options] [file...]
//! hexdump -C /bin/ls
//! hexdump -n 64 -s 0x100 firmware.bin
//! xxd file.bin
//! xxd -r hexfile.txt > binary.out
//! xxd -i data.bin
//! ```

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::process;

/// Size of the read buffer for streaming file processing.
const BUF_SIZE: usize = 8192;

/// Default number of octets per line in most modes.
const DEFAULT_LINE_WIDTH: usize = 16;

// ============================================================================
// Display mode
// ============================================================================

/// The primary display format selected by command-line flags.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DisplayMode {
    /// Canonical hex+ASCII (`-C` or default).
    Canonical,
    /// Two-byte hexadecimal (`-x`).
    TwoByteHex,
    /// Two-byte decimal (`-d`).
    TwoByteDec,
    /// Two-byte octal (`-o`).
    TwoByteOctal,
    /// One-byte octal (`-b`).
    OneByteOctal,
    /// One-byte character display (`-c`).
    CharDisplay,
    /// JSON output (`--json`).
    Json,
    /// xxd plain hex dump (default when invoked as `xxd`).
    XxdPlain,
    /// xxd reverse: hex dump back to binary (`xxd -r`).
    XxdReverse,
    /// xxd C include style (`xxd -i`).
    XxdCInclude,
    /// xxd plain hex only, no offsets (`xxd -p`).
    XxdPlainHex,
}

// ============================================================================
// Configuration
// ============================================================================

/// Parsed command-line options.
struct Options {
    /// Display mode.
    mode: DisplayMode,
    /// Maximum number of bytes to interpret (None = entire file).
    length: Option<u64>,
    /// Number of bytes to skip from the beginning.
    skip: u64,
    /// If true, show all data (do not replace duplicate lines with `*`).
    no_squeeze: bool,
    /// Number of octets per output line (xxd `-c`).
    cols: usize,
    /// Byte grouping size for xxd (xxd `-g`).
    group: usize,
    /// Input files (empty means read from stdin).
    files: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            mode: DisplayMode::Canonical,
            length: None,
            skip: 0,
            no_squeeze: false,
            cols: DEFAULT_LINE_WIDTH,
            group: 2,
            files: Vec::new(),
        }
    }
}

// ============================================================================
// Argument parsing
// ============================================================================

/// Print hexdump usage and exit.
fn usage_hexdump() -> ! {
    let msg = "\
Usage: hexdump [options] [file...]

Display file contents in various formats.

Options:
  -C, --canonical       Canonical hex+ASCII display (default)
  -x                    Two-byte hexadecimal display
  -d                    Two-byte decimal display
  -o                    Two-byte octal display
  -b                    One-byte octal display
  -c                    One-byte character display
  -n <length>           Interpret only <length> bytes
  -s <offset>,
      --skip=<offset>   Skip <offset> bytes from beginning
  -v, --no-squeezing    Show all data (don't collapse duplicate lines)
      --json            JSON output with offset and hex bytes
  -h, --help            Show this help

If no files are given, reads from standard input.";
    eprintln!("{msg}");
    process::exit(0);
}

/// Print xxd usage and exit.
fn usage_xxd() -> ! {
    let msg = "\
Usage: xxd [options] [file]

Make a hexdump or do the reverse.

Options:
  -r          Reverse: convert hexdump back to binary
  -i          C include file style output
  -p          Plain hexdump (no offset or ASCII)
  -l <len>    Stop after <len> bytes
  -s <offset> Start at <offset>
  -c <cols>   Octets per line (default 16)
  -g <bytes>  Group bytes (default 2)
  -h, --help  Show this help";
    eprintln!("{msg}");
    process::exit(0);
}

/// Parse a numeric argument that may be decimal or hex (0x prefix).
fn parse_number(s: &str) -> Result<u64, String> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16)
            .map_err(|_| format!("invalid hex number: '{s}'"))
    } else {
        s.parse::<u64>()
            .map_err(|_| format!("invalid number: '{s}'"))
    }
}

/// Extract the value for a short option that requires an argument.
///
/// If there are remaining characters after position `j` in the current flag
/// group, those characters are the value (e.g. `-n64`). Otherwise, the next
/// command-line argument is consumed (e.g. `-n 64`).
fn get_option_value(
    chars: &[char],
    j: usize,
    args: &[String],
    arg_idx: &mut usize,
) -> Result<String, String> {
    if j + 1 < chars.len() {
        return Ok(chars[j + 1..].iter().collect());
    }
    if *arg_idx + 1 < args.len() {
        *arg_idx += 1;
        return Ok(args[*arg_idx].clone());
    }
    let opt_char = chars[j];
    Err(format!("option -{opt_char} requires a value"))
}

/// Parse arguments when invoked as `hexdump`.
fn parse_args_hexdump(args: &[String]) -> Result<Options, String> {
    let mut opts = Options::default();
    let mut i = 0;
    let mut end_of_opts = false;

    while i < args.len() {
        let arg = &args[i];

        if end_of_opts || !arg.starts_with('-') || arg == "-" {
            opts.files.push(arg.clone());
            i += 1;
            continue;
        }

        if arg == "--" {
            end_of_opts = true;
            i += 1;
            continue;
        }

        // Long options.
        if let Some(rest) = arg.strip_prefix("--") {
            if rest == "help" {
                usage_hexdump();
            } else if rest == "canonical" {
                opts.mode = DisplayMode::Canonical;
            } else if rest == "no-squeezing" {
                opts.no_squeeze = true;
            } else if rest == "json" {
                opts.mode = DisplayMode::Json;
            } else if let Some(val) = rest.strip_prefix("skip=") {
                opts.skip = parse_number(val)?;
            } else {
                return Err(format!("unknown option: --{rest}"));
            }
            i += 1;
            continue;
        }

        // Short options.
        let chars: Vec<char> = arg[1..].chars().collect();
        let mut j = 0;
        while j < chars.len() {
            match chars[j] {
                'h' => usage_hexdump(),
                'C' => opts.mode = DisplayMode::Canonical,
                'x' => opts.mode = DisplayMode::TwoByteHex,
                'd' => opts.mode = DisplayMode::TwoByteDec,
                'o' => opts.mode = DisplayMode::TwoByteOctal,
                'b' => opts.mode = DisplayMode::OneByteOctal,
                'c' => opts.mode = DisplayMode::CharDisplay,
                'v' => opts.no_squeeze = true,
                'n' => {
                    let val = get_option_value(&chars, j, args, &mut i)?;
                    opts.length = Some(parse_number(&val)?);
                    j = chars.len();
                    continue;
                }
                's' => {
                    let val = get_option_value(&chars, j, args, &mut i)?;
                    opts.skip = parse_number(&val)?;
                    j = chars.len();
                    continue;
                }
                other => {
                    return Err(format!("unknown option: -{other}"));
                }
            }
            j += 1;
        }
        i += 1;
    }

    Ok(opts)
}

/// Parse arguments when invoked as `xxd`.
fn parse_args_xxd(args: &[String]) -> Result<Options, String> {
    let mut opts = Options {
        mode: DisplayMode::XxdPlain,
        ..Options::default()
    };
    let mut i = 0;
    let mut end_of_opts = false;

    while i < args.len() {
        let arg = &args[i];

        if end_of_opts || !arg.starts_with('-') || arg == "-" {
            opts.files.push(arg.clone());
            i += 1;
            continue;
        }

        if arg == "--" {
            end_of_opts = true;
            i += 1;
            continue;
        }

        if let Some(rest) = arg.strip_prefix("--") {
            match rest {
                "help" => usage_xxd(),
                _ => return Err(format!("unknown option: --{rest}")),
            }
        }

        let chars: Vec<char> = arg[1..].chars().collect();
        let mut j = 0;
        while j < chars.len() {
            match chars[j] {
                'h' => usage_xxd(),
                'r' => opts.mode = DisplayMode::XxdReverse,
                'i' => opts.mode = DisplayMode::XxdCInclude,
                'p' => opts.mode = DisplayMode::XxdPlainHex,
                'l' => {
                    let val = get_option_value(&chars, j, args, &mut i)?;
                    opts.length = Some(parse_number(&val)?);
                    j = chars.len();
                    continue;
                }
                's' => {
                    let val = get_option_value(&chars, j, args, &mut i)?;
                    opts.skip = parse_number(&val)?;
                    j = chars.len();
                    continue;
                }
                'c' => {
                    let val = get_option_value(&chars, j, args, &mut i)?;
                    let n = parse_number(&val)?;
                    if n == 0 || n > 256 {
                        return Err(format!("invalid column count: {n}"));
                    }
                    opts.cols = n as usize;
                    j = chars.len();
                    continue;
                }
                'g' => {
                    let val = get_option_value(&chars, j, args, &mut i)?;
                    let n = parse_number(&val)?;
                    if n > 256 {
                        return Err(format!("invalid group size: {n}"));
                    }
                    opts.group = n as usize;
                    j = chars.len();
                    continue;
                }
                other => {
                    return Err(format!("unknown option: -{other}"));
                }
            }
            j += 1;
        }
        i += 1;
    }

    Ok(opts)
}

/// Detect whether we were invoked as `xxd` (by checking argv[0]).
fn is_xxd_invocation() -> bool {
    let argv0 = env::args().next().unwrap_or_default();
    let basename = argv0
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&argv0);
    // Strip .exe suffix on Windows-like platforms.
    let name = basename.strip_suffix(".exe").unwrap_or(basename);
    name == "xxd"
}

/// Parse command-line arguments, dispatching based on invocation name.
fn parse_args() -> Result<Options, String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if is_xxd_invocation() {
        parse_args_xxd(&args)
    } else {
        parse_args_hexdump(&args)
    }
}

// ============================================================================
// Input source abstraction
// ============================================================================

/// A reader that optionally limits the total number of bytes read.
struct LimitedReader<R: Read> {
    inner: R,
    remaining: Option<u64>,
}

impl<R: Read> LimitedReader<R> {
    fn new(inner: R, limit: Option<u64>) -> Self {
        Self {
            inner,
            remaining: limit,
        }
    }
}

impl<R: Read> Read for LimitedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let max_read = match self.remaining {
            Some(0) => return Ok(0),
            Some(r) => buf.len().min(r as usize),
            None => buf.len(),
        };
        let n = self.inner.read(&mut buf[..max_read])?;
        if let Some(ref mut r) = self.remaining {
            *r = r.saturating_sub(n as u64);
        }
        Ok(n)
    }
}

/// Open the input source (file or stdin), applying skip and length limits.
///
/// Returns a boxed reader and the starting offset for display purposes.
fn open_input(opts: &Options, path: Option<&str>) -> Result<(Box<dyn Read>, u64), String> {
    let display_offset = opts.skip;

    match path {
        Some("-") | None => {
            let stdin = io::stdin();
            let mut reader: Box<dyn Read> = Box::new(stdin);
            // Skip bytes by reading and discarding.
            if opts.skip > 0 {
                let mut remaining = opts.skip;
                let mut discard = [0u8; BUF_SIZE];
                while remaining > 0 {
                    let to_read = remaining.min(discard.len() as u64) as usize;
                    let n = reader
                        .read(&mut discard[..to_read])
                        .map_err(|e| format!("stdin: {e}"))?;
                    if n == 0 {
                        break;
                    }
                    remaining = remaining.saturating_sub(n as u64);
                }
            }
            let limited = LimitedReader::new(reader, opts.length);
            Ok((Box::new(limited), display_offset))
        }
        Some(path) => {
            let mut file = File::open(path).map_err(|e| format!("{path}: {e}"))?;
            if opts.skip > 0 {
                file.seek(SeekFrom::Start(opts.skip))
                    .map_err(|e| format!("{path}: seek error: {e}"))?;
            }
            let limited = LimitedReader::new(file, opts.length);
            Ok((Box::new(limited), display_offset))
        }
    }
}

// ============================================================================
// Canonical hex+ASCII display (-C)
// ============================================================================

/// Format and write a canonical hex+ASCII line.
///
/// Format: `XXXXXXXX  HH HH HH HH HH HH HH HH  HH HH HH HH HH HH HH HH  |................|`
fn write_canonical_line(
    out: &mut impl Write,
    offset: u64,
    data: &[u8],
) -> io::Result<()> {
    // Offset.
    write!(out, "{offset:08x}  ")?;

    // Hex bytes in two groups of 8.
    for i in 0..DEFAULT_LINE_WIDTH {
        if i == 8 {
            write!(out, " ")?;
        }
        if i < data.len() {
            write!(out, "{:02x} ", data[i])?;
        } else {
            write!(out, "   ")?;
        }
    }

    // ASCII sidebar.
    write!(out, " |")?;
    for &b in data {
        let ch = if (0x20..=0x7E).contains(&b) {
            b as char
        } else {
            '.'
        };
        write!(out, "{ch}")?;
    }
    writeln!(out, "|")
}

/// Run the canonical hex+ASCII display mode.
fn dump_canonical(
    reader: &mut dyn Read,
    out: &mut impl Write,
    start_offset: u64,
    no_squeeze: bool,
) -> io::Result<()> {
    let mut buf = [0u8; BUF_SIZE];
    let mut line_buf = [0u8; DEFAULT_LINE_WIDTH];
    let mut line_len = 0usize;
    let mut offset = start_offset;
    let mut prev_line: Option<[u8; DEFAULT_LINE_WIDTH]> = None;
    let mut prev_line_len: usize = 0;
    let mut squeezed = false;

    loop {
        // Fill the read buffer.
        let n = reader.read(&mut buf)?;
        if n == 0 {
            // Flush any partial line.
            if line_len > 0 {
                write_canonical_line(out, offset, &line_buf[..line_len])?;
                offset = offset.wrapping_add(line_len as u64);
            }
            break;
        }

        let mut pos = 0;
        while pos < n {
            let space = DEFAULT_LINE_WIDTH - line_len;
            let copy_len = space.min(n - pos);
            line_buf[line_len..line_len + copy_len].copy_from_slice(&buf[pos..pos + copy_len]);
            line_len += copy_len;
            pos += copy_len;

            if line_len == DEFAULT_LINE_WIDTH {
                // Check for duplicate line squeezing.
                if !no_squeeze
                    && let Some(ref prev) = prev_line
                        && prev_line_len == DEFAULT_LINE_WIDTH && &line_buf == prev {
                            if !squeezed {
                                writeln!(out, "*")?;
                                squeezed = true;
                            }
                            offset = offset.wrapping_add(DEFAULT_LINE_WIDTH as u64);
                            line_len = 0;
                            continue;
                        }

                write_canonical_line(out, offset, &line_buf)?;
                squeezed = false;
                prev_line = Some(line_buf);
                prev_line_len = DEFAULT_LINE_WIDTH;
                offset = offset.wrapping_add(DEFAULT_LINE_WIDTH as u64);
                line_len = 0;
            }
        }
    }

    // Print the final offset line.
    writeln!(out, "{offset:08x}")
}

// ============================================================================
// Two-byte hex display (-x)
// ============================================================================

fn dump_two_byte_hex(
    reader: &mut dyn Read,
    out: &mut impl Write,
    start_offset: u64,
    no_squeeze: bool,
) -> io::Result<()> {
    dump_two_byte_generic(reader, out, start_offset, no_squeeze, |out, w| {
        write!(out, " {:04x}", w)
    })
}

// ============================================================================
// Two-byte decimal display (-d)
// ============================================================================

fn dump_two_byte_dec(
    reader: &mut dyn Read,
    out: &mut impl Write,
    start_offset: u64,
    no_squeeze: bool,
) -> io::Result<()> {
    dump_two_byte_generic(reader, out, start_offset, no_squeeze, |out, w| {
        write!(out, " {:5}", w)
    })
}

// ============================================================================
// Two-byte octal display (-o)
// ============================================================================

fn dump_two_byte_octal(
    reader: &mut dyn Read,
    out: &mut impl Write,
    start_offset: u64,
    no_squeeze: bool,
) -> io::Result<()> {
    dump_two_byte_generic(reader, out, start_offset, no_squeeze, |out, w| {
        write!(out, " {:06o}", w)
    })
}

// ============================================================================
// Generic two-byte display
// ============================================================================

/// Generic two-byte display: reads 16 bytes per line, formats each pair of
/// bytes using the provided formatter closure.
fn dump_two_byte_generic(
    reader: &mut dyn Read,
    out: &mut impl Write,
    start_offset: u64,
    no_squeeze: bool,
    mut format_word: impl FnMut(&mut dyn Write, u16) -> io::Result<()>,
) -> io::Result<()> {
    let mut buf = [0u8; BUF_SIZE];
    let mut line_buf = [0u8; DEFAULT_LINE_WIDTH];
    let mut line_len = 0usize;
    let mut offset = start_offset;
    let mut prev_line: Option<[u8; DEFAULT_LINE_WIDTH]> = None;
    let mut prev_line_len: usize = 0;
    let mut squeezed = false;

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            if line_len > 0 {
                write!(out, "{offset:07x}")?;
                let pairs = line_len.div_ceil(2);
                for i in 0..pairs {
                    let lo = line_buf[i * 2];
                    let hi = if i * 2 + 1 < line_len {
                        line_buf[i * 2 + 1]
                    } else {
                        0
                    };
                    let word = u16::from_le_bytes([lo, hi]);
                    format_word(out, word)?;
                }
                writeln!(out)?;
            }
            break;
        }

        let mut pos = 0;
        while pos < n {
            let space = DEFAULT_LINE_WIDTH - line_len;
            let copy_len = space.min(n - pos);
            line_buf[line_len..line_len + copy_len].copy_from_slice(&buf[pos..pos + copy_len]);
            line_len += copy_len;
            pos += copy_len;

            if line_len == DEFAULT_LINE_WIDTH {
                if !no_squeeze
                    && let Some(ref prev) = prev_line
                        && prev_line_len == DEFAULT_LINE_WIDTH && &line_buf == prev {
                            if !squeezed {
                                writeln!(out, "*")?;
                                squeezed = true;
                            }
                            offset = offset.wrapping_add(DEFAULT_LINE_WIDTH as u64);
                            line_len = 0;
                            continue;
                        }

                write!(out, "{offset:07x}")?;
                for i in 0..8 {
                    let word = u16::from_le_bytes([
                        line_buf[i * 2],
                        line_buf[i * 2 + 1],
                    ]);
                    format_word(out, word)?;
                }
                writeln!(out)?;

                squeezed = false;
                prev_line = Some(line_buf);
                prev_line_len = DEFAULT_LINE_WIDTH;
                offset = offset.wrapping_add(DEFAULT_LINE_WIDTH as u64);
                line_len = 0;
            }
        }
    }

    Ok(())
}

// ============================================================================
// One-byte octal display (-b)
// ============================================================================

fn dump_one_byte_octal(
    reader: &mut dyn Read,
    out: &mut impl Write,
    start_offset: u64,
    no_squeeze: bool,
) -> io::Result<()> {
    dump_one_byte_generic(reader, out, start_offset, no_squeeze, |out, b| {
        write!(out, " {:03o}", b)
    })
}

// ============================================================================
// One-byte character display (-c)
// ============================================================================

fn dump_char_display(
    reader: &mut dyn Read,
    out: &mut impl Write,
    start_offset: u64,
    no_squeeze: bool,
) -> io::Result<()> {
    dump_one_byte_generic(reader, out, start_offset, no_squeeze, |out, b| {
        match b {
            b'\0' => write!(out, "  \\0"),
            b'\t' => write!(out, "  \\t"),
            b'\n' => write!(out, "  \\n"),
            b'\r' => write!(out, "  \\r"),
            0x20..=0x7E => write!(out, "   {}", b as char),
            _ => write!(out, " {:03o}", b),
        }
    })
}

// ============================================================================
// Generic one-byte display
// ============================================================================

fn dump_one_byte_generic(
    reader: &mut dyn Read,
    out: &mut impl Write,
    start_offset: u64,
    no_squeeze: bool,
    mut format_byte: impl FnMut(&mut dyn Write, u8) -> io::Result<()>,
) -> io::Result<()> {
    let mut buf = [0u8; BUF_SIZE];
    let mut line_buf = [0u8; DEFAULT_LINE_WIDTH];
    let mut line_len = 0usize;
    let mut offset = start_offset;
    let mut prev_line: Option<[u8; DEFAULT_LINE_WIDTH]> = None;
    let mut prev_line_len: usize = 0;
    let mut squeezed = false;

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            if line_len > 0 {
                write!(out, "{offset:07x}")?;
                for &b in &line_buf[..line_len] {
                    format_byte(out, b)?;
                }
                writeln!(out)?;
            }
            break;
        }

        let mut pos = 0;
        while pos < n {
            let space = DEFAULT_LINE_WIDTH - line_len;
            let copy_len = space.min(n - pos);
            line_buf[line_len..line_len + copy_len].copy_from_slice(&buf[pos..pos + copy_len]);
            line_len += copy_len;
            pos += copy_len;

            if line_len == DEFAULT_LINE_WIDTH {
                if !no_squeeze
                    && let Some(ref prev) = prev_line
                        && prev_line_len == DEFAULT_LINE_WIDTH && &line_buf == prev {
                            if !squeezed {
                                writeln!(out, "*")?;
                                squeezed = true;
                            }
                            offset = offset.wrapping_add(DEFAULT_LINE_WIDTH as u64);
                            line_len = 0;
                            continue;
                        }

                write!(out, "{offset:07x}")?;
                for &b in &line_buf[..DEFAULT_LINE_WIDTH] {
                    format_byte(out, b)?;
                }
                writeln!(out)?;

                squeezed = false;
                prev_line = Some(line_buf);
                prev_line_len = DEFAULT_LINE_WIDTH;
                offset = offset.wrapping_add(DEFAULT_LINE_WIDTH as u64);
                line_len = 0;
            }
        }
    }

    Ok(())
}

// ============================================================================
// JSON output (--json)
// ============================================================================

fn dump_json(
    reader: &mut dyn Read,
    out: &mut impl Write,
    start_offset: u64,
) -> io::Result<()> {
    let mut buf = [0u8; BUF_SIZE];
    let mut line_buf = [0u8; DEFAULT_LINE_WIDTH];
    let mut line_len = 0usize;
    let mut offset = start_offset;
    let mut first = true;

    writeln!(out, "[")?;

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            if line_len > 0 {
                if !first {
                    writeln!(out, ",")?;
                }
                write_json_line(out, offset, &line_buf[..line_len])?;
            }
            break;
        }

        let mut pos = 0;
        while pos < n {
            let space = DEFAULT_LINE_WIDTH - line_len;
            let copy_len = space.min(n - pos);
            line_buf[line_len..line_len + copy_len].copy_from_slice(&buf[pos..pos + copy_len]);
            line_len += copy_len;
            pos += copy_len;

            if line_len == DEFAULT_LINE_WIDTH {
                if !first {
                    writeln!(out, ",")?;
                }
                write_json_line(out, offset, &line_buf)?;
                first = false;
                offset = offset.wrapping_add(DEFAULT_LINE_WIDTH as u64);
                line_len = 0;
            }
        }
    }

    writeln!(out, "\n]")
}

fn write_json_line(
    out: &mut impl Write,
    offset: u64,
    data: &[u8],
) -> io::Result<()> {
    write!(out, "  {{\"offset\":{offset},\"hex\":[")?;
    for (i, &b) in data.iter().enumerate() {
        if i > 0 {
            write!(out, ",")?;
        }
        write!(out, "\"0x{b:02x}\"")?;
    }
    write!(out, "],\"ascii\":\"")?;
    for &b in data {
        match b {
            b'"' => write!(out, "\\\"")?,
            b'\\' => write!(out, "\\\\")?,
            0x20..=0x7E => write!(out, "{}", b as char)?,
            _ => write!(out, ".")?,
        }
    }
    write!(out, "\"}}")
}

// ============================================================================
// xxd plain hex dump
// ============================================================================

fn dump_xxd_plain(
    reader: &mut dyn Read,
    out: &mut impl Write,
    start_offset: u64,
    cols: usize,
    group: usize,
) -> io::Result<()> {
    let mut buf = [0u8; BUF_SIZE];
    let line_width = cols;
    let mut line_buf = vec![0u8; line_width];
    let mut line_len = 0usize;
    let mut offset = start_offset;

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            if line_len > 0 {
                write_xxd_line(out, offset, &line_buf[..line_len], line_width, group)?;
            }
            break;
        }

        let mut pos = 0;
        while pos < n {
            let space = line_width - line_len;
            let copy_len = space.min(n - pos);
            line_buf[line_len..line_len + copy_len].copy_from_slice(&buf[pos..pos + copy_len]);
            line_len += copy_len;
            pos += copy_len;

            if line_len == line_width {
                write_xxd_line(out, offset, &line_buf, line_width, group)?;
                offset = offset.wrapping_add(line_width as u64);
                line_len = 0;
            }
        }
    }

    Ok(())
}

fn write_xxd_line(
    out: &mut impl Write,
    offset: u64,
    data: &[u8],
    line_width: usize,
    group: usize,
) -> io::Result<()> {
    write!(out, "{offset:08x}: ")?;

    // Hex portion.
    for (i, &b) in data.iter().enumerate() {
        write!(out, "{b:02x}")?;
        // Insert space after each group (but not after the last byte if it
        // falls right on a group boundary at the end of actual data).
        if group > 0 && (i + 1) % group == 0 && i + 1 < line_width {
            write!(out, " ")?;
        }
    }

    // Pad if short line.
    if data.len() < line_width {
        for i in data.len()..line_width {
            write!(out, "  ")?;
            if group > 0 && (i + 1) % group == 0 && i + 1 < line_width {
                write!(out, " ")?;
            }
        }
    }

    // ASCII portion.
    write!(out, "  ")?;
    for &b in data {
        let ch = if (0x20..=0x7E).contains(&b) {
            b as char
        } else {
            '.'
        };
        write!(out, "{ch}")?;
    }
    writeln!(out)
}

// ============================================================================
// xxd C include style (-i)
// ============================================================================

fn dump_xxd_c_include(
    reader: &mut dyn Read,
    out: &mut impl Write,
    filename: Option<&str>,
) -> io::Result<()> {
    let var_name = make_c_identifier(filename.unwrap_or("stdin"));

    writeln!(out, "unsigned char {var_name}[] = {{")?;

    let mut buf = [0u8; BUF_SIZE];
    let mut total: u64 = 0;
    let mut col = 0;

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }

        for &b in &buf[..n] {
            if col == 0 {
                write!(out, "  ")?;
            }
            write!(out, "0x{b:02x}, ")?;
            col += 1;
            if col >= 12 {
                writeln!(out)?;
                col = 0;
            }
            total = total.wrapping_add(1);
        }
    }

    if col > 0 {
        writeln!(out)?;
    }
    writeln!(out, "}};")?;
    writeln!(out, "unsigned int {var_name}_len = {total};")
}

/// Convert a filename to a valid C identifier.
fn make_c_identifier(name: &str) -> String {
    let basename = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name);
    let mut result = String::with_capacity(basename.len());
    for (i, ch) in basename.chars().enumerate() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            if i == 0 && ch.is_ascii_digit() {
                result.push('_');
            }
            result.push(ch);
        } else {
            result.push('_');
        }
    }
    if result.is_empty() {
        result.push_str("data");
    }
    result
}

// ============================================================================
// xxd plain hex only (-p)
// ============================================================================

fn dump_xxd_plain_hex(
    reader: &mut dyn Read,
    out: &mut impl Write,
    cols: usize,
) -> io::Result<()> {
    let mut buf = [0u8; BUF_SIZE];
    let mut col = 0;

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }

        for &b in &buf[..n] {
            write!(out, "{b:02x}")?;
            col += 1;
            if col >= cols {
                writeln!(out)?;
                col = 0;
            }
        }
    }

    if col > 0 {
        writeln!(out)?;
    }

    Ok(())
}

// ============================================================================
// xxd reverse (-r)
// ============================================================================

/// Parse a hex dump (xxd-format) back into binary.
///
/// Expected input format per line: `OFFSET: HH HH HH ...  ASCII`
/// Also handles plain hex lines (no offset).
fn dump_xxd_reverse(
    reader: &mut dyn Read,
    out: &mut impl Write,
) -> io::Result<()> {
    let buf_reader = BufReader::new(reader);

    for line_result in buf_reader.lines() {
        let line = line_result?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Determine where the hex data starts.
        let hex_data = if let Some(colon_pos) = trimmed.find(':') {
            // Format: "offset: hex_data  ascii"
            let after_colon = &trimmed[colon_pos + 1..];
            // Strip any trailing ASCII section (after double-space).
            if let Some(ascii_sep) = after_colon.find("  ") {
                // Check if the part after the double-space looks like ASCII
                // (not more hex). The ASCII section typically starts with
                // printable characters without spaces between them.
                let potential_ascii = after_colon[ascii_sep + 2..].trim();
                if !potential_ascii.is_empty()
                    && potential_ascii
                        .chars()
                        .all(|c| c.is_ascii_graphic() || c == '.')
                {
                    &after_colon[..ascii_sep]
                } else {
                    after_colon
                }
            } else {
                after_colon
            }
        } else {
            // Plain hex, no offset prefix.
            trimmed
        };

        // Parse hex characters, ignoring spaces.
        let hex_chars: Vec<u8> = hex_data
            .bytes()
            .filter(|&b| b != b' ')
            .collect();

        let mut i = 0;
        while i + 1 < hex_chars.len() {
            let hi = hex_digit_value(hex_chars[i]);
            let lo = hex_digit_value(hex_chars[i + 1]);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.write_all(&[h << 4 | l])?;
            }
            i += 2;
        }
    }

    out.flush()
}

/// Convert a hex character to its numeric value.
fn hex_digit_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// ============================================================================
// Dispatch
// ============================================================================

/// Run the selected display mode on a single input source.
fn process_input(
    opts: &Options,
    path: Option<&str>,
    out: &mut impl Write,
) -> Result<(), String> {
    // xxd reverse reads hex text, not binary -- handle separately.
    if opts.mode == DisplayMode::XxdReverse {
        let reader: Box<dyn Read> = match path {
            Some("-") | None => Box::new(io::stdin()),
            Some(p) => Box::new(
                File::open(p).map_err(|e| format!("{p}: {e}"))?
            ),
        };
        let mut reader = reader;
        return dump_xxd_reverse(&mut reader, out)
            .map_err(|e| format!("{}: {e}", path.unwrap_or("stdin")));
    }

    let (mut reader, start_offset) = open_input(opts, path)?;

    let result = match opts.mode {
        DisplayMode::Canonical => {
            dump_canonical(&mut reader, out, start_offset, opts.no_squeeze)
        }
        DisplayMode::TwoByteHex => {
            dump_two_byte_hex(&mut reader, out, start_offset, opts.no_squeeze)
        }
        DisplayMode::TwoByteDec => {
            dump_two_byte_dec(&mut reader, out, start_offset, opts.no_squeeze)
        }
        DisplayMode::TwoByteOctal => {
            dump_two_byte_octal(&mut reader, out, start_offset, opts.no_squeeze)
        }
        DisplayMode::OneByteOctal => {
            dump_one_byte_octal(&mut reader, out, start_offset, opts.no_squeeze)
        }
        DisplayMode::CharDisplay => {
            dump_char_display(&mut reader, out, start_offset, opts.no_squeeze)
        }
        DisplayMode::Json => {
            dump_json(&mut reader, out, start_offset)
        }
        DisplayMode::XxdPlain => {
            dump_xxd_plain(&mut reader, out, start_offset, opts.cols, opts.group)
        }
        DisplayMode::XxdCInclude => {
            dump_xxd_c_include(&mut reader, out, path)
        }
        DisplayMode::XxdPlainHex => {
            dump_xxd_plain_hex(&mut reader, out, opts.cols)
        }
        DisplayMode::XxdReverse => {
            // Already handled above; unreachable.
            Ok(())
        }
    };

    result.map_err(|e| format!("{}: {e}", path.unwrap_or("stdin")))
}

// ============================================================================
// Entry point
// ============================================================================

fn run() -> Result<(), String> {
    let opts = parse_args()?;
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    if opts.files.is_empty() {
        process_input(&opts, None, &mut out)?;
    } else {
        for path in &opts.files {
            let p = if path == "-" { None } else { Some(path.as_str()) };
            process_input(&opts, p, &mut out)?;
        }
    }

    out.flush().map_err(|e| format!("write error: {e}"))?;
    Ok(())
}

fn main() {
    if let Err(msg) = run() {
        let name = if is_xxd_invocation() { "xxd" } else { "hexdump" };
        eprintln!("{name}: {msg}");
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Number parsing ---

    #[test]
    fn parse_number_decimal() {
        assert_eq!(parse_number("0").unwrap(), 0);
        assert_eq!(parse_number("42").unwrap(), 42);
        assert_eq!(parse_number("1024").unwrap(), 1024);
    }

    #[test]
    fn parse_number_hex() {
        assert_eq!(parse_number("0x0").unwrap(), 0);
        assert_eq!(parse_number("0xff").unwrap(), 255);
        assert_eq!(parse_number("0X100").unwrap(), 256);
        assert_eq!(parse_number("0xDEAD").unwrap(), 0xDEAD);
    }

    #[test]
    fn parse_number_invalid() {
        assert!(parse_number("").is_err());
        assert!(parse_number("xyz").is_err());
        assert!(parse_number("0xGG").is_err());
    }

    // --- C identifier conversion ---

    #[test]
    fn c_identifier_simple() {
        assert_eq!(make_c_identifier("data.bin"), "data_bin");
    }

    #[test]
    fn c_identifier_with_path() {
        assert_eq!(make_c_identifier("/usr/local/data.bin"), "data_bin");
        assert_eq!(make_c_identifier("C:\\Users\\data.bin"), "data_bin");
    }

    #[test]
    fn c_identifier_leading_digit() {
        assert_eq!(make_c_identifier("123file"), "_123file");
    }

    #[test]
    fn c_identifier_special_chars() {
        assert_eq!(make_c_identifier("my-file.dat"), "my_file_dat");
    }

    #[test]
    fn c_identifier_empty() {
        assert_eq!(make_c_identifier(""), "data");
    }

    // --- Hex digit parsing ---

    #[test]
    fn hex_digit_value_digits() {
        for i in 0..=9 {
            assert_eq!(hex_digit_value(b'0' + i), Some(i));
        }
    }

    #[test]
    fn hex_digit_value_lowercase() {
        assert_eq!(hex_digit_value(b'a'), Some(10));
        assert_eq!(hex_digit_value(b'f'), Some(15));
    }

    #[test]
    fn hex_digit_value_uppercase() {
        assert_eq!(hex_digit_value(b'A'), Some(10));
        assert_eq!(hex_digit_value(b'F'), Some(15));
    }

    #[test]
    fn hex_digit_value_invalid() {
        assert_eq!(hex_digit_value(b'g'), None);
        assert_eq!(hex_digit_value(b' '), None);
        assert_eq!(hex_digit_value(b'\n'), None);
    }

    // --- Canonical format ---

    #[test]
    fn canonical_hello_world() {
        let data = b"Hello World\n";
        let mut cursor = io::Cursor::new(data);
        let mut output = Vec::new();
        dump_canonical(&mut cursor, &mut output, 0, true).unwrap();
        let text = String::from_utf8(output).unwrap();
        // Should have the hex line followed by the final offset.
        assert!(text.contains("00000000"));
        assert!(text.contains("|Hello World.|"));
        assert!(text.contains("0000000c"));
    }

    #[test]
    fn canonical_exact_16_bytes() {
        let data = b"0123456789ABCDEF";
        let mut cursor = io::Cursor::new(data);
        let mut output = Vec::new();
        dump_canonical(&mut cursor, &mut output, 0, true).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("|0123456789ABCDEF|"));
        assert!(text.contains("00000010"));
    }

    #[test]
    fn canonical_nonprintable() {
        let data = [0x00, 0x01, 0x7F, 0xFF];
        let mut cursor = io::Cursor::new(&data);
        let mut output = Vec::new();
        dump_canonical(&mut cursor, &mut output, 0, true).unwrap();
        let text = String::from_utf8(output).unwrap();
        // Non-printable bytes should be shown as dots in the ASCII column.
        assert!(text.contains("|....|"));
    }

    #[test]
    fn canonical_squeeze_duplicate_lines() {
        // Two identical 16-byte lines followed by a different line.
        let mut data = Vec::new();
        data.extend_from_slice(&[0xAA; 16]);
        data.extend_from_slice(&[0xAA; 16]);
        data.extend_from_slice(&[0xBB; 4]);
        let mut cursor = io::Cursor::new(&data);
        let mut output = Vec::new();
        dump_canonical(&mut cursor, &mut output, 0, false).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("*"));
    }

    #[test]
    fn canonical_no_squeeze() {
        let mut data = Vec::new();
        data.extend_from_slice(&[0xAA; 16]);
        data.extend_from_slice(&[0xAA; 16]);
        let mut cursor = io::Cursor::new(&data);
        let mut output = Vec::new();
        dump_canonical(&mut cursor, &mut output, 0, true).unwrap();
        let text = String::from_utf8(output).unwrap();
        // With no_squeeze=true, the `*` should NOT appear.
        assert!(!text.contains("*"));
        // Both lines should be printed.
        assert!(text.contains("00000000"));
        assert!(text.contains("00000010"));
    }

    #[test]
    fn canonical_with_offset() {
        let data = b"Test";
        let mut cursor = io::Cursor::new(data);
        let mut output = Vec::new();
        dump_canonical(&mut cursor, &mut output, 0x100, true).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.starts_with("00000100"));
    }

    // --- One-byte octal ---

    #[test]
    fn one_byte_octal_basic() {
        let data = [0o101, 0o102, 0o103]; // 'A', 'B', 'C'
        let mut cursor = io::Cursor::new(&data);
        let mut output = Vec::new();
        dump_one_byte_octal(&mut cursor, &mut output, 0, true).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("101"));
        assert!(text.contains("102"));
        assert!(text.contains("103"));
    }

    // --- Character display ---

    #[test]
    fn char_display_printable() {
        let data = b"ABC";
        let mut cursor = io::Cursor::new(data);
        let mut output = Vec::new();
        dump_char_display(&mut cursor, &mut output, 0, true).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("A"));
        assert!(text.contains("B"));
        assert!(text.contains("C"));
    }

    #[test]
    fn char_display_control() {
        let data = [0x00, b'\t', b'\n', b'\r'];
        let mut cursor = io::Cursor::new(&data);
        let mut output = Vec::new();
        dump_char_display(&mut cursor, &mut output, 0, true).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("\\0"));
        assert!(text.contains("\\t"));
        assert!(text.contains("\\n"));
        assert!(text.contains("\\r"));
    }

    // --- JSON output ---

    #[test]
    fn json_basic() {
        let data = b"Hi";
        let mut cursor = io::Cursor::new(data);
        let mut output = Vec::new();
        dump_json(&mut cursor, &mut output, 0).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("\"offset\":0"));
        assert!(text.contains("\"0x48\""));
        assert!(text.contains("\"0x69\""));
        assert!(text.contains("\"ascii\":\"Hi\""));
    }

    // --- xxd plain hex ---

    #[test]
    fn xxd_plain_basic() {
        let data = b"Hello";
        let mut cursor = io::Cursor::new(data);
        let mut output = Vec::new();
        dump_xxd_plain(&mut cursor, &mut output, 0, 16, 2).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("00000000:"));
        assert!(text.contains("4865"));
        assert!(text.contains("Hello"));
    }

    // --- xxd C include ---

    #[test]
    fn xxd_c_include_basic() {
        let data = b"\x01\x02\x03";
        let mut cursor = io::Cursor::new(data);
        let mut output = Vec::new();
        dump_xxd_c_include(&mut cursor, &mut output, Some("test.bin")).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("unsigned char test_bin[]"));
        assert!(text.contains("0x01"));
        assert!(text.contains("0x02"));
        assert!(text.contains("0x03"));
        assert!(text.contains("unsigned int test_bin_len = 3;"));
    }

    // --- xxd plain hex only ---

    #[test]
    fn xxd_plain_hex_only() {
        let data = b"\xDE\xAD\xBE\xEF";
        let mut cursor = io::Cursor::new(data);
        let mut output = Vec::new();
        dump_xxd_plain_hex(&mut cursor, &mut output, 16).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("deadbeef"));
    }

    // --- xxd reverse ---

    #[test]
    fn xxd_reverse_basic() {
        let hex_input = b"00000000: 4865 6c6c 6f0a                           Hello.\n";
        let mut cursor = io::Cursor::new(hex_input);
        let mut output = Vec::new();
        dump_xxd_reverse(&mut cursor, &mut output).unwrap();
        assert_eq!(&output, b"Hello\n");
    }

    #[test]
    fn xxd_reverse_plain_hex() {
        let hex_input = b"48656c6c6f\n";
        let mut cursor = io::Cursor::new(hex_input);
        let mut output = Vec::new();
        dump_xxd_reverse(&mut cursor, &mut output).unwrap();
        assert_eq!(&output, b"Hello");
    }

    // --- LimitedReader ---

    #[test]
    fn limited_reader_exact() {
        let data = b"Hello World";
        let reader = io::Cursor::new(data);
        let mut limited = LimitedReader::new(reader, Some(5));
        let mut buf = [0u8; 32];
        let n = limited.read(&mut buf).unwrap();
        assert_eq!(n, 5);
        assert_eq!(&buf[..5], b"Hello");
        // Further reads should return 0.
        let n2 = limited.read(&mut buf).unwrap();
        assert_eq!(n2, 0);
    }

    #[test]
    fn limited_reader_unlimited() {
        let data = b"Hello";
        let reader = io::Cursor::new(data);
        let mut limited = LimitedReader::new(reader, None);
        let mut buf = [0u8; 32];
        let n = limited.read(&mut buf).unwrap();
        assert_eq!(n, 5);
    }

    // --- Two-byte hex ---

    #[test]
    fn two_byte_hex_basic() {
        let data = [0x01, 0x02, 0x03, 0x04];
        let mut cursor = io::Cursor::new(&data);
        let mut output = Vec::new();
        dump_two_byte_hex(&mut cursor, &mut output, 0, true).unwrap();
        let text = String::from_utf8(output).unwrap();
        // Little-endian: 0x01, 0x02 -> 0x0201
        assert!(text.contains("0201"));
    }

    // --- Two-byte decimal ---

    #[test]
    fn two_byte_dec_basic() {
        let data = [0x01, 0x00, 0x00, 0x01];
        let mut cursor = io::Cursor::new(&data);
        let mut output = Vec::new();
        dump_two_byte_dec(&mut cursor, &mut output, 0, true).unwrap();
        let text = String::from_utf8(output).unwrap();
        // 0x01, 0x00 in LE = 1; 0x00, 0x01 in LE = 256
        assert!(text.contains("    1"));
        assert!(text.contains("  256"));
    }

    // --- Two-byte octal ---

    #[test]
    fn two_byte_octal_basic() {
        let data = [0o10, 0o0];
        let mut cursor = io::Cursor::new(&data);
        let mut output = Vec::new();
        dump_two_byte_octal(&mut cursor, &mut output, 0, true).unwrap();
        let text = String::from_utf8(output).unwrap();
        // 0o10 = 8, LE pair (8, 0) = 8 in LE = 000010
        assert!(text.contains("000010"));
    }

    // --- Empty input ---

    #[test]
    fn canonical_empty_input() {
        let data: &[u8] = &[];
        let mut cursor = io::Cursor::new(data);
        let mut output = Vec::new();
        dump_canonical(&mut cursor, &mut output, 0, true).unwrap();
        let text = String::from_utf8(output).unwrap();
        // Should just print the final offset.
        assert_eq!(text.trim(), "00000000");
    }

    #[test]
    fn json_empty_input() {
        let data: &[u8] = &[];
        let mut cursor = io::Cursor::new(data);
        let mut output = Vec::new();
        dump_json(&mut cursor, &mut output, 0).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("["));
        assert!(text.contains("]"));
    }

    // --- Round-trip: xxd dump then reverse ---

    #[test]
    fn xxd_round_trip() {
        let original = b"The quick brown fox jumps over the lazy dog.\n";
        let mut cursor = io::Cursor::new(original);
        let mut hex_output = Vec::new();
        dump_xxd_plain(&mut cursor, &mut hex_output, 0, 16, 2).unwrap();

        // Now reverse it.
        let mut hex_cursor = io::Cursor::new(&hex_output);
        let mut binary_output = Vec::new();
        dump_xxd_reverse(&mut hex_cursor, &mut binary_output).unwrap();
        assert_eq!(&binary_output, original);
    }
}
