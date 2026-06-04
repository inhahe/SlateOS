//! tty — print the file name of the terminal connected to stdin.
//!
//! Usage: tty [-s]
//!   -s  silent mode: print nothing, only return exit status
//!   Exit 0 if stdin is a terminal, 1 if not.

use std::env;

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn isatty(fd: i32) -> i32;
    fn ttyname(fd: i32) -> *const u8;
}

/// True if any element of `args` is the `-s` (silent) flag.
fn parse_silent(args: &[String]) -> bool {
    args.iter().any(|a| a == "-s")
}

/// Read a NUL-terminated UTF-8 string from a raw byte pointer.  Returns
/// `None` if the pointer is null.  Used to wrap the result of `ttyname(0)`
/// in a way that can be exercised by unit tests with a synthetic buffer.
///
/// # Safety
/// `ptr`, if non-null, must point to a NUL-terminated byte sequence that
/// remains valid for the lifetime of the call.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
unsafe fn read_cstr_lossy(ptr: *const u8) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let mut len: usize = 0;
    // SAFETY: caller guarantees `ptr` points to a NUL-terminated byte run.
    while unsafe { *ptr.add(len) } != 0 {
        len = len.saturating_add(1);
    }
    // SAFETY: same as above; the run has `len` bytes before the NUL.
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    Some(String::from_utf8_lossy(slice).into_owned())
}

/// Pure helper that picks the line to print given the platform results.
/// Returns `None` when the silent flag is set (no output).
fn output_line(silent: bool, is_tty: bool, name: Option<String>) -> Option<String> {
    if silent {
        return None;
    }
    if is_tty {
        Some(name.unwrap_or_else(|| "/dev/tty".to_string()))
    } else {
        Some("not a tty".to_string())
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let silent = parse_silent(&args);

    #[cfg(target_os = "linux")]
    let (is_tty, name) = {
        // SAFETY: isatty(0) is a pure query on file-descriptor 0.
        let is_tty = unsafe { isatty(0) } != 0;
        let name = if is_tty {
            // SAFETY: ttyname returns a pointer to a static string or null.
            unsafe { read_cstr_lossy(ttyname(0)) }
        } else {
            None
        };
        (is_tty, name)
    };
    #[cfg(not(target_os = "linux"))]
    let (is_tty, name): (bool, Option<String>) = (false, None);

    if let Some(line) = output_line(silent, is_tty, name) {
        println!("{line}");
    }

    std::process::exit(if is_tty { 0 } else { 1 });
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn silent_absent() {
        assert!(!parse_silent(&s(&[])));
        assert!(!parse_silent(&s(&["-a"])));
    }

    #[test]
    fn silent_present() {
        assert!(parse_silent(&s(&["-s"])));
        assert!(parse_silent(&s(&["-a", "-s"])));
    }

    #[test]
    fn output_silent_emits_nothing() {
        assert_eq!(output_line(true, true, Some("/dev/pts/0".into())), None);
        assert_eq!(output_line(true, false, None), None);
    }

    #[test]
    fn output_not_a_tty() {
        assert_eq!(output_line(false, false, None).as_deref(), Some("not a tty"));
    }

    #[test]
    fn output_tty_with_name() {
        assert_eq!(
            output_line(false, true, Some("/dev/pts/3".into())).as_deref(),
            Some("/dev/pts/3"),
        );
    }

    #[test]
    fn output_tty_without_name_uses_dev_tty() {
        assert_eq!(
            output_line(false, true, None).as_deref(),
            Some("/dev/tty"),
        );
    }

    #[test]
    fn read_cstr_null_pointer_returns_none() {
        // SAFETY: passing a null pointer is the documented null case.
        assert_eq!(unsafe { read_cstr_lossy(std::ptr::null()) }, None);
    }

    #[test]
    fn read_cstr_reads_until_nul() {
        let buf: [u8; 8] = *b"hello\0XX";
        // SAFETY: buf is a fixed-size local with a NUL at index 5.
        let s = unsafe { read_cstr_lossy(buf.as_ptr()) };
        assert_eq!(s.as_deref(), Some("hello"));
    }

    #[test]
    fn read_cstr_empty_string() {
        let buf: [u8; 4] = [0, b'X', b'Y', 0];
        // SAFETY: first byte is NUL, run length is zero.
        let s = unsafe { read_cstr_lossy(buf.as_ptr()) };
        assert_eq!(s.as_deref(), Some(""));
    }

    #[test]
    fn read_cstr_invalid_utf8_lossy() {
        let buf: [u8; 4] = [0xff, b'A', 0, 0];
        // SAFETY: NUL at index 2 bounds the read.
        let s = unsafe { read_cstr_lossy(buf.as_ptr()) }.unwrap();
        assert!(s.ends_with('A'));
        assert!(s.contains('\u{FFFD}'));
    }
}
