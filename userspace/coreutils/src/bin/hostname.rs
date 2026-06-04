//! hostname — show or set the system host name.
//!
//! Usage: hostname [NAME]
//!   Without arguments: print the current hostname.
//!   With NAME: set the hostname (requires privilege).

use std::env;
use std::process;

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn gethostname(buf: *mut u8, len: usize) -> i32;
    fn sethostname(name: *const u8, len: usize) -> i32;
}

/// Decode a fixed-size hostname buffer into a `String`.  The kernel writes
/// a NUL-terminated name when the buffer is large enough; if the name fills
/// the buffer with no terminator, the full buffer is decoded.  Invalid
/// UTF-8 is replaced with U+FFFD.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn decode_hostname(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let slice = buf.get(..end).unwrap_or(&[]);
    String::from_utf8_lossy(slice).into_owned()
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        // Print current hostname.
        #[cfg(target_os = "linux")]
        {
            let mut buf = [0u8; 256];
            // SAFETY: buf is a valid 256-byte buffer on the stack.
            let ret = unsafe { gethostname(buf.as_mut_ptr(), buf.len()) };
            if ret != 0 {
                eprintln!("hostname: failed to get hostname");
                process::exit(1);
            }
            println!("{}", decode_hostname(&buf));
        }
        #[cfg(not(target_os = "linux"))]
        {
            // On non-linux hosts, fall back to the HOSTNAME / COMPUTERNAME
            // environment variables so the binary at least compiles and runs.
            let name = env::var("HOSTNAME")
                .or_else(|_| env::var("COMPUTERNAME"))
                .unwrap_or_else(|_| "unknown".to_string());
            println!("{name}");
        }
    } else {
        let name = args.first().map(String::as_str).unwrap_or_default();
        #[cfg(target_os = "linux")]
        {
            let bytes = name.as_bytes();
            // SAFETY: bytes points to a valid string, len is correct.
            let ret = unsafe { sethostname(bytes.as_ptr(), bytes.len()) };
            if ret != 0 {
                eprintln!("hostname: failed to set hostname");
                process::exit(1);
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = name;
            eprintln!("hostname: setting hostname not supported on this platform");
            process::exit(1);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn decode_empty_buffer() {
        assert_eq!(decode_hostname(&[]), "");
    }

    #[test]
    fn decode_zero_filled_buffer() {
        let buf = [0u8; 16];
        assert_eq!(decode_hostname(&buf), "");
    }

    #[test]
    fn decode_full_name_with_nul() {
        let mut buf = [0u8; 16];
        buf[..7].copy_from_slice(b"hosting");
        assert_eq!(decode_hostname(&buf), "hosting");
    }

    #[test]
    fn decode_name_filling_entire_buffer() {
        // No terminator: all 8 bytes are part of the name.
        let buf = *b"abcdefgh";
        assert_eq!(decode_hostname(&buf), "abcdefgh");
    }

    #[test]
    fn decode_stops_at_first_nul() {
        let buf = *b"box1\0junkjunk";
        assert_eq!(decode_hostname(&buf), "box1");
    }

    #[test]
    fn decode_invalid_utf8_lossy() {
        let buf = [b'a', 0xff, b'b', 0u8, 0u8];
        let out = decode_hostname(&buf);
        assert!(out.starts_with('a'));
        assert!(out.ends_with('b'));
        assert!(out.contains('\u{FFFD}'));
    }

    #[test]
    fn decode_typical_fqdn() {
        let mut buf = [0u8; 64];
        let name = b"host.example.org";
        buf[..name.len()].copy_from_slice(name);
        assert_eq!(decode_hostname(&buf), "host.example.org");
    }
}
