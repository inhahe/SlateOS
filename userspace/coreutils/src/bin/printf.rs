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
    let buf = format_into_bytes(format, arguments);

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let _ = out.write_all(&buf);
    let _ = out.flush();
}

/// Apply printf-style formatting to `format` using `arguments` and return
/// the resulting byte buffer. Pure helper — unit-testable without I/O.
///
/// Recognises %s, %d/%i, %u, %x/%X, %o, %c, %% specifiers and \n, \t, \r,
/// \\, and \0NNN octal escape sequences. Unknown specifiers are emitted
/// literally.
fn format_into_bytes(format: &str, arguments: &[String]) -> Vec<u8> {
    let mut out = Vec::with_capacity(format.len());
    let fmt_bytes = format.as_bytes();
    let mut fi = 0; // format index
    let mut ai = 0; // argument index

    while fi < fmt_bytes.len() {
        if fmt_bytes[fi] == b'%' {
            fi += 1;
            if fi >= fmt_bytes.len() {
                out.push(b'%');
                break;
            }
            match fmt_bytes[fi] {
                b's' => {
                    out.extend_from_slice(arg_str(arguments, ai).as_bytes());
                    ai += 1;
                }
                b'd' | b'i' => {
                    let n: i64 = arg_str(arguments, ai).parse().unwrap_or(0);
                    out.extend_from_slice(n.to_string().as_bytes());
                    ai += 1;
                }
                b'u' => {
                    let n: u64 = arg_str(arguments, ai).parse().unwrap_or(0);
                    out.extend_from_slice(n.to_string().as_bytes());
                    ai += 1;
                }
                b'x' => {
                    let n: u64 = arg_str(arguments, ai).parse().unwrap_or(0);
                    out.extend_from_slice(format!("{n:x}").as_bytes());
                    ai += 1;
                }
                b'X' => {
                    let n: u64 = arg_str(arguments, ai).parse().unwrap_or(0);
                    out.extend_from_slice(format!("{n:X}").as_bytes());
                    ai += 1;
                }
                b'o' => {
                    let n: u64 = arg_str(arguments, ai).parse().unwrap_or(0);
                    out.extend_from_slice(format!("{n:o}").as_bytes());
                    ai += 1;
                }
                b'c' => {
                    if let Some(c) = arg_str(arguments, ai).chars().next() {
                        out.extend_from_slice(c.to_string().as_bytes());
                    }
                    ai += 1;
                }
                b'%' => {
                    out.push(b'%');
                }
                other => {
                    // Unknown specifier — print literally.
                    out.push(b'%');
                    out.push(other);
                }
            }
            fi += 1;
        } else if fmt_bytes[fi] == b'\\' {
            fi += 1;
            if fi >= fmt_bytes.len() {
                out.push(b'\\');
                break;
            }
            match fmt_bytes[fi] {
                b'n' => out.push(b'\n'),
                b't' => out.push(b'\t'),
                b'r' => out.push(b'\r'),
                b'\\' => out.push(b'\\'),
                b'0' => {
                    // Octal escape \0NNN
                    fi += 1;
                    let mut val: u8 = 0;
                    let mut count = 0;
                    while fi < fmt_bytes.len() && count < 3 {
                        let d = fmt_bytes[fi];
                        if (b'0'..=b'7').contains(&d) {
                            val = val.wrapping_mul(8).wrapping_add(d - b'0');
                            fi += 1;
                            count += 1;
                        } else {
                            break;
                        }
                    }
                    out.push(val);
                    continue; // fi already advanced
                }
                other => {
                    out.push(b'\\');
                    out.push(other);
                }
            }
            fi += 1;
        } else {
            out.push(fmt_bytes[fi]);
            fi += 1;
        }
    }

    out
}

