//! echo — write arguments to standard output.
//!
//! Usage: echo [-n] [-e] [STRING...]
//!   -n  do not output trailing newline
//!   -e  enable interpretation of backslash escapes

use std::env;
use std::io::{self, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut no_newline = false;
    let mut interpret_escapes = false;
    let mut first_text = 0;

    // Parse flags (echo is unusual: stops at first non-flag arg)
    for (i, arg) in args.iter().enumerate() {
        match arg.as_str() {
            "-n" => no_newline = true,
            "-e" => interpret_escapes = true,
            "-ne" | "-en" => {
                no_newline = true;
                interpret_escapes = true;
            }
            _ => {
                first_text = i;
                break;
            }
        }
        first_text = i + 1;
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for (i, arg) in args[first_text..].iter().enumerate() {
        if i > 0 {
            let _ = out.write_all(b" ");
        }
        if interpret_escapes {
            let _ = out.write_all(&unescape(arg));
        } else {
            let _ = out.write_all(arg.as_bytes());
        }
    }

    if !no_newline {
        let _ = out.write_all(b"\n");
    }
    let _ = out.flush();
}

fn unescape(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            i += 1;
            match bytes[i] {
                b'n' => out.push(b'\n'),
                b't' => out.push(b'\t'),
                b'r' => out.push(b'\r'),
                b'\\' => out.push(b'\\'),
                b'0' => out.push(0),
                b'a' => out.push(7),  // BEL
                b'b' => out.push(8),  // BS
                b'f' => out.push(12), // FF
                other => {
                    out.push(b'\\');
                    out.push(other);
                }
            }
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    out
}
