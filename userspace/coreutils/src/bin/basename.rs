//! basename -- strip directory and optional suffix from a pathname.
//!
//! Usage: basename PATH [SUFFIX]
//!   Print the final component of PATH, removing a trailing SUFFIX if given.

use std::env;
use std::path::Path;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!("basename: missing operand");
        process::exit(1);
    }

    let path = &args[0];
    let suffix = args.get(1).map(|s| s.as_str());

    // Extract the final path component.
    let base = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| {
            // Handle root paths like "/" which have no file_name.
            if path == "/" {
                "/".to_string()
            } else {
                path.clone()
            }
        });

    // Strip suffix if provided and the base name is longer than the suffix.
    let result = match suffix {
        Some(sfx) if !sfx.is_empty() && base.len() > sfx.len() && base.ends_with(sfx) => {
            base[..base.len() - sfx.len()].to_string()
        }
        _ => base,
    };

    println!("{result}");
}