fn arg_str(args: &[String], index: usize) -> &str {
    if index < args.len() {
        &args[index]
    } else {
        ""
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn argv(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    // ---------------- literal text ----------------

    #[test]
    fn no_format_specifiers() {
        assert_eq!(format_into_bytes("hello world", &[]), b"hello world");
    }

    #[test]
    fn empty_format() {
        assert!(format_into_bytes("", &[]).is_empty());
    }

    // ---------------- %s ----------------

    #[test]
    fn pct_s_basic() {
        assert_eq!(
            format_into_bytes("hello %s!", &argv(&["world"])),
            b"hello world!"
        );
    }

    #[test]
    fn pct_s_missing_arg_uses_empty() {
        assert_eq!(format_into_bytes("[%s]", &[]), b"[]");
    }

    #[test]
    fn pct_s_multiple() {
        assert_eq!(
            format_into_bytes("%s and %s", &argv(&["foo", "bar"])),
            b"foo and bar"
        );
    }

    // ---------------- %d / %i ----------------

    #[test]
    fn pct_d_basic() {
        assert_eq!(format_into_bytes("n=%d", &argv(&["42"])), b"n=42");
    }

    #[test]
    fn pct_i_basic() {
        assert_eq!(format_into_bytes("n=%i", &argv(&["-7"])), b"n=-7");
    }

    #[test]
    fn pct_d_invalid_uses_zero() {
        assert_eq!(format_into_bytes("n=%d", &argv(&["abc"])), b"n=0");
    }

    // ---------------- %u ----------------

    #[test]
    fn pct_u_basic() {
        assert_eq!(format_into_bytes("u=%u", &argv(&["100"])), b"u=100");
    }

    #[test]
    fn pct_u_negative_uses_zero() {
        // Can't parse a negative number as u64 — falls back to 0.
        assert_eq!(format_into_bytes("u=%u", &argv(&["-5"])), b"u=0");
    }

    // ---------------- %x %X %o ----------------

    #[test]
    fn pct_x_lowercase() {
        assert_eq!(format_into_bytes("%x", &argv(&["255"])), b"ff");
    }

    #[test]
    fn pct_x_uppercase() {
        assert_eq!(format_into_bytes("%X", &argv(&["255"])), b"FF");
    }

    #[test]
    fn pct_o_octal() {
        assert_eq!(format_into_bytes("%o", &argv(&["8"])), b"10");
        assert_eq!(format_into_bytes("%o", &argv(&["64"])), b"100");
    }

    // ---------------- %c ----------------

    #[test]
    fn pct_c_first_char() {
        assert_eq!(format_into_bytes("%c", &argv(&["abc"])), b"a");
    }

    #[test]
    fn pct_c_empty_arg() {
        assert_eq!(format_into_bytes("[%c]", &argv(&[""])), b"[]");
    }

    // ---------------- %% ----------------

    #[test]
    fn pct_percent_literal() {
        assert_eq!(format_into_bytes("100%%", &[]), b"100%");
    }

    #[test]
    fn trailing_percent_kept_literal() {
        // Trailing lone % with no spec letter — emitted as %.
        assert_eq!(format_into_bytes("x%", &[]), b"x%");
    }

    #[test]
    fn unknown_spec_emitted_literally() {
        // %z is unknown — emit "%z" verbatim.
        assert_eq!(format_into_bytes("%z", &[]), b"%z");
    }

    // ---------------- escape sequences ----------------

    #[test]
    fn escape_newline_tab_cr() {
        assert_eq!(format_into_bytes("a\\nb\\tc\\rd", &[]), b"a\nb\tc\rd");
    }

    #[test]
    fn escape_backslash() {
        assert_eq!(format_into_bytes("a\\\\b", &[]), b"a\\b");
    }

    #[test]
    fn escape_octal_3_digit() {
        // \0101 -> 0o101 = 65 = 'A'
        assert_eq!(format_into_bytes("\\0101", &[]), b"A");
    }

    #[test]
    fn escape_octal_short() {
        // \07 -> 0o7 = 7 (BEL)
        assert_eq!(format_into_bytes("x\\07y", &[]), [b'x', 7u8, b'y']);
    }

    #[test]
    fn escape_octal_null() {
        // \0 with no following digits -> NUL byte.
        assert_eq!(format_into_bytes("a\\0b", &[]), [b'a', 0u8, b'b']);
    }

    #[test]
    fn escape_unknown_passes_through() {
        // \z is not recognized — print backslash + z verbatim.
        assert_eq!(format_into_bytes("a\\zb", &[]), b"a\\zb");
    }

    #[test]
    fn escape_trailing_backslash_kept() {
        assert_eq!(format_into_bytes("a\\", &[]), b"a\\");
    }

    // ---------------- arg_str ----------------

    #[test]
    fn arg_str_in_range() {
        let v = argv(&["a", "b"]);
        assert_eq!(arg_str(&v, 0), "a");
        assert_eq!(arg_str(&v, 1), "b");
    }

    #[test]
    fn arg_str_out_of_range_empty() {
        assert_eq!(arg_str(&[], 0), "");
        let v = argv(&["a"]);
        assert_eq!(arg_str(&v, 5), "");
    }

    #[test]
    fn combined_format_and_escapes() {
        assert_eq!(
            format_into_bytes("Name: %s\\nAge: %d\\n", &argv(&["Alice", "30"])),
            b"Name: Alice\nAge: 30\n"
        );
    }
}
