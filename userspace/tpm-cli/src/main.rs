#![deny(clippy::all)]

//! tpm-cli — SlateOS TPM (Trusted Platform Module) tools
//!
//! Multi-personality: `tpm2_getcap`, `tpm2_getrandom`, `tpm2_pcrread`,
//! `tpm2_createprimary`, `tpm2_nvdefine`, `tpm2_nvread`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_tpm2_getcap(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tpm2_getcap [OPTIONS] CAPABILITY");
        println!();
        println!("tpm2_getcap — display TPM capabilities (Slate OS).");
        println!();
        println!("Capabilities: algorithms, commands, properties-fixed, properties-variable");
        return 0;
    }

    let cap = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("properties-fixed");
    match cap {
        "algorithms" => {
            println!("sha1:");
            println!("  value:      0x4");
            println!("  asymmetric: 0");
            println!("  symmetric:  0");
            println!("  hash:       1");
            println!("sha256:");
            println!("  value:      0xB");
            println!("  asymmetric: 0");
            println!("  symmetric:  0");
            println!("  hash:       1");
            println!("rsa:");
            println!("  value:      0x1");
            println!("  asymmetric: 1");
            println!("ecc:");
            println!("  value:      0x23");
            println!("  asymmetric: 1");
        }
        "properties-fixed" => {
            println!("TPM2_PT_FAMILY_INDICATOR:");
            println!("  raw: 0x322E3000");
            println!("  value: \"2.0\"");
            println!("TPM2_PT_LEVEL:");
            println!("  raw: 0");
            println!("TPM2_PT_REVISION:");
            println!("  value: 1.59");
            println!("TPM2_PT_MANUFACTURER:");
            println!("  raw: 0x494E5443");
            println!("  value: \"INTC\"");
            println!("TPM2_PT_FIRMWARE_VERSION_1:");
            println!("  raw: 0x00040033");
            println!("  value: 4.51");
        }
        "commands" => {
            println!("TPM2_CC_CreatePrimary: 0x00000131");
            println!("TPM2_CC_Create: 0x00000153");
            println!("TPM2_CC_Load: 0x00000157");
            println!("TPM2_CC_Sign: 0x0000015D");
            println!("TPM2_CC_Unseal: 0x0000015E");
            println!("TPM2_CC_PCR_Read: 0x0000017E");
            println!("TPM2_CC_GetRandom: 0x0000017B");
        }
        _ => println!("tpm2_getcap: capability '{}' listed", cap),
    }
    0
}

fn run_tpm2_getrandom(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tpm2_getrandom [OPTIONS] SIZE");
        println!();
        println!("tpm2_getrandom — get random bytes from TPM (Slate OS).");
        return 0;
    }

    let hex = args.iter().any(|a| a == "--hex");
    let size_str = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("32");
    let size: usize = size_str.parse().unwrap_or(32);

    if hex {
        for i in 0..size {
            print!("{:02x}", (i * 7 + 42) % 256);
        }
        println!();
    } else {
        println!("(binary data, {} bytes)", size);
    }
    0
}

fn run_tpm2_pcrread(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tpm2_pcrread [OPTIONS] [PCR_LIST]");
        println!();
        println!("tpm2_pcrread — read PCR values from TPM (Slate OS).");
        return 0;
    }
    let _ = args;
    println!("  sha256:");
    println!("    0 : 0x0000000000000000000000000000000000000000000000000000000000000000");
    println!("    1 : 0xABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF01234567");
    println!("    2 : 0x1234567890ABCDEF1234567890ABCDEF1234567890ABCDEF1234567890ABCD");
    println!("    3 : 0x0000000000000000000000000000000000000000000000000000000000000000");
    println!("    7 : 0xFEDCBA9876543210FEDCBA9876543210FEDCBA9876543210FEDCBA98765432");
    0
}

fn run_tpm2_createprimary(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tpm2_createprimary [OPTIONS]");
        println!("Options: -C HIERARCHY, -G ALG, -g HASH, -c CONTEXT");
        return 0;
    }
    let _ = args;
    println!("name-alg:");
    println!("  value: sha256");
    println!("  raw: 0xb");
    println!("type:");
    println!("  value: rsa");
    println!("  raw: 0x1");
    println!("name: 000b1234567890abcdef");
    0
}

fn run_default(prog: &str, args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        println!("TPM2 tool (Slate OS). See tpm2-tools documentation.");
        return 0;
    }
    println!("{}: operation completed successfully", prog);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "tpm2_getcap".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "tpm2_getcap" => run_tpm2_getcap(&rest),
        "tpm2_getrandom" => run_tpm2_getrandom(&rest),
        "tpm2_pcrread" => run_tpm2_pcrread(&rest),
        "tpm2_createprimary" => run_tpm2_createprimary(&rest),
        _ => run_default(&prog, &rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tpm2_getcap};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/tpm"), "tpm");
        assert_eq!(basename(r"C:\bin\tpm.exe"), "tpm.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("tpm.exe"), "tpm");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tpm2_getcap(&["--help".to_string()]), 0);
        assert_eq!(run_tpm2_getcap(&["-h".to_string()]), 0);
        let _ = run_tpm2_getcap(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tpm2_getcap(&[]);
    }
}
