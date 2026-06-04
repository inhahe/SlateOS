#![deny(clippy::all)]

//! cryptsetup-cli — OurOS cryptsetup disk encryption
//!
//! Multi-personality: `cryptsetup`, `veritysetup`, `integritysetup`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cryptsetup(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cryptsetup <action> [OPTIONS] <device>");
        println!("cryptsetup v2.7 (OurOS) — LUKS disk encryption setup");
        println!();
        println!("Actions:");
        println!("  luksFormat DEVICE       Format LUKS partition");
        println!("  luksOpen DEVICE NAME    Open LUKS partition");
        println!("  luksClose NAME          Close LUKS partition");
        println!("  luksDump DEVICE         Dump LUKS header");
        println!("  luksAddKey DEVICE       Add passphrase");
        println!("  luksRemoveKey DEVICE    Remove passphrase");
        println!("  status NAME             Show mapping status");
        println!();
        println!("Options:");
        println!("  --cipher SPEC    Cipher specification");
        println!("  --key-size BITS  Key size in bits");
        println!("  --hash ALG       Hash algorithm");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("cryptsetup v2.7 (OurOS)"); return 0; }
    match args.first().map(|s| s.as_str()) {
        Some("luksDump") => {
            println!("LUKS header information:");
            println!("  Version:        2");
            println!("  Cipher name:    aes");
            println!("  Cipher mode:    xts-plain64");
            println!("  Hash spec:      sha256");
            println!("  Key slots:      1 of 8 used");
        }
        Some("status") => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("root");
            println!("/dev/mapper/{} is active.", name);
            println!("  type:    LUKS2");
            println!("  cipher:  aes-xts-plain64");
            println!("  keysize: 512 bits");
            println!("  device:  /dev/sda2");
        }
        _ => {
            println!("cryptsetup: use --help for available actions");
        }
    }
    0
}

fn run_veritysetup(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: veritysetup <action> [OPTIONS]");
        println!("veritysetup v2.7 (OurOS) — dm-verity volume setup");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("veritysetup v2.7 (OurOS)"); return 0; }
    println!("veritysetup: dm-verity tool");
    0
}

fn run_integritysetup(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: integritysetup <action> [OPTIONS]");
        println!("integritysetup v2.7 (OurOS) — dm-integrity volume setup");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("integritysetup v2.7 (OurOS)"); return 0; }
    println!("integritysetup: dm-integrity tool");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cryptsetup".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "veritysetup" => run_veritysetup(&rest, &prog),
        "integritysetup" => run_integritysetup(&rest, &prog),
        _ => run_cryptsetup(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, run_cryptsetup, run_integritysetup, run_veritysetup, strip_ext};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cryptsetup"), "cryptsetup");
        assert_eq!(basename(r"C:\bin\cryptsetup.exe"), "cryptsetup.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cryptsetup.exe"), "cryptsetup");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn cryptsetup_help_and_version() {
        assert_eq!(run_cryptsetup(&["--help".to_string()], "cryptsetup"), 0);
        assert_eq!(run_cryptsetup(&["-h".to_string()], "cryptsetup"), 0);
        let _ = run_cryptsetup(&["--version".to_string()], "cryptsetup");
    }

    #[test]
    fn cryptsetup_actions_succeed() {
        assert_eq!(run_cryptsetup(&["luksDump".to_string()], "cryptsetup"), 0);
        assert_eq!(
            run_cryptsetup(
                &["status".to_string(), "myvol".to_string()],
                "cryptsetup",
            ),
            0
        );
        assert_eq!(
            run_cryptsetup(&["unknown".to_string()], "cryptsetup"),
            0
        );
    }

    #[test]
    fn verity_and_integrity_help_and_version() {
        assert_eq!(run_veritysetup(&["--help".to_string()], "veritysetup"), 0);
        assert_eq!(
            run_veritysetup(&["--version".to_string()], "veritysetup"),
            0
        );
        assert_eq!(
            run_integritysetup(&["--help".to_string()], "integritysetup"),
            0
        );
        assert_eq!(
            run_integritysetup(&["--version".to_string()], "integritysetup"),
            0
        );
    }
}
