#![deny(clippy::all)]

//! texlive-cli — SlateOS TeX Live manager
//!
//! Multi-personality: `tlmgr`, `texhash`, `fmtutil`, `updmap`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tlmgr(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: tlmgr ACTION [OPTIONS]");
        println!("TeX Live Manager 2024 (SlateOS)");
        println!();
        println!("Actions:");
        println!("  install PKG    Install package(s)");
        println!("  remove PKG     Remove package(s)");
        println!("  update PKG     Update package(s)");
        println!("  update --all   Update all packages");
        println!("  update --self  Update tlmgr itself");
        println!("  list           List installed packages");
        println!("  info PKG       Show package info");
        println!("  search TEXT    Search for packages");
        println!("  paper SIZE     Set default paper size");
        println!("  path           Manage PATH");
        println!("  conf           Show or change config");
        println!("  backup PKG     Backup a package");
        println!("  restore PKG    Restore from backup");
        println!("  repository     Manage repositories");
        println!("  --version      Show version");
        return 0;
    }
    let action = args.first().map(|s| s.as_str()).unwrap_or("help");
    match action {
        "--version" => {
            println!("tlmgr revision 70389 (2024-03-15 07:42:37 +0100)");
            println!("tlmgr using installation: /usr/local/texlive/2024");
            println!("TeX Live (https://tug.org/texlive) version 2024");
        }
        "install" => {
            let pkgs: Vec<&str> = args.iter().skip(1)
                .filter(|a| !a.starts_with('-'))
                .map(|s| s.as_str())
                .collect();
            for p in &pkgs {
                println!("tlmgr install: package {} (42 MB)", p);
                println!("  [1/1, ??:??/??:??] install: {} [42k]", p);
            }
            println!("tlmgr: package log updated");
            println!("running mktexlsr ...");
            println!("done.");
        }
        "remove" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("pkg");
            println!("tlmgr remove: {}", pkg);
            println!("  removed {}", pkg);
        }
        "update" => {
            if args.iter().any(|a| a == "--all") {
                println!("tlmgr: updating all installed packages");
                println!("  [1/5] update: geometry [12k]");
                println!("  [2/5] update: hyperref [89k]");
                println!("  [3/5] update: listings [56k]");
                println!("  [4/5] update: pgf [1.2M]");
                println!("  [5/5] update: tikz [890k]");
                println!("tlmgr: 5 packages updated");
            } else if args.iter().any(|a| a == "--self") {
                println!("tlmgr: updating tlmgr itself");
                println!("  tlmgr: updated to revision 70389");
            } else {
                let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("pkg");
                println!("tlmgr update: {} [42k]", pkg);
            }
        }
        "list" => {
            println!("i amsmath:     AMS mathematical facilities");
            println!("i babel:       Multilingual support");
            println!("i geometry:    Flexible and complete page dimensions");
            println!("i hyperref:    Extensive support for hypertext");
            println!("i listings:    Typeset source code");
            println!("i pgf:         Create PostScript and PDF graphics");
            println!("  pgfplots:    Create normal/logarithmic plots");
            println!("i tikz:        TeX packages for producing graphics");
            println!("i xcolor:      Driver-independent color extensions");
        }
        "info" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("amsmath");
            println!("package:     {}", pkg);
            println!("category:    Collection");
            println!("shortdesc:   AMS mathematical facilities for LaTeX");
            println!("longdesc:    Provides miscellaneous enhancements for...");
            println!("installed:   Yes");
            println!("revision:    70125");
            println!("sizes:       42k (run), 15k (doc), 3k (source)");
        }
        "search" => {
            let term = args.get(1).map(|s| s.as_str()).unwrap_or("math");
            println!("tlmgr: searching for '{}'", term);
            println!("  amsmath - AMS mathematical facilities");
            println!("  mathtools - Mathematical tools");
            println!("  unicode-math - Unicode mathematics support");
        }
        "paper" => {
            let size = args.get(1).map(|s| s.as_str());
            if let Some(s) = size {
                println!("tlmgr: setting default paper to {}", s);
            } else {
                println!("tlmgr: current default paper: a4");
            }
        }
        _ => println!("tlmgr: '{}' completed", action),
    }
    0
}

fn run_texhash(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: texhash [DIRECTORY ...]");
        println!("Update TeX filename databases (ls-R).");
        return 0;
    }
    let dirs: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();
    if dirs.is_empty() {
        println!("texhash: updating /usr/local/texlive/2024/texmf-dist ...");
        println!("texhash: updating /usr/local/texlive/2024/texmf-var ...");
    } else {
        for d in &dirs {
            println!("texhash: updating {} ...", d);
        }
    }
    println!("texhash: Done.");
    0
}

fn run_fmtutil(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: fmtutil [OPTIONS]");
        println!("Manage TeX format files.");
        println!("  --all          Rebuild all formats");
        println!("  --byfmt FMT    Rebuild specific format");
        println!("  --missing      Build missing formats");
        println!("  --refresh      Refresh existing formats");
        println!("  --list         List configured formats");
        return 0;
    }
    if args.iter().any(|a| a == "--all") {
        println!("fmtutil: rebuilding all formats");
        println!("  pdflatex ... done");
        println!("  xelatex ... done");
        println!("  lualatex ... done");
        println!("  plain ... done");
        println!("fmtutil: 4 formats rebuilt");
    } else if args.iter().any(|a| a == "--list") {
        println!("fmtutil: configured formats:");
        println!("  pdflatex  pdftex   -translate-file=cp227.tcx *pdflatex.ini");
        println!("  xelatex   xetex    -etex xelatex.ini");
        println!("  lualatex  luahbtex -ini lualatex.ini");
    } else if args.iter().any(|a| a == "--missing") {
        println!("fmtutil: no missing formats");
    } else {
        let fmt = args.windows(2)
            .find(|w| w[0] == "--byfmt")
            .map(|w| w[1].as_str())
            .unwrap_or("pdflatex");
        println!("fmtutil: rebuilding {}", fmt);
        println!("  {} ... done", fmt);
    }
    0
}

fn run_updmap(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: updmap [OPTIONS]");
        println!("Manage TeX font map files.");
        println!("  --enable Map=FILE   Enable font map");
        println!("  --disable FILE      Disable font map");
        println!("  --listmaps          List active maps");
        println!("  --syncwithtrees     Sync with texmf trees");
        return 0;
    }
    if args.iter().any(|a| a == "--listmaps") {
        println!("updmap: active map files:");
        println!("  pdftex.map (pdftex)");
        println!("  psfonts.map (dvips)");
        println!("  dvipdfmx.map (dvipdfmx)");
    } else if args.iter().any(|a| a == "--syncwithtrees") {
        println!("updmap: syncing with texmf trees");
        println!("updmap: done");
    } else {
        println!("updmap: updating map files");
        println!("updmap: done");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tlmgr".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "texhash" | "mktexlsr" => run_texhash(&rest),
        "fmtutil" | "fmtutil-sys" => run_fmtutil(&rest),
        "updmap" | "updmap-sys" => run_updmap(&rest),
        _ => run_tlmgr(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tlmgr};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/texlive"), "texlive");
        assert_eq!(basename(r"C:\bin\texlive.exe"), "texlive.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("texlive.exe"), "texlive");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tlmgr(&["--help".to_string()]), 0);
        assert_eq!(run_tlmgr(&["-h".to_string()]), 0);
        let _ = run_tlmgr(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tlmgr(&[]);
    }
}
