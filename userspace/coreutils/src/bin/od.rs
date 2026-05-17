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
        let addr = offset * bytes_per_line;

        // Print address
        match addr_radix {
            'o' => {
                let _ = write!(out, "{:07o}", addr);
            }
            'x' => {
                let _ = write!(out, "{:07x}", addr);
            }
            'd' => {
                let _ = write!(out, "{:07}", addr);
            }
            'n' => {} // no address
            _ => {
                let _ = write!(out, "{:07o}", addr);
            }
        }

        // Print data
        match output_type {
            'x' => {
                for &b in chunk {
                    let _ = write!(out, " {:02x}", b);
                }
            }
            'd' => {
                for &b in chunk {
                    let _ = write!(out, " {:3}", b);
                }
            }
            'c' => {
                for &b in chunk {
                    let c = match b {
                        b'\0' => "\\0".to_string(),
                        b'\n' => "\\n".to_string(),
                        b'\r' => "\\r".to_string(),
                        b'\t' => "\\t".to_string(),
                        b'\\' => "\\\\".to_string(),
                        0x20..=0x7e => format!("  {}", b as char),
                        _ => format!("{:03o}", b),
                    };
                    let _ = write!(out, " {c:>3}");
                }
            }
            _ => {
                // octal
                for &b in chunk {
                    let _ = write!(out, " {:03o}", b);
                }
            }
        }

        let _ = writeln!(out);
    }

    // Print final address
    match addr_radix {
        'o' => {
            let _ = writeln!(out, "{:07o}", all_data.len());
        }
        'x' => {
            let _ = writeln!(out, "{:07x}", all_data.len());
        }
        'd' => {
            let _ = writeln!(out, "{:07}", all_data.len());
        }
        _ => {}
    }
}
