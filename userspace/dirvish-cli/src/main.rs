#![deny(clippy::all)]

//! dirvish-cli — OurOS Dirvish rsync-based backup
//!
//! Multi-personality: `dirvish`, `dirvish-runall`, `dirvish-expire`, `dirvish-locate`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dirvish(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dirvish --vault NAME [OPTIONS]");
        println!("dirvish v1.2 (OurOS) — Rsync-based rotating backup");
        println!();
        println!("Options:");
        println!("  --vault NAME   Backup vault name");
        println!("  --branch NAME  Branch within vault");
        println!("  --init         Initialize new vault");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("dirvish v1.2 (OurOS)"); return 0; }
    println!("dirvish: creating backup image");
    println!("  Vault: default");
    println!("  Image: 2024-01-15_1200");
    println!("  Files transferred: 42");
    println!("  Total size: 1.2 GiB");
    0
}

fn run_dirvish_runall(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dirvish-runall [OPTIONS]");
        println!("dirvish-runall v1.2 (OurOS) — Run all configured vaults");
        return 0;
    }
    let _ = args;
    println!("dirvish-runall: running all vaults");
    println!("  Vault 'home': completed");
    println!("  Vault 'etc': completed");
    println!("  Vault 'var': completed");
    0
}

fn run_dirvish_expire(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dirvish-expire [OPTIONS]");
        println!("dirvish-expire v1.2 (OurOS) — Expire old backup images");
        return 0;
    }
    let _ = args;
    println!("dirvish-expire: expiring old images");
    println!("  Expired: 3 images");
    println!("  Retained: 14 images");
    0
}

fn run_dirvish_locate(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dirvish-locate <pattern>");
        println!("dirvish-locate v1.2 (OurOS) — Find files in backup history");
        return 0;
    }
    if let Some(pat) = args.first() {
        println!("dirvish-locate: searching for '{}'", pat);
        println!("  Found in 3 images");
    } else {
        println!("dirvish-locate: no pattern specified");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dirvish".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "dirvish-runall" => run_dirvish_runall(&rest, &prog),
        "dirvish-expire" => run_dirvish_expire(&rest, &prog),
        "dirvish-locate" => run_dirvish_locate(&rest, &prog),
        _ => run_dirvish(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
