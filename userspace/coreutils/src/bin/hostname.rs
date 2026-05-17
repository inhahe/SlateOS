//! hostname — show or set the system host name.
//!
//! Usage: hostname [NAME]
//!   Without arguments: print the current hostname.
//!   With NAME: set the hostname (requires privilege).

use std::env;
use std::process;

unsafe extern "C" {
    fn gethostname(buf: *mut u8, len: usize) -> i32;
    fn sethostname(name: *const u8, len: usize) -> i32;
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        // Print current hostname.
        let mut buf = [0u8; 256];
        // SAFETY: buf is a valid 256-byte buffer on the stack.
        let ret = unsafe { gethostname(buf.as_mut_ptr(), buf.len()) };
        if ret != 0 {
            eprintln!("hostname: failed to get hostname");
            process::exit(1);
        }
        // Find the null terminator.
        let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        let name = String::from_utf8_lossy(&buf[..len]);
        println!("{name}");
    } else {
        // Set hostname.
        let name = &args[0];
        let bytes = name.as_bytes();
        // SAFETY: bytes points to a valid string, len is correct.
        let ret = unsafe { sethostname(bytes.as_ptr(), bytes.len()) };
        if ret != 0 {
            eprintln!("hostname: failed to set hostname");
            process::exit(1);
        }
    }
}
