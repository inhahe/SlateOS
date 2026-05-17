//! ln -- create links between files.
//!
//! Usage: ln [-s] TARGET LINK_NAME
//!   -s  create a symbolic link instead of a hard link

use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut symbolic = false;
    let mut paths: Vec<String> = Vec::new();

    for arg in &args {
        if arg.starts_with('-') && arg.len() > 1 {
            for c in arg[1..].chars() {
                match c {
                    's' => symbolic = true,
                    _ => {
                        eprintln!("ln: unknown option: -{c}");
                        process::exit(1);
                    }
                }
            }
        } else {
            paths.push(arg.clone());
        }
    }

    if paths.len() != 2 {
        eprintln!("ln: expected exactly two arguments: TARGET LINK_NAME");
        process::exit(1);
    }

    let target = &paths[0];
    let link_name = &paths[1];

    let result = if symbolic {
        symlink(target, link_name)
    } else {
        std::fs::hard_link(target, link_name)
    };

    if let Err(e) = result {
        let kind = if symbolic { "symbolic" } else { "hard" };
        eprintln!("ln: cannot create {kind} link '{link_name}' -> '{target}': {e}");
        process::exit(1);
    }
}

/// Create a symbolic link. Delegates to the platform-specific API.
#[cfg(unix)]
fn symlink(target: &str, link_name: &str) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link_name)
}

#[cfg(windows)]
fn symlink(target: &str, link_name: &str) -> std::io::Result<()> {
    let target_path = std::path::Path::new(target);
    if target_path.is_dir() {
        std::os::windows::fs::symlink_dir(target, link_name)
    } else {
        std::os::windows::fs::symlink_file(target, link_name)
    }
}

#[cfg(not(any(unix, windows)))]
fn symlink(_target: &str, _link_name: &str) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "symbolic links not supported on this platform",
    ))
}
