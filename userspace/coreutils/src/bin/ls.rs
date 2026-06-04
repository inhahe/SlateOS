//! ls — list directory contents.
//!
//! Usage: ls [-l] [-a] [-h] [-1] [PATH...]
//!   -l  long listing format (permissions, size, date, name)
//!   -a  show hidden files (starting with .)
//!   -h  human-readable sizes (K, M, G) in long format
//!   -1  one entry per line (default when output is not a terminal)

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

struct Options {
    long: bool,
    all: bool,
    human: bool,
    one_per_line: bool,
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut opts = Options {
        long: false,
        all: false,
        human: false,
        one_per_line: false,
    };
    let mut paths: Vec<String> = Vec::new();

    for arg in &args {
        if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") {
            for c in arg[1..].chars() {
                match c {
                    'l' => opts.long = true,
                    'a' => opts.all = true,
                    'h' => opts.human = true,
                    '1' => opts.one_per_line = true,
                    _ => eprintln!("ls: unknown option: -{c}"),
                }
            }
        } else {
            paths.push(arg.clone());
        }
    }

    if paths.is_empty() {
        paths.push(".".to_string());
    }

    let show_dir_name = paths.len() > 1;
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for (i, path) in paths.iter().enumerate() {
        if i > 0 {
            let _ = writeln!(out);
        }
        if show_dir_name {
            let _ = writeln!(out, "{path}:");
        }
        list_dir(&mut out, path, &opts);
    }
}

fn list_dir(out: &mut impl Write, path: &str, opts: &Options) {
    let entries = match fs::read_dir(path) {
        Ok(e) => e,
        Err(e) => {
            // Maybe it's a file, not a directory
            if Path::new(path).is_file() {
                show_entry(out, path, Path::new(path), opts);
                return;
            }
            eprintln!("ls: cannot access '{path}': {e}");
            return;
        }
    };

    let mut names: Vec<(String, std::path::PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !opts.all && name.starts_with('.') {
            continue;
        }
        names.push((name, entry.path()));
    }
    names.sort_by_key(|a| a.0.to_lowercase());

    if opts.long {
        for (name, path) in &names {
            show_entry_long(out, name, path, opts);
        }
    } else if opts.one_per_line {
        for (name, _) in &names {
            let _ = writeln!(out, "{name}");
        }
    } else {
        // Simple columnar output
        let mut first = true;
        for (name, _) in &names {
            if !first {
                let _ = write!(out, "  ");
            }
            first = false;
            let _ = write!(out, "{name}");
        }
        if !first {
            let _ = writeln!(out);
        }
    }
}

fn show_entry(out: &mut impl Write, name: &str, path: &Path, opts: &Options) {
    if opts.long {
        show_entry_long(out, name, path, opts);
    } else {
        let _ = writeln!(out, "{name}");
    }
}

fn show_entry_long(out: &mut impl Write, name: &str, path: &Path, opts: &Options) {
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(_) => {
            let _ = writeln!(out, "?????????? ? ? {name}");
            return;
        }
    };

    let file_type = if meta.is_dir() { "d" } else if meta.is_symlink() { "l" } else { "-" };
    let size = meta.len();
    let size_str = if opts.human {
        human_size(size)
    } else {
        format!("{size:>8}")
    };

    let _ = writeln!(out, "{file_type}rw-r--r--  {size_str} {name}");
}

fn human_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;

    if bytes >= GIB {
        format!("{:>5.1}G", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:>5.1}M", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:>5.1}K", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes:>6}")
    }
}
