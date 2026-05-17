//! dirname — strip last component from file name.
//!
//! Usage: dirname PATH

use std::env;
use std::path::Path;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("dirname: missing operand");
        process::exit(1);
    }
    for arg in &args {
        let p = Path::new(arg);
        match p.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => {
                println!("{}", parent.display());
            }
            _ => println!("."),
        }
    }
}
