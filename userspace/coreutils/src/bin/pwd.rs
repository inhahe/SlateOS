//! pwd -- print the current working directory.
//!
//! Usage: pwd

use std::env;

fn main() {
    match env::current_dir() {
        Ok(path) => println!("{}", path.display()),
        Err(e) => {
            eprintln!("pwd: {e}");
            std::process::exit(1);
        }
    }
}
