//! split — split a file into pieces.
//!
//! Usage: split [-l LINES] [-b BYTES] [-a SUFFIX_LEN] [FILE [PREFIX]]
//!   -l LINES       put LINES lines per output file (default: 1000)
//!   -b BYTES       put at most BYTES bytes per output file
//!                  suffixes: k (1024), m (1048576), g (1073741824)
//!   -a SUFFIX_LEN  use suffixes of length N (default: 2)
//!   FILE           input file (default: stdin, or "-" for stdin)
//!   PREFIX         output file prefix (default: "x")
//!
//! Output files are named PREFIXaa, PREFIXab, ..., PREFIXzz.
//! With -a 3: PREFIXaaa, PREFIXaab, etc.
//!
//! Exit codes:
//!   0  success
//!   1  error

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::process;

/// Generate the suffix string for a given index and suffix length.
/// index 0 -> "aa", index 1 -> "ab", ..., index 25 -> "az",
/// index 26 -> "ba", etc.
fn suffix_for(index: usize, suffix_len: usize) -> Option<String> {
    let total = 26usize.checked_pow(suffix_len as u32)?;
    if index >= total {
        return None;
    }

    let mut result = vec![0u8; suffix_len];
    let mut remaining = index;
    for pos in (0..suffix_len).rev() {
        result[pos] = b'a' + (remaining % 26) as u8;
        remaining /= 26;
    }

    Some(String::from_utf8(result).unwrap_or_default())
}

/// Parse a byte count with optional suffix (k, m, g).
fn parse_bytes(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let (num_str, multiplier) = match s.as_bytes().last() {
        Some(b'k' | b'K') => (&s[..s.len() - 1], 1024u64),
        Some(b'm' | b'M') => (&s[..s.len() - 1], 1024 * 1024),
        Some(b'g' | b'G') => (&s[..s.len() - 1], 1024 * 1024 * 1024),
        _ => (s, 1u64),
    };

    num_str.parse::<u64>().ok().map(|n| n * multiplier)
}

enum SplitMode {
    Lines(usize),
    Bytes(u64),
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut mode = SplitMode::Lines(1000);
    let mut suffix_len: usize = 2;
    let mut input_file: Option<String> = None;
    let mut prefix = "x".to_string();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-l" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("split: option -l requires an argument");
                    process::exit(1);
                }
                match args[i].parse::<usize>() {
                    Ok(n) if n > 0 => mode = SplitMode::Lines(n),
                    _ => {
                        eprintln!("split: invalid line count: {}", args[i]);
                        process::exit(1);
                    }
                }
            }
            "-b" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("split: option -b requires an argument");
                    process::exit(1);
                }
                match parse_bytes(&args[i]) {
                    Some(n) if n > 0 => mode = SplitMode::Bytes(n),
                    _ => {
                        eprintln!("split: invalid byte count: {}", args[i]);
                        process::exit(1);
                    }
                }
            }
            "-a" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("split: option -a requires an argument");
                    process::exit(1);
                }
                match args[i].parse::<usize>() {
                    Ok(n) if n > 0 => suffix_len = n,
                    _ => {
                        eprintln!("split: invalid suffix length: {}", args[i]);
                        process::exit(1);
                    }
                }
            }
            arg if arg.starts_with('-') && arg.len() > 1 => {
                eprintln!("split: unknown option: {arg}");
                process::exit(1);
            }
            _ => {
                if input_file.is_none() {
                    input_file = Some(args[i].clone());
                } else {
                    prefix = args[i].clone();
                }
            }
        }
        i += 1;
    }

    let reader: Box<dyn Read> = match input_file.as_deref() {
        None | Some("-") => Box::new(io::stdin()),
        Some(path) => match File::open(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("split: {path}: {e}");
                process::exit(1);
            }
        },
    };

    match mode {
        SplitMode::Lines(n) => split_by_lines(reader, &prefix, n, suffix_len),
        SplitMode::Bytes(n) => split_by_bytes(reader, &prefix, n, suffix_len),
    }
}

fn split_by_lines(reader: Box<dyn Read>, prefix: &str, lines_per_file: usize, suffix_len: usize) {
    let buf = BufReader::new(reader);
    let mut file_index = 0;
    let mut line_count = 0;
    let mut writer: Option<BufWriter<File>> = None;

    for line in buf.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("split: read error: {e}");
                process::exit(1);
            }
        };

        if line_count == 0 || writer.is_none() {
            let suffix = match suffix_for(file_index, suffix_len) {
                Some(s) => s,
                None => {
                    eprintln!("split: output file suffixes exhausted");
                    process::exit(1);
                }
            };
            let filename = format!("{prefix}{suffix}");
            writer = match File::create(&filename) {
                Ok(f) => Some(BufWriter::new(f)),
                Err(e) => {
                    eprintln!("split: {filename}: {e}");
                    process::exit(1);
                }
            };
        }

        if let Some(ref mut w) = writer
            && writeln!(w, "{line}").is_err() {
                eprintln!("split: write error");
                process::exit(1);
            }

        line_count += 1;
        if line_count >= lines_per_file {
            line_count = 0;
            file_index += 1;
            writer = None;
        }
    }
}

