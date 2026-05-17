//! cmp — compare two files byte by byte.
//!
//! Usage: cmp [-l] [-s] FILE1 FILE2
//!   -l  print byte number and differing bytes for each difference
//!   -s  silent: only return exit status
//!   Exit 0 if identical, 1 if different, 2 on error.

use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut verbose = false;
    let mut silent = false;
    let mut files: Vec<&str> = Vec::new();

    for arg in &args {
        match arg.as_str() {
            "-l" => verbose = true,
            "-s" => silent = true,
            _ => files.push(arg),
        }
    }

    if files.len() != 2 {
        eprintln!("cmp: requires exactly two files");
        process::exit(2);
    }

    let mut f1 = match File::open(files[0]) {
        Ok(f) => f,
        Err(e) => {
            if !silent {
                eprintln!("cmp: {}: {e}", files[0]);
            }
            process::exit(2);
        }
    };

    let mut f2 = match File::open(files[1]) {
        Ok(f) => f,
        Err(e) => {
            if !silent {
                eprintln!("cmp: {}: {e}", files[1]);
            }
            process::exit(2);
        }
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let mut buf1 = [0u8; 4096];
    let mut buf2 = [0u8; 4096];
    let mut byte_num: u64 = 1;
    let mut line_num: u64 = 1;
    let mut found_diff = false;

    loop {
        let n1 = match f1.read(&mut buf1) {
            Ok(n) => n,
            Err(e) => {
                if !silent {
                    eprintln!("cmp: {}: {e}", files[0]);
                }
                process::exit(2);
            }
        };
        let n2 = match f2.read(&mut buf2) {
            Ok(n) => n,
            Err(e) => {
                if !silent {
                    eprintln!("cmp: {}: {e}", files[1]);
                }
                process::exit(2);
            }
        };

        let min_n = n1.min(n2);

        for i in 0..min_n {
            if buf1[i] != buf2[i] {
                found_diff = true;
                if verbose {
                    let _ = writeln!(
                        out,
                        "{:>6} {:3o} {:3o}",
                        byte_num + i as u64,
                        buf1[i],
                        buf2[i]
                    );
                } else if !silent {
                    println!(
                        "{} {} differ: byte {}, line {}",
                        files[0],
                        files[1],
                        byte_num + i as u64,
                        line_num
                    );
                    process::exit(1);
                } else {
                    process::exit(1);
                }
            }
            if buf1[i] == b'\n' {
                line_num += 1;
            }
        }

        byte_num += min_n as u64;

        if n1 != n2 {
            if !silent {
                let shorter = if n1 < n2 { files[0] } else { files[1] };
                eprintln!("cmp: EOF on {shorter}");
            }
            process::exit(1);
        }

        if n1 == 0 {
            break;
        }
    }

    process::exit(if found_diff { 1 } else { 0 });
}
