//! dd — convert and copy a file.
//!
//! Usage: dd [if=FILE] [of=FILE] [bs=N] [count=N] [skip=N] [seek=N]
//!   if=     input file (default: stdin)
//!   of=     output file (default: stdout)
//!   bs=     block size in bytes (default: 512)
//!   count=  number of blocks to copy
//!   skip=   skip N blocks at start of input
//!   seek=   skip N blocks at start of output

use std::env;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::process;
use std::time::Instant;

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct DdOperands {
    input_file: Option<String>,
    output_file: Option<String>,
    bs: usize,
    count: Option<usize>,
    skip: usize,
    seek: usize,
}

impl Default for DdOperands {
    fn default() -> Self {
        Self {
            input_file: None,
            output_file: None,
            bs: 512,
            count: None,
            skip: 0,
            seek: 0,
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let ops = match parse_operands(&args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("dd: {e}");
            process::exit(1);
        }
    };

    let bs = ops.bs;
    let mut reader: Box<dyn Read> = match &ops.input_file {
        Some(path) => match File::open(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("dd: failed to open '{path}': {e}");
                process::exit(1);
            }
        },
        None => Box::new(io::stdin()),
    };

    let mut writer: Box<dyn Write> = match &ops.output_file {
        Some(path) => match OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
        {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("dd: failed to open '{path}': {e}");
                process::exit(1);
            }
        },
        None => Box::new(io::stdout()),
    };

    if ops.skip > 0 {
        let skip_bytes = ops.skip.saturating_mul(bs);
        if let Some(f) = ops.input_file.as_ref() {
            if let Ok(mut fh) = File::open(f)
                && fh.seek(SeekFrom::Start(skip_bytes as u64)).is_ok()
            {
                reader = Box::new(fh);
            }
        } else {
            let mut discard = vec![0u8; bs];
            for _ in 0..ops.skip {
                if reader.read(&mut discard).unwrap_or(0) == 0 {
                    break;
                }
            }
        }
    }

    if ops.seek > 0 {
        let seek_bytes = ops.seek.saturating_mul(bs);
        // dd with `seek=` preserves the existing tail of the output file —
        // it positions the cursor and overwrites in place, so truncate(false)
        // is the correct semantic (not truncate(true)).
        if let Some(f) = ops.output_file.as_ref()
            && let Ok(mut fh) = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(f)
            && fh.seek(SeekFrom::Start(seek_bytes as u64)).is_ok()
        {
            writer = Box::new(fh);
        }
    }

    let start = Instant::now();
    let mut buf = vec![0u8; bs];
    let mut blocks_in: usize = 0;
    let mut blocks_out: usize = 0;
    let mut partial_in: usize = 0;
    let mut partial_out: usize = 0;
    let mut total_bytes: u64 = 0;

    loop {
        if let Some(c) = ops.count
            && blocks_in.saturating_add(partial_in) >= c
        {
            break;
        }

        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                eprintln!("dd: read error: {e}");
                process::exit(1);
            }
        };

        if n == bs {
            blocks_in = blocks_in.saturating_add(1);
        } else {
            partial_in = partial_in.saturating_add(1);
        }

        match writer.write_all(buf.get(..n).unwrap_or(&[])) {
            Ok(()) => {
                if n == bs {
                    blocks_out = blocks_out.saturating_add(1);
                } else {
                    partial_out = partial_out.saturating_add(1);
                }
                total_bytes = total_bytes.saturating_add(n as u64);
            }
            Err(e) => {
                eprintln!("dd: write error: {e}");
                process::exit(1);
            }
        }
    }

    let _ = writer.flush();
    let elapsed = start.elapsed();
    let secs = elapsed.as_secs_f64();

    eprintln!("{blocks_in}+{partial_in} records in");
    eprintln!("{blocks_out}+{partial_out} records out");
    eprintln!("{}", format_rate_line(total_bytes, secs));
}

/// Parse dd's `key=value` operands.
fn parse_operands(args: &[String]) -> Result<DdOperands, String> {
    let mut ops = DdOperands::default();
    for arg in args {
        let Some((key, val)) = arg.split_once('=') else {
            return Err(format!("unrecognized argument: {arg}"));
        };
        match key {
            "if" => ops.input_file = Some(val.to_string()),
            "of" => ops.output_file = Some(val.to_string()),
            "bs" => ops.bs = parse_size(val),
            "count" => ops.count = Some(parse_size(val)),
            "skip" => ops.skip = parse_size(val),
            "seek" => ops.seek = parse_size(val),
            _ => return Err(format!("unknown operand: {key}")),
        }
    }
    Ok(ops)
}

/// Parse a size string with optional suffix: k/K (1024), m/M (1048576),
/// g/G (1073741824). Plain number = bytes.
fn parse_size(s: &str) -> usize {
    let s = s.trim();
    if s.is_empty() {
        return 0;
    }

    let (num_str, multiplier): (&str, usize) = match s.as_bytes().last() {
        Some(b'k' | b'K') => (s.get(..s.len().saturating_sub(1)).unwrap_or(""), 1024),
        Some(b'm' | b'M') => (
            s.get(..s.len().saturating_sub(1)).unwrap_or(""),
            1024 * 1024,
        ),
        Some(b'g' | b'G') => (
            s.get(..s.len().saturating_sub(1)).unwrap_or(""),
            1024 * 1024 * 1024,
        ),
        _ => (s, 1),
    };

    num_str.parse::<usize>().unwrap_or(0).saturating_mul(multiplier)
}

