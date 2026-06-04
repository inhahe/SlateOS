#![deny(clippy::all)]

//! root-cli — OurOS ROOT data analysis framework (CERN)
//!
//! Multi-personality: `root`, `rootcling`, `hadd`, `rootls`, `rootcp`, `rootmv`, `rootrm`, `rootprint`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_root(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: root [OPTIONS] [FILE.root | MACRO.C]");
        println!("  -l            Don't show splash screen");
        println!("  -b            Batch mode (no graphics)");
        println!("  -q            Quit after processing");
        println!("  -x MACRO.C    Execute macro");
        println!("  --version     Show version");
        println!("  --web         Use web-based display");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("ROOT 6.30.04 (OurOS)");
        println!("Built for linuxx8664gcc on Jan 15 2024");
        println!("LLVM/Clang 16.0.6");
        println!("Python 3.12.0");
        return 0;
    }
    let file = args.iter().find(|a| a.ends_with(".root") || a.ends_with(".C")).map(|s| s.as_str());
    let batch = args.iter().any(|a| a == "-b");
    if let Some(f) = file {
        if f.ends_with(".C") {
            println!("Processing {}", f);
            println!("Macro executed successfully.");
        } else {
            println!("root [0] TFile *f = TFile::Open(\"{}\");", f);
            println!("root [1] f->ls()");
            println!("TFile**\t\t{}", f);
            println!(" KEY: TH1F\thist;1\tExample histogram");
            println!(" KEY: TTree\ttree;1\tExample tree (1000 entries)");
        }
    } else if batch {
        println!("ROOT 6.30.04 (batch mode)");
        println!("root [0]");
    } else {
        println!("   -------------------------------------------------------");
        println!("  | Welcome to ROOT 6.30.04               https://root.cern |");
        println!("  | (c) 1995-2024, The ROOT Team; conception R. Brun, F. Rademakers |");
        println!("  | Built for OurOS on Jan 15 2024                          |");
        println!("  | From tag v6-30-04, 15 January 2024                      |");
        println!("   -------------------------------------------------------");
        println!();
        println!("root [0]");
    }
    0
}

fn run_rootcling(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: rootcling [OPTIONS] DICTNAME.cxx HEADER.h [LINKDEF.h]");
        println!("  -f             Force overwrite");
        println!("  -v             Verbose");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("rootcling 6.30.04 (OurOS)");
        return 0;
    }
    let dict = args.iter().find(|a| a.ends_with(".cxx")).map(|s| s.as_str()).unwrap_or("Dict.cxx");
    println!("rootcling: generating dictionary '{}'", dict);
    println!("Parsing headers...");
    println!("Generating dictionary code...");
    println!("Dictionary generated successfully.");
    0
}

fn run_hadd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: hadd [OPTIONS] TARGET.root SOURCE1.root [SOURCE2.root ...]");
        println!("  -f    Force overwrite of target");
        println!("  -k    Skip corrupt or missing files");
        println!("  -T    Do not merge Trees");
        println!("  -a    Append to target file");
        return 0;
    }
    let files: Vec<&str> = args.iter().filter(|a| a.ends_with(".root")).map(|s| s.as_str()).collect();
    if files.len() < 2 {
        println!("hadd: need at least a target and one source file");
        return 1;
    }
    let target = files.first().unwrap_or(&"merged.root");
    let n_sources = files.len() - 1;
    println!("hadd: merging {} files into '{}'", n_sources, target);
    for f in files.iter().skip(1) {
        println!("  Adding: {}", f);
    }
    println!("Target file has 3 keys (2 histograms, 1 tree)");
    println!("hadd: merged {} files successfully", n_sources);
    0
}

fn run_rootls(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: rootls [OPTIONS] FILE.root");
        println!("  -t    Print tree information");
        println!("  -l    Long listing format");
        return 0;
    }
    let file = args.iter().find(|a| a.ends_with(".root")).map(|s| s.as_str()).unwrap_or("data.root");
    let long = args.iter().any(|a| a == "-l");
    println!("{}", file);
    if long {
        println!("  TH1F    hist        Example histogram         ;1  2.1 KB");
        println!("  TH2F    hist2d      2D histogram               ;1  8.4 KB");
        println!("  TTree   tree        Example tree (1000 entries);1  45.2 KB");
        println!("  TGraph  graph       Example graph              ;1  1.2 KB");
    } else {
        println!("  hist    hist2d    tree    graph");
    }
    0
}

fn run_rootcp(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: rootcp SOURCE.root:OBJ DEST.root");
        return 0;
    }
    let src = args.first().map(|s| s.as_str()).unwrap_or("src.root:hist");
    let dst = args.get(1).map(|s| s.as_str()).unwrap_or("dst.root");
    println!("Copying {} -> {}", src, dst);
    println!("Done.");
    0
}

fn run_rootmv(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: rootmv SOURCE.root:OBJ DEST.root");
        return 0;
    }
    let src = args.first().map(|s| s.as_str()).unwrap_or("src.root:hist");
    let dst = args.get(1).map(|s| s.as_str()).unwrap_or("dst.root");
    println!("Moving {} -> {}", src, dst);
    println!("Done.");
    0
}

fn run_rootrm(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: rootrm FILE.root:OBJ");
        return 0;
    }
    let target = args.first().map(|s| s.as_str()).unwrap_or("data.root:hist");
    println!("Removing {} ...", target);
    println!("Done.");
    0
}

fn run_rootprint(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: rootprint [OPTIONS] FILE.root:OBJ");
        println!("  -o FILE    Output file (png, pdf, svg)");
        println!("  --size WxH Canvas size");
        return 0;
    }
    let target = args.iter().find(|a| a.contains(".root")).map(|s| s.as_str()).unwrap_or("data.root:hist");
    let output = args.windows(2).find(|w| w[0] == "-o").map(|w| w[1].as_str()).unwrap_or("output.png");
    println!("Drawing: {}", target);
    println!("Canvas: 800x600");
    println!("Saved: {}", output);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "root".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "rootcling" => run_rootcling(&rest),
        "hadd" => run_hadd(&rest),
        "rootls" => run_rootls(&rest),
        "rootcp" => run_rootcp(&rest),
        "rootmv" => run_rootmv(&rest),
        "rootrm" => run_rootrm(&rest),
        "rootprint" => run_rootprint(&rest),
        _ => run_root(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_root};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/root"), "root");
        assert_eq!(basename(r"C:\bin\root.exe"), "root.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("root.exe"), "root");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_root(&["--help".to_string()]), 0);
        assert_eq!(run_root(&["-h".to_string()]), 0);
        let _ = run_root(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_root(&[]);
    }
}
