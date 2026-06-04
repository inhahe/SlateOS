//! mkfifo — make FIFOs (named pipes).
//!
//! Usage: mkfifo [-m MODE] NAME...
//!   -m MODE   set permission mode (default: 0666, modified by umask)

use std::env;
use std::process;

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn mkfifo(path: *const u8, mode: u32) -> i32;
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct MkfifoArgs {
    mode: u32,
    names: Vec<String>,
}

impl Default for MkfifoArgs {
    fn default() -> Self {
        Self {
            mode: 0o666,
            names: Vec::new(),
        }
    }
}

/// Parse mkfifo's argv.  `-m MODE` (octal) sets `mode`; unknown args are
/// treated as fifo names.  Invalid octal in `-m` silently falls back to the
/// default 0o666, matching the existing implementation.
fn parse_args(args: &[String]) -> MkfifoArgs {
    let mut out = MkfifoArgs::default();
    let mut i: usize = 0;
    while i < args.len() {
        let Some(arg) = args.get(i) else { break };
        if arg == "-m" && i.saturating_add(1) < args.len() {
            if let Some(v) = args.get(i.saturating_add(1)) {
                out.mode = u32::from_str_radix(v, 8).unwrap_or(0o666);
            }
            i = i.saturating_add(2);
        } else {
            out.names.push(arg.clone());
            i = i.saturating_add(1);
        }
    }
    out
}

/// Convert a path string to a NUL-terminated byte buffer suitable for an FFI
/// `*const u8`.
fn to_cstring_bytes(name: &str) -> Vec<u8> {
    let mut buf: Vec<u8> = name.as_bytes().to_vec();
    buf.push(0);
    buf
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let parsed = parse_args(&args);

    if parsed.names.is_empty() {
        eprintln!("mkfifo: missing operand");
        process::exit(1);
    }

    let mut exit_code = 0;
    for name in &parsed.names {
        let c_path = to_cstring_bytes(name);
        #[cfg(target_os = "linux")]
        {
            // SAFETY: c_path is a valid null-terminated string, mode is a u32.
            let ret = unsafe { mkfifo(c_path.as_ptr(), parsed.mode) };
            if ret != 0 {
                eprintln!("mkfifo: cannot create fifo '{name}'");
                exit_code = 1;
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (c_path, parsed.mode);
            eprintln!("mkfifo: not supported on this platform");
            exit_code = 1;
        }
    }

    process::exit(exit_code);
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn args_default_mode_no_names() {
        let a = parse_args(&s(&[]));
        assert_eq!(a.mode, 0o666);
        assert!(a.names.is_empty());
    }

    #[test]
    fn args_names_only_uses_default_mode() {
        let a = parse_args(&s(&["a.fifo", "b.fifo"]));
        assert_eq!(a.mode, 0o666);
        assert_eq!(a.names, vec!["a.fifo", "b.fifo"]);
    }

    #[test]
    fn args_dash_m_sets_mode() {
        let a = parse_args(&s(&["-m", "644", "a.fifo"]));
        assert_eq!(a.mode, 0o644);
        assert_eq!(a.names, vec!["a.fifo"]);
    }

    #[test]
    fn args_dash_m_700() {
        let a = parse_args(&s(&["-m", "700", "p"]));
        assert_eq!(a.mode, 0o700);
    }

    #[test]
    fn args_dash_m_invalid_falls_back_to_default() {
        let a = parse_args(&s(&["-m", "garbage", "p"]));
        assert_eq!(a.mode, 0o666);
        assert_eq!(a.names, vec!["p"]);
    }

    #[test]
    fn args_dash_m_at_end_no_value_treated_as_name() {
        // `-m` with no value falls into the else branch and is added as a name.
        let a = parse_args(&s(&["-m"]));
        assert_eq!(a.mode, 0o666);
        assert_eq!(a.names, vec!["-m"]);
    }

    #[test]
    fn args_multiple_dash_m_uses_last() {
        let a = parse_args(&s(&["-m", "600", "-m", "755", "p"]));
        assert_eq!(a.mode, 0o755);
    }

    #[test]
    fn args_mode_with_leading_zero_octal() {
        // Already-octal "0644" still parses as octal (leading zero allowed).
        let a = parse_args(&s(&["-m", "0644", "p"]));
        assert_eq!(a.mode, 0o644);
    }

    #[test]
    fn cstring_basic() {
        let buf = to_cstring_bytes("hello");
        assert_eq!(buf, b"hello\0");
    }

    #[test]
    fn cstring_empty() {
        assert_eq!(to_cstring_bytes(""), b"\0");
    }

    #[test]
    fn cstring_unicode_passthrough() {
        let buf = to_cstring_bytes("café");
        // Last byte is the trailing NUL.
        assert_eq!(*buf.last().unwrap(), 0);
        // Everything before NUL is the original UTF-8.
        assert_eq!(&buf[..buf.len() - 1], "café".as_bytes());
    }

    #[test]
    fn cstring_path_with_slashes() {
        let buf = to_cstring_bytes("/tmp/myfifo");
        assert_eq!(buf, b"/tmp/myfifo\0");
    }
}