/// Format dd's final progress line based on total bytes and elapsed time.
fn format_rate_line(total_bytes: u64, secs: f64) -> String {
    if secs <= 0.0 {
        return format!("{total_bytes} bytes copied");
    }
    let rate = total_bytes as f64 / secs;
    if rate >= 1_000_000_000.0 {
        format!(
            "{total_bytes} bytes ({:.1} GB) copied, {secs:.6} s, {:.1} GB/s",
            total_bytes as f64 / 1e9,
            rate / 1e9
        )
    } else if rate >= 1_000_000.0 {
        format!(
            "{total_bytes} bytes ({:.1} MB) copied, {secs:.6} s, {:.1} MB/s",
            total_bytes as f64 / 1e6,
            rate / 1e6
        )
    } else {
        format!("{total_bytes} bytes copied, {secs:.6} s, {rate:.0} bytes/s")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn parse_size_plain() {
        assert_eq!(parse_size("100"), 100);
    }

    #[test]
    fn parse_size_k_suffix() {
        assert_eq!(parse_size("1k"), 1024);
        assert_eq!(parse_size("4K"), 4 * 1024);
    }

    #[test]
    fn parse_size_m_suffix() {
        assert_eq!(parse_size("1m"), 1024 * 1024);
        assert_eq!(parse_size("2M"), 2 * 1024 * 1024);
    }

    #[test]
    fn parse_size_g_suffix() {
        assert_eq!(parse_size("1g"), 1024 * 1024 * 1024);
        assert_eq!(parse_size("1G"), 1024 * 1024 * 1024);
    }

    #[test]
    fn parse_size_empty_zero() {
        assert_eq!(parse_size(""), 0);
    }

    #[test]
    fn parse_size_garbage_zero() {
        assert_eq!(parse_size("notanumber"), 0);
    }

    #[test]
    fn parse_size_zero_plain() {
        assert_eq!(parse_size("0"), 0);
    }

    #[test]
    fn parse_size_trims_whitespace() {
        assert_eq!(parse_size("  100  "), 100);
    }

    #[test]
    fn parse_operands_defaults() {
        let o = parse_operands(&s(&[])).unwrap();
        assert_eq!(o, DdOperands::default());
    }

    #[test]
    fn parse_operands_if_of() {
        let o = parse_operands(&s(&["if=a.bin", "of=b.bin"])).unwrap();
        assert_eq!(o.input_file.as_deref(), Some("a.bin"));
        assert_eq!(o.output_file.as_deref(), Some("b.bin"));
    }

    #[test]
    fn parse_operands_bs_count_skip_seek() {
        let o = parse_operands(&s(&["bs=4k", "count=10", "skip=1", "seek=2"])).unwrap();
        assert_eq!(o.bs, 4096);
        assert_eq!(o.count, Some(10));
        assert_eq!(o.skip, 1);
        assert_eq!(o.seek, 2);
    }

    #[test]
    fn parse_operands_unknown_operand_errors() {
        let err = parse_operands(&s(&["nope=1"])).unwrap_err();
        assert!(err.contains("unknown operand"));
    }

    #[test]
    fn parse_operands_no_equals_errors() {
        let err = parse_operands(&s(&["badarg"])).unwrap_err();
        assert!(err.contains("unrecognized"));
    }

    #[test]
    fn parse_operands_value_with_embedded_equals() {
        // split_once on '=' splits on the first '=' only.
        let o = parse_operands(&s(&["if=a=b.bin"])).unwrap();
        assert_eq!(o.input_file.as_deref(), Some("a=b.bin"));
    }

    #[test]
    fn rate_line_zero_elapsed() {
        assert_eq!(format_rate_line(1000, 0.0), "1000 bytes copied");
    }

    #[test]
    fn rate_line_negative_elapsed() {
        assert_eq!(format_rate_line(1000, -1.0), "1000 bytes copied");
    }

    #[test]
    fn rate_line_bytes_per_sec() {
        // 100 bytes in 1 second = 100 bytes/s
        let line = format_rate_line(100, 1.0);
        assert!(line.contains("100 bytes copied"));
        assert!(line.contains("1.000000 s"));
        assert!(line.contains("100 bytes/s"));
    }

    #[test]
    fn rate_line_mb_per_sec() {
        // 5 MB in 1 second = 5 MB/s.
        let line = format_rate_line(5_000_000, 1.0);
        assert!(line.contains("(5.0 MB)"));
        assert!(line.contains("5.0 MB/s"));
    }

    #[test]
    fn rate_line_gb_per_sec() {
        let line = format_rate_line(2_000_000_000, 1.0);
        assert!(line.contains("(2.0 GB)"));
        assert!(line.contains("2.0 GB/s"));
    }
}
