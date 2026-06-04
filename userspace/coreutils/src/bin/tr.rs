//! tr — translate or delete characters.
//!
//! Usage: tr [-d] SET1 [SET2]
//!   -d  delete characters in SET1 (no SET2 needed)
//!   Without -d: translate SET1 chars to corresponding SET2 chars.
//!   Reads from stdin, writes to stdout.

use std::env;
use std::io::{self, Read, Write};
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut delete = false;
    let mut sets: Vec<String> = Vec::new();

    for arg in &args {
        if arg == "-d" {
            delete = true;
        } else {
            sets.push(arg.clone());
        }
    }

    if sets.is_empty() {
        eprintln!("tr: missing operand");
        process::exit(1);
    }

    let set1 = expand_set(&sets[0]);

    if delete {
        // Delete mode: remove all chars in set1
        let mut input = Vec::new();
        io::stdin().read_to_end(&mut input).unwrap_or(0);
        let stdout = io::stdout();
        let mut out = stdout.lock();
        for &b in &input {
            if !set1.contains(&b) {
                let _ = out.write_all(&[b]);
            }
        }
    } else {
        // Translate mode
        if sets.len() < 2 {
            eprintln!("tr: missing SET2");
            process::exit(1);
        }
        let set2 = expand_set(&sets[1]);

        // Build translation table
        let mut table = [0u8; 256];
        for (i, slot) in table.iter_mut().enumerate() {
            *slot = i as u8;
        }
        for (i, &from) in set1.iter().enumerate() {
            let to = if i < set2.len() {
                set2[i]
            } else {
                // Pad with last char of set2
                *set2.last().unwrap_or(&from)
            };
            table[from as usize] = to;
        }

        let mut input = Vec::new();
        io::stdin().read_to_end(&mut input).unwrap_or(0);
        let stdout = io::stdout();
        let mut out = stdout.lock();
        for &b in &input {
            let _ = out.write_all(&[table[b as usize]]);
        }
    }
}

/// Expand a set string, handling ranges like a-z.
fn expand_set(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if i + 2 < bytes.len() && bytes[i + 1] == b'-' {
            let start = bytes[i];
            let end = bytes[i + 2];
            if start <= end {
                for b in start..=end {
                    result.push(b);
                }
            } else {
                for b in (end..=start).rev() {
                    result.push(b);
                }
            }
            i += 3;
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    result
}
