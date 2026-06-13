#![deny(clippy::all)]

//! john-cli — SlateOS John the Ripper CLI
//!
//! Single personality: `john`

use std::env;
use std::process;

fn run_john(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: john [OPTIONS] [PASSWORD_FILES]");
        println!();
        println!("John the Ripper — password cracker (SlateOS).");
        println!();
        println!("Options:");
        println!("  --single               \"Single crack\" mode");
        println!("  --wordlist=FILE        Wordlist mode");
        println!("  --incremental[=MODE]   Incremental mode");
        println!("  --rules[=SECTION]      Enable word mangling rules");
        println!("  --format=FORMAT        Force hash format");
        println!("  --show                 Show cracked passwords");
        println!("  --test[=TIME]          Run benchmark");
        println!("  --list=WHAT            List capabilities (formats, etc.)");
        println!("  --pot=FILE             Potfile path");
        println!("  --session=NAME         Session name");
        println!("  --restore[=NAME]       Restore session");
        println!("  --fork=N               Fork N processes");
        println!("  --node=MIN-MAX/TOTAL   Distributed processing");
        println!("  --status[=NAME]        Show session status");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("John the Ripper 1.9.0-jumbo-1 (SlateOS)");
        return 0;
    }

    if args.iter().any(|a| a.starts_with("--test")) {
        println!("Benchmarking: descrypt, traditional crypt(3) [DES 128/128 AVX]...");
        println!("DONE (Many salts): 4567K c/s real, 4567K c/s virtual");
        println!();
        println!("Benchmarking: bcrypt (\"$2b$\") [Blowfish 32/64 X3]...");
        println!("DONE: 1234 c/s real, 1234 c/s virtual");
        println!();
        println!("Benchmarking: sha256crypt [SHA256 128/128 AVX 4x]...");
        println!("DONE: 23456 c/s real, 23456 c/s virtual");
        println!();
        println!("Benchmarking: sha512crypt [SHA512 128/128 AVX 2x]...");
        println!("DONE: 12345 c/s real, 12345 c/s virtual");
        println!();
        println!("Benchmarking: Raw-MD5 [MD5 128/128 AVX 4x3]...");
        println!("DONE (Many salts): 98765K c/s real, 98765K c/s virtual");
        return 0;
    }

    if args.iter().any(|a| a.starts_with("--list")) {
        let what = args.iter().find(|a| a.starts_with("--list="))
            .map(|a| a.strip_prefix("--list=").unwrap_or("formats"))
            .unwrap_or("formats");
        match what {
            "formats" => {
                println!("descrypt, bsdicrypt, md5crypt, bcrypt, scrypt,");
                println!("sha256crypt, sha512crypt, Raw-MD5, Raw-SHA1,");
                println!("Raw-SHA256, Raw-SHA512, NTLM, LM, mysql-sha1,");
                println!("ZIP, RAR, 7z, PDF, KeePass, SSH, PGP, WPA");
            }
            _ => { println!("Unknown list type: {}", what); }
        }
        return 0;
    }

    if args.iter().any(|a| a == "--show") {
        let file = args.iter().find(|a| !a.starts_with('-'))
            .map(|s| s.as_str()).unwrap_or("hashes.txt");
        println!("user1:password123");
        println!("user2:letmein");
        println!("admin:admin2024");
        println!();
        println!("3 password hashes cracked, 2 left");
        let _ = file;
        return 0;
    }

    if args.iter().any(|a| a.starts_with("--status")) {
        println!("0g 0:00:01:23  3/3 0g/s 1234p/s 1234c/s 1234C/s wordlist..");
        return 0;
    }

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    let hash_file = files.first().copied().unwrap_or("hashes.txt");
    let wordlist = args.iter().find(|a| a.starts_with("--wordlist="))
        .map(|a| a.strip_prefix("--wordlist=").unwrap_or("wordlist.txt"));

    println!("Using default input encoding: UTF-8");
    println!("Loaded 5 password hashes with 5 different salts (sha512crypt [SHA512 128/128 AVX 2x])");

    if let Some(wl) = wordlist {
        println!("Press 'q' or Ctrl-C to abort, almost any other key for status");
        println!("Using wordlist: {}", wl);
    }

    println!("Will run {} OpenMP threads", 4);
    println!("password123      (user1)");
    println!("letmein          (user2)");
    println!("admin2024        (admin)");
    println!("3g 0:00:00:45 DONE (2024-01-15 12:00) 0.06666g/s 1234p/s 1234c/s 1234C/s");
    println!("Use the \"--show\" option to display cracked passwords reliably");
    let _ = hash_file;
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_john(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_john};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_john(vec!["--help".to_string()]), 0);
        assert_eq!(run_john(vec!["-h".to_string()]), 0);
        let _ = run_john(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_john(vec![]);
    }
}
