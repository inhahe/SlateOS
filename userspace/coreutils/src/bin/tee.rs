//! tee — read from stdin, write to stdout and files.
//!
//! Usage: tee [-a] [FILE...]
//!   -a  append to files instead of overwriting

use std::env;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut append = false;
    let mut paths: Vec<String> = Vec::new();

    for arg in &args {
        if arg == "-a" {
            append = true;
        } else if arg.starts_with('-') && arg.len() > 1 {
            eprintln!("tee: unknown option: {arg}");
            process::exit(1);
        } else {
            paths.push(arg.clone());
        }
    }

    let mut files: Vec<File> = Vec::new();
    for path in &paths {
        let file = if append {
            OpenOptions::new().create(true).append(true).open(path)
        } else {
            File::create(path)
        };
        match file {
            Ok(f) => files.push(f),
            Err(e) => {
                eprintln!("tee: {path}: {e}");
            }
        }
    }

    let mut buf = [0u8; 8192];
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    loop {
        match stdin.lock().read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let _ = out.write_all(&buf[..n]);
                for f in &mut files {
                    let _ = f.write_all(&buf[..n]);
                }
            }
            Err(e) => {
                eprintln!("tee: read error: {e}");
                process::exit(1);
            }
        }
    }
}
