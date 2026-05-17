//! logname — print the login name.
//!
//! Usage: logname
//!   Prints the user's login name from $LOGNAME or $USER.
//!   Exits with 1 if the login name cannot be determined.

use std::env;
use std::process;

fn main() {
    if let Ok(name) = env::var("LOGNAME") {
        println!("{name}");
    } else if let Ok(name) = env::var("USER") {
        println!("{name}");
    } else {
        eprintln!("logname: no login name");
        process::exit(1);
    }
}
