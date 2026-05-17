//! printf — format and print data.
//!
//! Usage: printf FORMAT [ARGUMENT...]
//!   Interprets C-style format specifiers: %s, %d, %x, %o, %c, %%.
//!   Interprets escape sequences: \n, \t, \\, \0NNN (octal).

use std::env;
use std::io::{self, Write};
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!("printf: missing format operand");
        process::exit(1);
    }

    let format = &args[0];
    let arguments = &args[1..];
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let fmt_bytes = format.as_bytes();
    let mut fi = 0; // format index
    let mut ai = 0; // argument index

    while fi < fmt_bytes.len() {
        if fmt_bytes[fi] == b'%' {
            fi += 1;
            if fi >= fmt_bytes.len() {
                let _ = out.write_all(b"%");
                break;
            }
            match fmt_bytes[fi] {
                b's' => {
                    let arg = arg_str(arguments, ai);
                    let _ = write!(out, "{arg}");
                    ai += 1;
                }
                b'd' | b'i' => {
                    let arg = arg_str(arguments, ai);
                    let n: i64 = arg.parse().unwrap_or(0);
                    let _ = write!(out, "{n}");
                    ai += 1;
                }
                b'u' => {
                    let arg = arg_str(arguments, ai);
                    let n: u64 = arg.parse().unwrap_or(0);
                    let _ = write!(out, "{n}");
                    ai += 1;
                }
                b'x' => {
                    let arg = arg_str(arguments, ai);
                    let n: u64 = arg.parse().unwrap_or(0);
                    let _ = write!(out, "{n:x}");
                    ai += 1;
                }
                b'X' => {
                    let arg = arg_str(arguments, ai);
                    let n: u64 = arg.parse().unwrap_or(0);
                    let _ = write!(out, "{n:X}");
                    ai += 1;
                }
                b'o' => {
                    let arg = arg_str(arguments, ai);
                    let n: u64 = arg.parse().unwrap_or(0);
                    let _ = write!(out, "{n:o}");
                    ai += 1;
                }
                b'c' => {
                    let arg = arg_str(arguments, ai);
                    if let Some(c) = arg.chars().next() {
                        let _ = write!(out, "{c}");
                    }
                    ai += 1;
                }
                b'%' => {
                    let _ = out.write_all(b"%");
                }
                other => {
                    // Unknown specifier — print literally.
                    let _ = out.write_all(&[b'%', other]);
                }
            }
            fi += 1;
        } else if fmt_bytes[fi] == b'\\' {
            fi += 1;
            if fi >= fmt_bytes.len() {
                let _ = out.write_all(b"\\");
                break;
            }
            match fmt_bytes[fi] {
                b'n' => {
                    let _ = out.write_all(b"\n");
                }
                b't' => {
                    let _ = out.write_all(b"\t");
                }
                b'r' => {
                    let _ = out.write_all(b"\r");
                }
                b'\\' => {
                    let _ = out.write_all(b"\\");
                }
                b'0' => {
                    // Octal escape \0NNN
                    fi += 1;
                    let mut val: u8 = 0;
                    let mut count = 0;
                    while fi < fmt_bytes.len() && count < 3 {
                        let d = fmt_bytes[fi];
                        if d >= b'0' && d <= b'7' {
                            val = val * 8 + (d - b'0');
                            fi += 1;
                            count += 1;
                        } else {
                            break;
                        }
                    }
                    let _ = out.write_all(&[val]);
                    continue; // fi already advanced
                }
                other => {
                    let _ = out.write_all(&[b'\\', other]);
                }
            }
            fi += 1;
        } else {
            let _ = out.write_all(&[fmt_bytes[fi]]);
            fi += 1;
        }
    }

    let _ = out.flush();
}

fn arg_str(args: &[String], index: usize) -> &str {
    if index < args.len() {
        &args[index]
    } else {
        ""
    }
}
