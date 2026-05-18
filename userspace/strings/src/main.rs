//! OurOS Printable String Finder
//!
//! Finds and prints sequences of printable characters in binary files.
//! Useful for inspecting executables, libraries, core dumps, and other
//! binary data.
//!
//! # Usage
//!
//! ```text
//! strings [options] [file...]
//! strings -n 8 /bin/ls
//! strings -t x core.dump
//! strings -e l firmware.bin
//! echo binary_data | strings
//! ```

use std::env;
use std::fs::File;
use std::io::{self, BufWriter, Read, Write};
use std::process;

/// Size of the read buffer for streaming file processing.
const BUF_SIZE: usize = 8192;

// ============================================================================
// Configuration
// ============================================================================

/// Offset display format for the `-t` option.
#[derive(Clone, Copy)]
enum RadixFormat {
    /// Decimal offsets.
    Decimal,
    /// Octal offsets.
    Octal,
    /// Hexadecimal offsets.
    Hex,
}

/// Character encoding mode for the `-e` option.
#[derive(Clone, Copy)]
enum Encoding {
    /// 7-bit ASCII (bytes 0x00..=0x7F are candidates).
    Ascii7,
    /// 8-bit: any byte whose value is in the printable range.
    Ascii8,
    /// 16-bit big-endian.
    BigEndian16,
    /// 16-bit little-endian.
    LittleEndian16,
    /// 32-bit big-endian.
    BigEndian32,
    /// 32-bit little-endian.
    LittleEndian32,
}

/// Parsed command-line options.
struct Options {
    /// Minimum number of characters for a string to be printed.
    min_length: usize,
    /// Offset display format (None = do not show offsets).
    radix: Option<RadixFormat>,
    /// Character encoding to use when scanning.
    encoding: Encoding,
    /// Whether to prefix each string with the source filename.
    print_filename: bool,
    /// Whether to include all whitespace characters (not just space/tab).
    include_all_whitespace: bool,
    /// Whether to produce JSON output.
    json_output: bool,
    /// Input files (empty means read from stdin).
    files: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            min_length: 4,
            radix: None,
            encoding: Encoding::Ascii7,
            print_filename: false,
            include_all_whitespace: false,
            json_output: false,
            files: Vec::new(),
        }
    }
}

// ============================================================================
// Argument parsing
// ============================================================================

/// Print usage information and exit.
fn usage() -> ! {
    let msg = "\
Usage: strings [options] [file...]

Find and print sequences of printable characters in files.

Options:
  -a, --all                   Scan the entire file (default)
  -n <N>, --bytes=<N>, -<N>   Minimum string length (default: 4)
  -t <format>, --radix=<fmt>  Print offset before each string
                                d = decimal, o = octal, x = hexadecimal
  -e <enc>, --encoding=<enc>  Character encoding to use
                                s = 7-bit ASCII (default)
                                S = 8-bit
                                b = 16-bit big-endian
                                l = 16-bit little-endian
                                B = 32-bit big-endian
                                L = 32-bit little-endian
  -f, --print-file-name       Prefix each string with the filename
  -o                          Shorthand for -t o (octal offsets)
  -w, --include-all-whitespace Include all whitespace, not just space/tab
      --json                  JSON output with offset and string
  -h, --help                  Show this help
  --                          End of options

If no files are given, reads from standard input.";
    eprintln!("{msg}");
    process::exit(0);
}

