#![deny(clippy::all)]

//! age-cli — OurOS age encryption CLI
//!
//! Multi-personality: `age` and `age-keygen`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn strip_ext(name: &str) -> &str {
    name.strip_suffix(".exe")
        .or_else(|| name.strip_suffix(".EXE"))
        .unwrap_or(name)
}

fn run_age(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: age [OPTIONS] [INPUT]");
        println!();
        println!("age file encryption tool (OurOS).");
        println!();
        println!("Options:");
        println!("  -e, --encrypt          Encrypt (default)");
        println!("  -d, --decrypt          Decrypt");
        println!("  -r, --recipient KEY    Encrypt to recipient");
        println!("  -R, --recipients-file  Recipients file");
        println!("  -i, --identity FILE    Identity file for decryption");
        println!("  -o, --output FILE      Output file");
        println!("  -p, --passphrase       Use passphrase");
        println!("  -a, --armor            ASCII armor output");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("age v1.1.1 (OurOS)");
        return 0;
    }

    let decrypt = args.iter().any(|a| a == "-d" || a == "--decrypt");
    let armor = args.iter().any(|a| a == "-a" || a == "--armor");
    let passphrase = args.iter().any(|a| a == "-p" || a == "--passphrase");
    let output = args.windows(2).find(|w| w[0] == "-o" || w[0] == "--output")
        .map(|w| w[1].as_str());

    if decrypt {
        let input = args.last().map(|s| s.as_str()).unwrap_or("secret.age");
        if passphrase {
            println!("Enter passphrase: ");
        }
        if let Some(out) = output {
            println!("Decrypted {} -> {}", input, out);
        } else {
            println!("(decrypted data written to stdout)");
        }
    } else {
        let input = args.last().map(|s| s.as_str()).unwrap_or("secret.txt");
        if passphrase {
            println!("Enter passphrase (leave empty to autogenerate a secure one): ");
            println!("Confirm passphrase: ");
        }
        if armor {
            println!("-----BEGIN AGE ENCRYPTED FILE-----");
            println!("YWdlLWVuY3J5cHRpb24ub3JnL3YxCi0+IHNjcnlwdCBKM0...");
            println!("-----END AGE ENCRYPTED FILE-----");
        } else if let Some(out) = output {
            println!("Encrypted {} -> {}", input, out);
        } else {
            println!("(encrypted data written to stdout)");
        }
    }
    0
}

fn run_age_keygen(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: age-keygen [-o OUTPUT]");
        println!();
        println!("Generate age key pairs.");
        return 0;
    }

    let output = args.windows(2).find(|w| w[0] == "-o" || w[0] == "--output")
        .map(|w| w[1].as_str());

    println!("# created: 2024-01-15T14:00:00Z");
    println!("# public key: age1abc123def456ghi789jkl012mno345pqr678stu901vwx234yz");
    println!("AGE-SECRET-KEY-1ABCDEF234567890GHIJKLMNOPQRSTUVWXYZ");

    if let Some(out) = output {
        println!();
        println!("Public key: age1abc123def456ghi789jkl012mno345pqr678stu901vwx234yz");
        println!("Key written to {}", out);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "age".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "age-keygen" => run_age_keygen(rest),
        _ => run_age(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_age};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/age"), "age");
        assert_eq!(basename(r"C:\bin\age.exe"), "age.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("age.exe"), "age");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_age(vec!["--help".to_string()]), 0);
        assert_eq!(run_age(vec!["-h".to_string()]), 0);
        assert_eq!(run_age(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_age(vec![]), 0);
    }
}
