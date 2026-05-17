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

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    let mut input_file: Option<String> = None;
    let mut output_file: Option<String> = None;
    let mut bs: usize = 512;
    let mut count: Option<usize> = None;
    let mut skip: usize = 0;
    let mut seek: usize = 0;

    for arg in &args {
        if let Some((key, val)) = arg.split_once('=') {
            match key {
                "if" => input_file = Some(val.to_string()),
                "of" => output_file = Some(val.to_string()),
                "bs" => bs = parse_size(val),
                "count" => count = Some(parse_size(val)),
                "skip" => skip = parse_size(val),
                "seek" => seek = parse_size(val),
                _ => {
                    eprintln!("dd: unknown operand: {key}");
                    process::exit(1);
                }
            }
        } else {
            eprintln!("dd: unrecognized argument: {arg}");
            process::exit(1);
        }
    }

    let mut reader: Box<dyn Read> = match &input_file {
        Some(path) => match File::open(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("dd: failed to open '{path}': {e}");
                process::exit(1);
            }
        },
        None => Box::new(io::stdin()),
    };

    let mut writer: Box<dyn Write> = match &output_file {
        Some(path) => {
            match OpenOptions::new()
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
            }
        }
        None => Box::new(io::stdout()),
    };

    // Skip input blocks
    if skip > 0 {
        let skip_bytes = skip * bs;
        // Try seek first, fall back to reading
        if let Some(f) = input_file.as_ref() {
            // Re-open is simpler than downcasting
            if let Ok(mut fh) = File::open(f) {
                if fh.seek(SeekFrom::Start(skip_bytes as u64)).is_ok() {
                    reader = Box::new(fh);
                }
            }
        } else {
            // stdin: just read and discard
            let mut discard = vec![0u8; bs];
            for _ in 0..skip {
                if reader.read(&mut discard).unwrap_or(0) == 0 {
                    break;
                }
            }
        }
    }

    // Seek output blocks
    if seek > 0 {
        let seek_bytes = seek * bs;
        if let Some(f) = output_file.as_ref() {
            if let Ok(mut fh) = OpenOptions::new().write(true).create(true).open(f) {
                if fh.seek(SeekFrom::Start(seek_bytes as u64)).is_ok() {
                    writer = Box::new(fh);
                }
            }
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
        if let Some(c) = count {
            if blocks_in + partial_in >= c {
                break;
            }
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
            blocks_in += 1;
        } else {
            partial_in += 1;
        }

        match writer.write_all(&buf[..n]) {
            Ok(()) => {
                if n == bs {
                    blocks_out += 1;
                } else {
                    partial_out += 1;
                }
                total_bytes += n as u64;
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

    if secs > 0.0 {
        let rate = total_bytes as f64 / secs;
        if rate >= 1_000_000_000.0 {
            eprintln!(
                "{total_bytes} bytes ({:.1} GB) copied, {secs:.6} s, {:.1} GB/s",
                total_bytes as f64 / 1e9,
                rate / 1e9
            );
        } else if rate >= 1_000_000.0 {
            eprintln!(
                "{total_bytes} bytes ({:.1} MB) copied, {secs:.6} s, {:.1} MB/s",
                total_bytes as f64 / 1e6,
                rate / 1e6
            );
        } else {
            eprintln!(
                "{total_bytes} bytes copied, {secs:.6} s, {:.0} bytes/s",
                rate
            );
        }
    } else {
        eprintln!("{total_bytes} bytes copied");
    }
}

/// Parse a size string with optional suffix: k/K (1024), m/M (1048576),
/// g/G (1073741824). Plain number = bytes.
fn parse_size(s: &str) -> usize {
    let s = s.trim();
    if s.is_empty() {
        return 0;
    }

    let (num_str, multiplier) = match s.as_bytes().last() {
        Some(b'k' | b'K') => (&s[..s.len() - 1], 1024),
        Some(b'm' | b'M') => (&s[..s.len() - 1], 1024 * 1024),
        Some(b'g' | b'G') => (&s[..s.len() - 1], 1024 * 1024 * 1024),
        _ => (s, 1),
    };

    num_str.parse::<usize>().unwrap_or(0) * multiplier
}