/// Parse command-line arguments into an `Options` struct.
///
/// Returns `Err` with a message if the arguments are invalid.
fn parse_args() -> Result<Options, String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut opts = Options::default();
    let mut i = 0;
    let mut end_of_opts = false;

    while i < args.len() {
        let arg = &args[i];

        if end_of_opts || !arg.starts_with('-') || arg == "-" {
            // Positional argument (filename); "-" means stdin
            opts.files.push(arg.clone());
            i += 1;
            continue;
        }

        if arg == "--" {
            end_of_opts = true;
            i += 1;
            continue;
        }

        // Long options
        if let Some(rest) = arg.strip_prefix("--") {
            if rest == "help" {
                usage();
            } else if rest == "all" {
                // -a/--all is the default; accept silently.
            } else if let Some(val) = rest.strip_prefix("bytes=") {
                opts.min_length = parse_min_length(val)?;
            } else if let Some(val) = rest.strip_prefix("radix=") {
                opts.radix = Some(parse_radix(val)?);
            } else if let Some(val) = rest.strip_prefix("encoding=") {
                opts.encoding = parse_encoding(val)?;
            } else if rest == "print-file-name" {
                opts.print_filename = true;
            } else if rest == "include-all-whitespace" {
                opts.include_all_whitespace = true;
            } else if rest == "json" {
                opts.json_output = true;
            } else {
                return Err(format!("unknown option: --{rest}"));
            }
            i += 1;
            continue;
        }

        // Short options: may be grouped (e.g. -af) or require a value.
        let chars: Vec<char> = arg[1..].chars().collect();
        let mut j = 0;
        while j < chars.len() {
            match chars[j] {
                'h' => usage(),
                'a' => {
                    // Default behavior; accept silently.
                }
                'f' => {
                    opts.print_filename = true;
                }
                'o' => {
                    opts.radix = Some(RadixFormat::Octal);
                }
                'w' => {
                    opts.include_all_whitespace = true;
                }
                'n' => {
                    let val = get_option_value(&chars, j, &args, &mut i)?;
                    opts.min_length = parse_min_length(&val)?;
                    // get_option_value consumed the rest of this flag group
                    // or the next argument; skip to next arg.
                    j = chars.len();
                    continue;
                }
                't' => {
                    let val = get_option_value(&chars, j, &args, &mut i)?;
                    opts.radix = Some(parse_radix(&val)?);
                    j = chars.len();
                    continue;
                }
                'e' => {
                    let val = get_option_value(&chars, j, &args, &mut i)?;
                    opts.encoding = parse_encoding(&val)?;
                    j = chars.len();
                    continue;
                }
                c if c.is_ascii_digit() => {
                    // -<N> shorthand: collect all remaining digits as the
                    // minimum length.
                    let num_str: String = chars[j..].iter().collect();
                    opts.min_length = parse_min_length(&num_str)?;
                    j = chars.len();
                    continue;
                }
                c => {
                    return Err(format!("unknown option: -{c}"));
                }
            }
            j += 1;
        }
        i += 1;
    }

    if opts.min_length == 0 {
        return Err("minimum string length must be at least 1".to_string());
    }

    Ok(opts)
}

/// Extract the value for a short option that requires an argument.
///
/// If there are remaining characters after position `j` in the current flag
/// group, those characters are the value (e.g. `-n8`). Otherwise, the next
/// command-line argument is consumed (e.g. `-n 8`).
fn get_option_value(
    chars: &[char],
    j: usize,
    args: &[String],
    arg_idx: &mut usize,
) -> Result<String, String> {
    // Characters remaining after the option letter in the same token.
    if j + 1 < chars.len() {
        return Ok(chars[j + 1..].iter().collect());
    }
    // Otherwise consume the next argument.
    if *arg_idx + 1 < args.len() {
        *arg_idx += 1;
        return Ok(args[*arg_idx].clone());
    }
    let opt_char = chars[j];
    Err(format!("option -{opt_char} requires a value"))
}

/// Parse a minimum-length string into a `usize`.
fn parse_min_length(s: &str) -> Result<usize, String> {
    s.parse::<usize>()
        .map_err(|_| format!("invalid minimum length: {s}"))
}

/// Parse a radix format character.
fn parse_radix(s: &str) -> Result<RadixFormat, String> {
    match s {
        "d" => Ok(RadixFormat::Decimal),
        "o" => Ok(RadixFormat::Octal),
        "x" => Ok(RadixFormat::Hex),
        _ => Err(format!(
            "invalid radix format '{s}': expected d, o, or x"
        )),
    }
}