fn split_by_bytes(reader: Box<dyn Read>, prefix: &str, bytes_per_file: u64, suffix_len: usize) {
    let mut buf_reader = BufReader::new(reader);
    let mut file_index = 0;
    let mut bytes_written: u64 = 0;
    let mut writer: Option<BufWriter<File>> = None;
    let mut buffer = [0u8; 8192];

    loop {
        let n = match buf_reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                eprintln!("split: read error: {e}");
                process::exit(1);
            }
        };

        let mut offset = 0;
        while offset < n {
            if writer.is_none() {
                let suffix = match suffix_for(file_index, suffix_len) {
                    Some(s) => s,
                    None => {
                        eprintln!("split: output file suffixes exhausted");
                        process::exit(1);
                    }
                };
                let filename = format!("{prefix}{suffix}");
                writer = match File::create(&filename) {
                    Ok(f) => Some(BufWriter::new(f)),
                    Err(e) => {
                        eprintln!("split: {filename}: {e}");
                        process::exit(1);
                    }
                };
                bytes_written = 0;
            }

            let remaining_in_file = (bytes_per_file - bytes_written) as usize;
            let chunk = remaining_in_file.min(n - offset);

            if let Some(ref mut w) = writer
                && w.write_all(&buffer[offset..offset + chunk]).is_err() {
                    eprintln!("split: write error");
                    process::exit(1);
                }

            bytes_written += chunk as u64;
            offset += chunk;

            if bytes_written >= bytes_per_file {
                writer = None;
                file_index += 1;
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    // ---------------- suffix_for ----------------

    #[test]
    fn suffix_first_position_aa() {
        assert_eq!(suffix_for(0, 2), Some("aa".to_string()));
    }

    #[test]
    fn suffix_second_position_ab() {
        assert_eq!(suffix_for(1, 2), Some("ab".to_string()));
    }

    #[test]
    fn suffix_az_at_index_25() {
        assert_eq!(suffix_for(25, 2), Some("az".to_string()));
    }

    #[test]
    fn suffix_ba_after_az() {
        assert_eq!(suffix_for(26, 2), Some("ba".to_string()));
    }

    #[test]
    fn suffix_zz_at_max() {
        // 26*26 - 1 = 675 is the last suffix for len=2.
        assert_eq!(suffix_for(675, 2), Some("zz".to_string()));
    }

    #[test]
    fn suffix_exhausted_returns_none() {
        // 26*26 = 676 is out of range.
        assert_eq!(suffix_for(676, 2), None);
    }

    #[test]
    fn suffix_length_three_aaa() {
        assert_eq!(suffix_for(0, 3), Some("aaa".to_string()));
    }

    #[test]
    fn suffix_length_three_aab() {
        assert_eq!(suffix_for(1, 3), Some("aab".to_string()));
    }

    #[test]
    fn suffix_length_three_zzz_at_max() {
        // 26^3 - 1 = 17575
        assert_eq!(suffix_for(17575, 3), Some("zzz".to_string()));
    }

    #[test]
    fn suffix_length_one_a() {
        assert_eq!(suffix_for(0, 1), Some("a".to_string()));
        assert_eq!(suffix_for(25, 1), Some("z".to_string()));
        assert_eq!(suffix_for(26, 1), None);
    }

    // ---------------- parse_bytes ----------------

    #[test]
    fn bytes_plain_number() {
        assert_eq!(parse_bytes("100"), Some(100));
    }

    #[test]
    fn bytes_kilo_lowercase() {
        assert_eq!(parse_bytes("2k"), Some(2 * 1024));
    }

    #[test]
    fn bytes_kilo_uppercase() {
        assert_eq!(parse_bytes("2K"), Some(2 * 1024));
    }

    #[test]
    fn bytes_mega_lowercase() {
        assert_eq!(parse_bytes("3m"), Some(3 * 1024 * 1024));
    }

    #[test]
    fn bytes_mega_uppercase() {
        assert_eq!(parse_bytes("3M"), Some(3 * 1024 * 1024));
    }

    #[test]
    fn bytes_giga_lowercase() {
        assert_eq!(parse_bytes("1g"), Some(1024 * 1024 * 1024));
    }

    #[test]
    fn bytes_giga_uppercase() {
        assert_eq!(parse_bytes("1G"), Some(1024 * 1024 * 1024));
    }

    #[test]
    fn bytes_zero_allowed_at_parse_level() {
        // parse_bytes itself returns Some(0); the >0 check happens in main.
        assert_eq!(parse_bytes("0"), Some(0));
    }

    #[test]
    fn bytes_empty_returns_none() {
        assert_eq!(parse_bytes(""), None);
        assert_eq!(parse_bytes("   "), None);
    }

    #[test]
    fn bytes_invalid_returns_none() {
        assert_eq!(parse_bytes("abc"), None);
        assert_eq!(parse_bytes("12abc"), None);
    }

    #[test]
    fn bytes_trims_whitespace() {
        assert_eq!(parse_bytes(" 100 "), Some(100));
    }

    #[test]
    fn bytes_suffix_only_returns_none() {
        // "k" with no number portion: num_str is "" -> parse fails.
        assert_eq!(parse_bytes("k"), None);
    }
}
