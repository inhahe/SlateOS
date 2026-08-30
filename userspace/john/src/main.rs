#![deny(clippy::all)]

//! john — Slate OS John the Ripper password cracker
//!
//! Multi-personality: `john`, `unshadow`, `zip2john`, `rar2john`, `pdf2john`

use std::env;
use std::process;

fn run_john(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: john [OPTIONS] [PASSWORD-FILES]");
        println!();
        println!("Options:");
        println!("  --single               Single crack mode");
        println!("  --wordlist=FILE        Wordlist mode, read from FILE");
        println!("  --incremental[=MODE]   Incremental mode");
        println!("  --external=MODE        External mode");
        println!("  --rules=RULES          Enable word mangling rules");
        println!("  --format=NAME          Force hash type");
        println!("  --show                 Show cracked passwords");
        println!("  --test[=TIME]          Benchmark");
        println!("  --users=[-]LOGIN|UID   Filter users");
        println!("  --fork=N               Fork N processes");
        println!("  --session=NAME         Session name");
        println!("  --restore[=NAME]       Restore session");
        println!("  --status[=NAME]        Show session status");
        println!("  --list=WHAT            List (formats/subformats/rules)");
        return 0;
    }
    if args.iter().any(|a| a == "--test") {
        println!("John the Ripper 1.9.0-jumbo-1 (Slate OS) [64-bit AVX2]");
        println!("Benchmarking: descrypt, traditional crypt(3) [DES 256/256 AVX2]... DONE");
        println!("Many salts:  5432K c/s real, 5432K c/s virtual");
        println!("Only one salt: 4567K c/s real, 4567K c/s virtual");
        println!();
        println!("Benchmarking: bcrypt (\"$2a$\") [Blowfish 32/64 AVX2]... DONE");
        println!("Cost 1 (iteration count) is 32 for all loaded hashes");
        println!("Speed for cost 1: 1234 c/s real, 1234 c/s virtual");
        return 0;
    }
    if args.iter().any(|a| a == "--show") {
        println!("user1:password123");
        println!("user2:admin");
        println!();
        println!("2 password hashes cracked, 3 left");
        return 0;
    }
    if args.iter().any(|a| a.starts_with("--list=")) {
        let what = args.iter().find_map(|a| a.strip_prefix("--list=")).unwrap_or("formats");
        match what {
            "formats" => {
                println!("descrypt, bsdicrypt, md5crypt, bcrypt, scrypt, LM, AFS,");
                println!("tripcode, AndroidBackup, adxcrypt, agilekeychain, aix-ssha1,");
                println!("Raw-SHA256, Raw-SHA512, Raw-MD5, NTLM, mssql, oracle");
            }
            _ => println!("(list: {} — simulated)", what),
        }
        return 0;
    }
    if args.iter().any(|a| a.starts_with("--status")) {
        println!("Session: default");
        println!("Status: running");
        println!("Guesses: 2/5 (40.00%)");
        println!("Time: 0:00:05, ETA: 0:00:07");
        println!("Speed: 123456 g/s");
        return 0;
    }

    println!("John the Ripper 1.9.0-jumbo-1 (Slate OS) [64-bit AVX2]");
    println!("Using default input encoding: UTF-8");
    println!("Loaded 5 password hashes with 5 different salts (bcrypt)");
    println!("Press 'q' or Ctrl-C to abort, almost any other key for status");
    println!("password123      (user1)");
    println!("admin            (user2)");
    println!("2g 0:00:05 0.40g/s 123.4p/s 617.2c/s 617.2C/s");
    0
}

fn run_unshadow(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: unshadow PASSWORD-FILE SHADOW-FILE");
        return 0;
    }
    println!("root:$6$rounds=5000$salt$hash:0:0:root:/root:/bin/bash");
    println!("user:$6$rounds=5000$salt$hash:1000:1000:User:/home/user:/bin/bash");
    let _ = args;
    0
}

fn run_x2john(args: Vec<String>, fmt: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {}2john <file>", fmt);
        return 0;
    }
    let file = args.first().map(|s| s.as_str()).unwrap_or("file");
    println!("{}:${}$*simulated*hash*data*", file, fmt);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("john");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "unshadow" => run_unshadow(rest),
        "zip2john" => run_x2john(rest, "zip"),
        "rar2john" => run_x2john(rest, "rar"),
        "pdf2john" => run_x2john(rest, "pdf"),
        _ => run_john(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_x2john};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_x2john(vec!["--help".to_string()], "john"), 0);
        assert_eq!(run_x2john(vec!["-h".to_string()], "john"), 0);
        let _ = run_x2john(vec!["--version".to_string()], "john");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_x2john(vec![], "john");
    }
}