/// Parse an encoding specifier character.
fn parse_encoding(s: &str) -> Result<Encoding, String> {
    match s {
        "s" => Ok(Encoding::Ascii7),
        "S" => Ok(Encoding::Ascii8),
        "b" => Ok(Encoding::BigEndian16),
        "l" => Ok(Encoding::LittleEndian16),
        "B" => Ok(Encoding::BigEndian32),
        "L" => Ok(Encoding::LittleEndian32),
        _ => Err(format!(
            "invalid encoding '{s}': expected s, S, b, l, B, or L"
        )),
    }
}

// ============================================================================
// Character classification
// ============================================================================

/// Check whether a byte is considered printable under the current options.
///
/// Printable = ASCII 32..=126 (space through tilde) plus tab (0x09).
/// With `include_all_whitespace`, also newline (0x0A), vertical tab (0x0B),
/// form feed (0x0C), and carriage return (0x0D).
#[inline]
fn is_printable(b: u8, include_all_ws: bool) -> bool {
    match b {
        0x20..=0x7E => true,  // space through tilde
        0x09 => true,         // horizontal tab
        0x0A | 0x0B | 0x0C | 0x0D if include_all_ws => true,
        _ => false,
    }
}

/// Check whether a wide character value is considered printable.
///
/// For 16-bit and 32-bit encodings, we consider a codepoint printable if its
/// value falls in the same ASCII printable range (space through tilde, plus
/// tab and optionally other whitespace). Values above 0x7E are not considered
/// printable to stay consistent with the single-byte behavior.
#[inline]
fn is_wide_printable(val: u32, include_all_ws: bool) -> bool {
    if val > 0xFF {
        return false;
    }
    // The value fits in a byte; reuse the byte-level check.
    #[allow(clippy::cast_possible_truncation)]
    is_printable(val as u8, include_all_ws)
}

// ============================================================================
// Output formatting
// ============================================================================

/// Write the offset prefix for a found string.
fn write_offset(
    out: &mut impl Write,
    offset: u64,
    radix: RadixFormat,
) -> io::Result<()> {
    match radix {
        RadixFormat::Decimal => write!(out, "{offset:7} "),
        RadixFormat::Octal => write!(out, "{offset:7o} "),
        RadixFormat::Hex => write!(out, "{offset:7x} "),
    }
}

/// Write a single found string in plain-text mode.
fn emit_string(
    out: &mut impl Write,
    opts: &Options,
    filename: Option<&str>,
    offset: u64,
    s: &str,
) -> io::Result<()> {
    if opts.print_filename {
        if let Some(name) = filename {
            write!(out, "{name}: ")?;
        }
    }
    if let Some(radix) = opts.radix {
        write_offset(out, offset, radix)?;
    }
    writeln!(out, "{s}")
}

/// Write a single found string in JSON mode.
fn emit_json(
    out: &mut impl Write,
    opts: &Options,
    filename: Option<&str>,
    offset: u64,
    s: &str,
) -> io::Result<()> {
    // Build a JSON line manually to avoid pulling in serde.
    write!(out, "{{\"offset\":{offset}")?;
    if opts.print_filename {
        if let Some(name) = filename {
            write!(out, ",\"file\":\"")?;
            write_json_escaped(out, name)?;
            write!(out, "\"")?;
        }
    }
    write!(out, ",\"string\":\"")?;
    write_json_escaped(out, s)?;
    writeln!(out, "\"}}")
}

/// Write a string with JSON escaping (backslash, double-quote, control chars).
fn write_json_escaped(out: &mut impl Write, s: &str) -> io::Result<()> {
    for ch in s.chars() {
        match ch {
            '"' => write!(out, "\\\"")?,
            '\\' => write!(out, "\\\\")?,
            '\n' => write!(out, "\\n")?,
            '\r' => write!(out, "\\r")?,
            '\t' => write!(out, "\\t")?,
            c if (c as u32) < 0x20 => write!(out, "\\u{:04x}", c as u32)?,
            c => write!(out, "{c}")?,
        }
    }
    Ok(())
}

