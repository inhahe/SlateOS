//! tty — print the file name of the terminal connected to stdin.
//!
//! Usage: tty [-s]
//!   -s  silent mode: print nothing, only return exit status
//!   Exit 0 if stdin is a terminal, 1 if not.

use std::env;

unsafe extern "C" {
    fn isatty(fd: i32) -> i32;
    fn ttyname(fd: i32) -> *const u8;
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let silent = args.iter().any(|a| a == "-s");

    // SAFETY: isatty(0) checks if stdin (fd 0) is a terminal.
    // ttyname(0) returns a static string or null.
    let is_tty = unsafe { isatty(0) } != 0;

    if !silent {
        if is_tty {
            let name_ptr = unsafe { ttyname(0) };
            if !name_ptr.is_null() {
                // Find null terminator
                let mut len = 0;
                while unsafe { *name_ptr.add(len) } != 0 {
                    len += 1;
                }
                let name =
                    String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(name_ptr, len) });
                println!("{name}");
            } else {
                println!("/dev/tty");
            }
        } else {
            println!("not a tty");
        }
    }

    std::process::exit(if is_tty { 0 } else { 1 });
}
