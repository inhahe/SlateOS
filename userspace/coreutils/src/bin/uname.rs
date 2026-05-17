//! uname -- print system information.
//!
//! Usage: uname [-a] [-s] [-r] [-m]
//!   -s  print the operating system name (default if no flags)
//!   -r  print the OS release version
//!   -m  print the machine hardware name
//!   -a  print all information

use std::env;
use std::process;

const SYSNAME: &str = "OurOS";
const RELEASE: &str = "0.1.0";
const MACHINE: &str = "x86_64";

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut show_sys = false;
    let mut show_rel = false;
    let mut show_mach = false;

    for arg in &args {
        if arg.starts_with('-') && arg.len() > 1 {
            for c in arg[1..].chars() {
                match c {
                    'a' => {
                        show_sys = true;
                        show_rel = true;
                        show_mach = true;
                    }
                    's' => show_sys = true,
                    'r' => show_rel = true,
                    'm' => show_mach = true,
                    _ => {
                        eprintln!("uname: unknown option: -{c}");
                        process::exit(1);
                    }
                }
            }
        } else {
            eprintln!("uname: unexpected argument: {arg}");
            process::exit(1);
        }
    }

    // Default: print system name only.
    if !show_sys && !show_rel && !show_mach {
        show_sys = true;
    }

    let mut parts: Vec<&str> = Vec::new();
    if show_sys {
        parts.push(SYSNAME);
    }
    if show_rel {
        parts.push(RELEASE);
    }
    if show_mach {
        parts.push(MACHINE);
    }

    println!("{}", parts.join(" "));
}