/// Emit a found string through either plain or JSON output.
fn emit(
    out: &mut impl Write,
    opts: &Options,
    filename: Option<&str>,
    offset: u64,
    s: &str,
) -> io::Result<()> {
    if opts.json_output {
        emit_json(out, opts, filename, offset, s)
    } else {
        emit_string(out, opts, filename, offset, s)
    }
}

// ============================================================================
// Single-byte scanning (encodings: s, S)
// ============================================================================

/// Scan a byte-oriented stream for printable strings.
///
/// Reads from `reader` in `BUF_SIZE` chunks, accumulating runs of printable
/// characters. When a non-printable byte (or EOF) terminates a run that is at
/// least `opts.min_length` characters, the string is emitted.
fn scan_bytes(
    reader: &mut dyn Read,
    out: &mut impl Write,
    opts: &Options,
    filename: Option<&str>,
    ascii7_only: bool,
) -> io::Result<()> {
    let mut buf = [0u8; BUF_SIZE];
    let mut accum = Vec::with_capacity(256);
    let mut file_offset: u64 = 0;
    let mut string_start_offset: u64 = 0;

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            // EOF: flush any accumulated string.
            if accum.len() >= opts.min_length {
                // SAFETY: accum only contains bytes that passed is_printable,
                // all of which are valid ASCII and therefore valid UTF-8.
                let s = unsafe { std::str::from_utf8_unchecked(&accum) };
                emit(out, opts, filename, string_start_offset, s)?;
            }
            break;
        }

        for &b in &buf[..n] {
            let printable = if ascii7_only {
                b <= 0x7F && is_printable(b, opts.include_all_whitespace)
            } else {
                is_printable(b, opts.include_all_whitespace)
            };

            if printable {
                if accum.is_empty() {
                    string_start_offset = file_offset;
                }
                accum.push(b);
            } else {
                if accum.len() >= opts.min_length {
                    let s = unsafe { std::str::from_utf8_unchecked(&accum) };
                    emit(out, opts, filename, string_start_offset, s)?;
                }
                accum.clear();
            }
            file_offset += 1;
        }
    }

    Ok(())
}

// ============================================================================
// Wide-character scanning (encodings: b, l, B, L)
// ============================================================================

/// Scan a stream using a multi-byte encoding (16-bit or 32-bit).
///
/// Reads `unit_size` bytes at a time and assembles them into a codepoint
/// using the specified byte order. If the codepoint is printable, its UTF-8
/// representation is appended to the accumulator.
fn scan_wide(
    reader: &mut dyn Read,
    out: &mut impl Write,
    opts: &Options,
    filename: Option<&str>,
    unit_size: usize,
    big_endian: bool,
) -> io::Result<()> {
    let mut buf = [0u8; BUF_SIZE];
    let mut accum = Vec::<u8>::with_capacity(256);
    let mut file_offset: u64 = 0;
    let mut string_start_offset: u64 = 0;

    // Leftover bytes from the previous read that did not form a complete unit.
    let mut leftover = Vec::with_capacity(unit_size);

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            // EOF: flush any accumulated string.
            if accum.len() >= opts.min_length {
                let s = core::str::from_utf8(&accum).unwrap_or("");
                if s.len() >= opts.min_length {
                    emit(out, opts, filename, string_start_offset, s)?;
                }
            }
            break;
        }

        let data = &buf[..n];
        let mut pos = 0;

        // If there are leftover bytes from the previous iteration, complete
        // the unit with bytes from the new buffer.
        if !leftover.is_empty() {
            let needed = unit_size - leftover.len();
            let avail = data.len().min(needed);
            leftover.extend_from_slice(&data[..avail]);
            pos = avail;

            if leftover.len() == unit_size {
                let val = decode_unit(&leftover, big_endian);
                process_wide_value(
                    val,
                    opts,
                    out,
                    filename,
                    &mut accum,
                    &mut string_start_offset,
                    file_offset,
                )?;
                file_offset += unit_size as u64;
                leftover.clear();
            }
        }

        // Process complete units from the buffer.
        while pos + unit_size <= data.len() {
            let val = decode_unit(&data[pos..pos + unit_size], big_endian);
            process_wide_value(
                val,
                opts,
                out,
                filename,
                &mut accum,
                &mut string_start_offset,
                file_offset,
            )?;
            file_offset += unit_size as u64;
            pos += unit_size;
        }

        // Stash any remaining bytes that do not form a complete unit.
        if pos < data.len() {
            leftover.extend_from_slice(&data[pos..]);
        }
    }

    Ok(())
}

