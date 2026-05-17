//! seq — print a sequence of numbers.
//!
//! Usage: seq [FIRST [INCREMENT]] LAST
//!   Prints numbers from FIRST to LAST by INCREMENT.
//!   Defaults: FIRST=1, INCREMENT=1.

use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    let (first, increment, last) = match args.len() {
        1 => {
            let last: f64 = args[0].parse().unwrap_or_else(|_| {
                eprintln!("seq: invalid number: '{}'", args[0]);
                process::exit(1);
            });
            (1.0, 1.0, last)
        }
        2 => {
            let first: f64 = args[0].parse().unwrap_or_else(|_| {
                eprintln!("seq: invalid number: '{}'", args[0]);
                process::exit(1);
            });
            let last: f64 = args[1].parse().unwrap_or_else(|_| {
                eprintln!("seq: invalid number: '{}'", args[1]);
                process::exit(1);
            });
            (first, 1.0, last)
        }
        3 => {
            let first: f64 = args[0].parse().unwrap_or_else(|_| {
                eprintln!("seq: invalid number: '{}'", args[0]);
                process::exit(1);
            });
            let increment: f64 = args[1].parse().unwrap_or_else(|_| {
                eprintln!("seq: invalid number: '{}'", args[1]);
                process::exit(1);
            });
            let last: f64 = args[2].parse().unwrap_or_else(|_| {
                eprintln!("seq: invalid number: '{}'", args[2]);
                process::exit(1);
            });
            if increment == 0.0 {
                eprintln!("seq: zero increment");
                process::exit(1);
            }
            (first, increment, last)
        }
        0 => {
            eprintln!("seq: missing operand");
            process::exit(1);
        }
        _ => {
            eprintln!("seq: too many arguments");
            process::exit(1);
        }
    };

    let mut val = first;
    if increment > 0.0 {
        while val <= last + f64::EPSILON {
            // Print integers without decimal point
            if val == val.trunc() && val.abs() < 1e15 {
                println!("{}", val as i64);
            } else {
                println!("{val}");
            }
            val += increment;
        }
    } else {
        while val >= last - f64::EPSILON {
            if val == val.trunc() && val.abs() < 1e15 {
                println!("{}", val as i64);
            } else {
                println!("{val}");
            }
            val += increment;
        }
    }
}
