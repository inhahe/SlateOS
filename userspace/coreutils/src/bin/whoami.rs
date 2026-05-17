//! whoami — print effective user name.
//!
//! Usage: whoami
//!   Prints the name of the current user.
//!   Falls back to the numeric UID if no name database is available.

use std::env;

unsafe extern "C" {
    fn geteuid() -> u32;
}

fn main() {
    // Try USER or LOGNAME environment variable first.
    if let Ok(name) = env::var("USER") {
        println!("{name}");
        return;
    }
    if let Ok(name) = env::var("LOGNAME") {
        println!("{name}");
        return;
    }

    // Fall back to numeric UID.
    // SAFETY: geteuid is a simple POSIX getter, no pointer arguments.
    let uid = unsafe { geteuid() };
    println!("{uid}");
}
