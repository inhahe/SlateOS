//! od — dump files in octal and other formats.
//!
//! Usage: od [-A RADIX] [-t TYPE] [-N COUNT] [FILE...]
//!   -A RADIX   address radix: o (octal, default), x (hex), d (decimal), n (none)
//!   -t TYPE    output type: o (octal), x (hex), d (decimal), c (char)
//!   -N COUNT   read only COUNT bytes
//!   Default: octal dump with octal addresses.

use std::env;
use std::fs::File;
use std::io::{self, Read, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut addr_radix = 'o';
    let mut output_type = 'o';
    let mut max_bytes: Option<usize> = None;
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-A" => {
                i += 1;
                if i < args.len() {
                    addr_radix = args[i].chars().next().unwrap_or('o');
                }
            }
            "-t" => {
                i += 1;
                if i < args.len() {
                    output_type = args[i].chars().next().unwrap_or('o');
                }
            }
            "-N" => {
                i += 1;
                if i < args.len() {
                    max_bytes = args[i].parse().ok();
                }
            }
            "-x" => output_type = 'x',
            "-c" => output_type = 'c',
            "-d" => output_type = 'd',
            arg => files.push(arg.to_string()),
        }
        i += 1;
    }

    if files.is_empty() {
        files.push("-".to_string());
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let mut all_data = Vec::new();
    for path in &files {
        let mut reader: Box<dyn Read> = if path == "-" {
            Box::new(io::stdin())
        } else {
            match File::open(path) {
                Ok(f) => Box::new(f),
                Err(e) => {
                    eprintln!("od: {path}: {e}");
                    continue;
                }
            }
        };

        let mut buf = Vec::new();
        let _ = reader.read_to_end(&mut buf);
        all_data.extend_from_slice(&buf);
    }

    if let Some(max) = max_bytes {
        all_data.truncate(max);
    }

    let bytes_per_line = 16;

    for (offset, chunk) in all_data.chunks(bytes_per_line).enumerate() {
        let addr = offset.saturating_mul(bytes_per_line);
        let _ = writeln!(
            out,
            "{}{}",
            format_address(addr, addr_radix),
            format_data(chunk, output_type)
        );
    }

    // Print final address
    if addr_radix != 'n' {
        let _ = writeln!(out, "{}", format_address(all_data.len(), addr_radix));
    }
}

/// Format the address column for one row of `od` output.
///
/// Returns the empty string when `radix == 'n'` (suppress addresses).
/// Unknown radices default to octal, matching POSIX `od`.
fn format_address(addr: usize, radix: char) -> String {
    match radix {
        'x' => format!("{addr:07x}"),
        'd' => format!("{addr:07}"),
        'n' => String::new(),
        // 'o' and unknown -> octal default.
        _ => format!("{addr:07o}"),
    }
}

/// Format one byte for the data column according to `output_type`.
fn format_byte(b: u8, output_type: char) -> String {
    match output_type {
        'x' => format!(" {b:02x}"),
        'd' => format!(" {b:3}"),
        'c' => {
            let c = match b {
                b'\0' => "\\0".to_string(),
                b'\n' => "\\n".to_string(),
                b'\r' => "\\r".to_string(),
                b'\t' => "\\t".to_string(),
                b'\\' => "\\\\".to_string(),
                0x20..=0x7e => format!("  {}", b as char),
                _ => format!("{b:03o}"),
            };
            format!(" {c:>3}")
        }
        // 'o' and unknown -> octal default.
        _ => format!(" {b:03o}"),
    }
}

/// Format the full data section for one row.
fn format_data(chunk: &[u8], output_type: char) -> String {
    let mut s = String::new();
    for &b in chunk {
        s.push_str(&format_byte(b, output_type));
    }
    s
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    // ---------------- format_address ----------------

    #[test]
    fn address_octal_zero_padded() {
        assert_eq!(format_address(0, 'o'), "0000000");
        assert_eq!(format_address(16, 'o'), "0000020");
    }

    #[test]
    fn address_hex_zero_padded() {
        assert_eq!(format_address(255, 'x'), "00000ff");
    }

    #[test]
    fn address_decimal_zero_padded() {
        assert_eq!(format_address(42, 'd'), "0000042");
    }

    #[test]
    fn address_none_returns_empty() {
        assert_eq!(format_address(123, 'n'), "");
    }

    #[test]
    fn address_unknown_radix_defaults_to_octal() {
        assert_eq!(format_address(8, '?'), "0000010");
    }

    #[test]
    fn address_large_overflows_width() {
        // Field width is minimum, not maximum.
        // 0o12345670 (octal) -> formatted as "12345670"
        assert_eq!(format_address(0o12345670, 'o'), "12345670");
    }

    // ---------------- format_byte ----------------

    #[test]
    fn byte_octal_default() {
        assert_eq!(format_byte(0, 'o'), " 000");
        assert_eq!(format_byte(0xff, 'o'), " 377");
        assert_eq!(format_byte(b'A', 'o'), " 101");
    }

    #[test]
    fn byte_hex_lowercase() {
        assert_eq!(format_byte(0, 'x'), " 00");
        assert_eq!(format_byte(0xab, 'x'), " ab");
    }

    #[test]
    fn byte_decimal_3wide() {
        assert_eq!(format_byte(0, 'd'), "   0");
        assert_eq!(format_byte(255, 'd'), " 255");
    }

    #[test]
    fn byte_char_printable() {
        assert_eq!(format_byte(b'A', 'c'), "   A");
    }

    #[test]
    fn byte_char_escape_sequences() {
        assert_eq!(format_byte(b'\0', 'c'), "  \\0");
        assert_eq!(format_byte(b'\n', 'c'), "  \\n");
        assert_eq!(format_byte(b'\r', 'c'), "  \\r");
        assert_eq!(format_byte(b'\t', 'c'), "  \\t");
        assert_eq!(format_byte(b'\\', 'c'), "  \\\\");
    }

    #[test]
    fn byte_char_nonprintable_falls_to_octal() {
        // 0x80 isn't printable ASCII and isn't a named escape -> octal.
        assert_eq!(format_byte(0x80, 'c'), " 200");
    }

    #[test]
    fn byte_unknown_type_defaults_to_octal() {
        assert_eq!(format_byte(0o17, '?'), " 017");
    }

    // ---------------- format_data ----------------

    #[test]
    fn data_empty_chunk_empty_string() {
        assert_eq!(format_data(&[], 'o'), "");
    }

    #[test]
    fn data_octal_run() {
        assert_eq!(format_data(&[0, 1, 0xff], 'o'), " 000 001 377");
    }

    #[test]
    fn data_hex_run() {
        assert_eq!(format_data(&[0xde, 0xad], 'x'), " de ad");
    }

    #[test]
    fn data_char_run_mixed() {
        // 'A' printable, '\n' escape, 0x80 nonprintable.
        assert_eq!(format_data(&[b'A', b'\n', 0x80], 'c'), "   A  \\n 200");
    }
}