/// Decode a multi-byte unit into a `u32` value.
fn decode_unit(bytes: &[u8], big_endian: bool) -> u32 {
    match bytes.len() {
        2 => {
            if big_endian {
                u32::from(u16::from_be_bytes([bytes[0], bytes[1]]))
            } else {
                u32::from(u16::from_le_bytes([bytes[0], bytes[1]]))
            }
        }
        4 => {
            if big_endian {
                u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
            } else {
                u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
            }
        }
        _ => 0,
    }
}

/// Process a single decoded wide-character value, appending to the accumulator
/// or flushing a completed string.
fn process_wide_value(
    val: u32,
    opts: &Options,
    out: &mut impl Write,
    filename: Option<&str>,
    accum: &mut Vec<u8>,
    string_start_offset: &mut u64,
    current_offset: u64,
) -> io::Result<()> {
    if is_wide_printable(val, opts.include_all_whitespace) {
        if accum.is_empty() {
            *string_start_offset = current_offset;
        }
        // Encode the codepoint as UTF-8 into the accumulator.
        #[allow(clippy::cast_possible_truncation)]
        let ch = char::from(val as u8);
        let mut utf8_buf = [0u8; 4];
        let encoded = ch.encode_utf8(&mut utf8_buf);
        accum.extend_from_slice(encoded.as_bytes());
    } else {
        // Non-printable: check if accumulated run is long enough.
        let s = core::str::from_utf8(accum).unwrap_or("");
        if s.chars().count() >= opts.min_length {
            emit(out, opts, filename, *string_start_offset, s)?;
        }
        accum.clear();
    }
    Ok(())
}

// ============================================================================
// Scanning dispatch
// ============================================================================

/// Scan a reader with the configured encoding.
fn scan(
    reader: &mut dyn Read,
    out: &mut impl Write,
    opts: &Options,
    filename: Option<&str>,
) -> io::Result<()> {
    match opts.encoding {
        Encoding::Ascii7 => scan_bytes(reader, out, opts, filename, true),
        Encoding::Ascii8 => scan_bytes(reader, out, opts, filename, false),
        Encoding::BigEndian16 => {
            scan_wide(reader, out, opts, filename, 2, true)
        }
        Encoding::LittleEndian16 => {
            scan_wide(reader, out, opts, filename, 2, false)
        }
        Encoding::BigEndian32 => {
            scan_wide(reader, out, opts, filename, 4, true)
        }
        Encoding::LittleEndian32 => {
            scan_wide(reader, out, opts, filename, 4, false)
        }
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn run() -> Result<(), String> {
    let opts = parse_args()?;
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    if opts.files.is_empty() {
        // Read from stdin.
        let mut stdin = io::stdin().lock();
        scan(&mut stdin, &mut out, &opts, None)
            .map_err(|e| format!("stdin: {e}"))?;
    } else {
        for path in &opts.files {
            if path == "-" {
                let mut stdin = io::stdin().lock();
                let name = if opts.print_filename {
                    Some("{standard input}")
                } else {
                    None
                };
                scan(&mut stdin, &mut out, &opts, name)
                    .map_err(|e| format!("stdin: {e}"))?;
            } else {
                let mut file = File::open(path)
                    .map_err(|e| format!("{path}: {e}"))?;
                let name = if opts.print_filename {
                    Some(path.as_str())
                } else {
                    None
                };
                scan(&mut file, &mut out, &opts, name)
                    .map_err(|e| format!("{path}: {e}"))?;
            }
        }
    }

    out.flush().map_err(|e| format!("write error: {e}"))?;
    Ok(())
}

fn main() {
    if let Err(msg) = run() {
        eprintln!("strings: {msg}");
        process::exit(1);
    }
}
