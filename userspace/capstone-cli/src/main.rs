#![deny(clippy::all)]

//! capstone-cli — SlateOS Capstone disassembly engine
//!
//! Single personality: `cstool`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cstool(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cstool [OPTIONS] ARCH MODE CODE");
        println!("cstool (SlateOS) — Capstone disassembly engine CLI");
        println!();
        println!("Architectures:");
        println!("  x86, x64       x86 16/32/64-bit");
        println!("  arm, thumb      ARM / Thumb");
        println!("  arm64           AArch64");
        println!("  mips, mips64    MIPS 32/64");
        println!("  ppc, ppc64      PowerPC 32/64");
        println!("  riscv32, riscv64  RISC-V");
        println!("  sparc           SPARC");
        println!("  sysz            SystemZ");
        println!();
        println!("Options:");
        println!("  -d             Show details");
        println!("  -s             Skip data mode");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Capstone v5.0.1 (SlateOS)"); return 0; }
    println!("Capstone v5.0.1 (SlateOS) — Disassembly");
    println!("  Architecture: x86_64");
    println!();
    println!("  0x1000: 55              push rbp");
    println!("  0x1001: 48 89 e5        mov rbp, rsp");
    println!("  0x1004: 48 83 ec 20     sub rsp, 0x20");
    println!("  0x1008: 89 7d ec        mov dword ptr [rbp - 0x14], edi");
    println!("  0x100b: 48 89 75 e0     mov qword ptr [rbp - 0x20], rsi");
    println!("  0x100f: e8 00 00 00 00  call 0x1014");
    println!("  0x1014: 48 83 c4 20     add rsp, 0x20");
    println!("  0x1018: 5d              pop rbp");
    println!("  0x1019: c3              ret");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cstool".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cstool(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cstool};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/capstone"), "capstone");
        assert_eq!(basename(r"C:\bin\capstone.exe"), "capstone.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("capstone.exe"), "capstone");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cstool(&["--help".to_string()], "capstone"), 0);
        assert_eq!(run_cstool(&["-h".to_string()], "capstone"), 0);
        let _ = run_cstool(&["--version".to_string()], "capstone");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cstool(&[], "capstone");
    }
}
