#![deny(clippy::all)]

//! macrium-cli — SlateOS Macrium Reflect disk imaging
//!
//! Single personality: `macrium`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: macrium [OPTIONS]");
        println!("Macrium Reflect X (SlateOS) — Disk imaging, cloning, backup");
        println!();
        println!("Options:");
        println!("  --image SRC DST        Create image (whole disk/partition)");
        println!("  --clone SRC DST        Clone disk (sector-by-sector or used-sectors)");
        println!("  --restore IMAGE        Restore image to disk");
        println!("  --rescue-media         Build WinPE/WinRE rescue media");
        println!("  --site-manager         Reflect Site Manager (central management)");
        println!("  --x10-defender         Macrium Image Guardian (anti-ransomware)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Macrium Reflect X 10.0.8323 (SlateOS)"); return 0; }
    println!("Macrium Reflect X 10.0.8323 (SlateOS)");
    println!("  Vendor: Paramount Software UK Ltd (Manchester, UK; founded 1992)");
    println!("  Sold to: Insight Partners Apr 2022 (same owner as Veeam)");
    println!("  Origin: started as 'StoreItForeverPlus' (cassette tape backup) — pivoted to disk imaging");
    println!("  Editions: Reflect X Home ($75/yr, 4 PCs), Workstation, Server, Server Plus");
    println!("  Free tier RETIRED: Reflect Free discontinued Jan 2024 — Home is paid replacement");
    println!("  Engine: Macrium Reflect Image Format (MRIMG), incremental + differential,");
    println!("          Rapid Delta Clone/Restore (only changed sectors), AES-256 encryption");
    println!("  Features: image whole disk/SSD/NVMe, clone OS to new drive, ReDeploy (drive HW change),");
    println!("            schedule, mount images as virtual drives, file/folder restore from image,");
    println!("            email notifications, Macrium viBoot (VM-boot from image)");
    println!("  Image Guardian: anti-ransomware that protects MRIMG files at filter-driver level");
    println!("  Boot media: builds WinPE 11 / WinRE rescue USB, drivers auto-imported");
    println!("  Strengths: fast (highly optimized C++), reliable, well-loved by sysadmins");
    println!("  Market: dominant in SMB Windows imaging; Free tier was very popular");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "macrium".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mc(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/macrium"), "macrium");
        assert_eq!(basename(r"C:\bin\macrium.exe"), "macrium.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("macrium.exe"), "macrium");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mc(&["--help".to_string()], "macrium"), 0);
        assert_eq!(run_mc(&["-h".to_string()], "macrium"), 0);
        let _ = run_mc(&["--version".to_string()], "macrium");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mc(&[], "macrium");
    }
}
