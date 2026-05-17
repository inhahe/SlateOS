//! mkfifo — make FIFOs (named pipes).
//!
//! Usage: mkfifo [-m MODE] NAME...
//!   -m MODE   set permission mode (default: 0666, modified by umask)

use std::env;
use std::process;

unsafe extern "C" {
    fn mkfifo(path: *const u8, mode: u32) -> i32;
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut mode: u32 = 0o666;
    let mut names: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        if args[i] == "-m" && i + 1 < args.len() {
            mode = u32::from_str_radix(&args[i + 1], 8).unwrap_or(0o666);
            i += 2;
        } else {
            names.push(&args[i]);
            i += 1;
        }
    }

    if names.is_empty() {
        eprintln!("mkfifo: missing operand");
        process::exit(1);
    }

    let mut exit_code = 0;
    for name in &names {
        let mut c_path: Vec<u8> = name.as_bytes().to_vec();
        c_path.push(0);

        // SAFETY: c_path is a valid null-terminated string, mode is a u32.
        let ret = unsafe { mkfifo(c_path.as_ptr(), mode) };
        if ret != 0 {
            eprintln!("mkfifo: cannot create fifo '{name}'");
            exit_code = 1;
        }
    }

    process::exit(exit_code);
}
