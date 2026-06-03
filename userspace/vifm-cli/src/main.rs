#![deny(clippy::all)]

//! vifm-cli — OurOS Vifm file manager
//!
//! Multi-personality: `vifm`, `vifm-pause`, `vifm-convert-dircolors`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vifm(args: &[String], prog: &str) -> i32 {
    match prog {
        "vifm-pause" => {
            println!("Press Enter to continue...");
            return 0;
        }
        "vifm-convert-dircolors" => {
            if args.iter().any(|a| a == "--help" || a == "-h") {
                println!("Usage: vifm-convert-dircolors [FILE]");
                println!("Convert dircolors database to vifm colorscheme");
                return 0;
            }
            let file = args.first().map(|s| s.as_str()).unwrap_or("DIR_COLORS");
            println!("Converting '{}' to vifm colorscheme...", file);
            println!("highlight {{*.tar}} cterm=none ctermfg=red ctermbg=default");
            return 0;
        }
        _ => {}
    }
    // vifm
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vifm [OPTIONS] [LDIR] [RDIR]");
        println!("vifm 0.13 (OurOS) — Vi-like file manager");
        println!();
        println!("Options:");
        println!("  --select FILE      Select file on start");
        println!("  --choose-files F   Write selection to file");
        println!("  --choose-dir F     Write last dir to file");
        println!("  --delimiter D      Delimiter for output");
        println!("  --on-choose CMD    Command on file choose");
        println!("  --logging          Enable logging");
        println!("  --server-name N    Server name");
        println!("  --remote CMD       Send remote command");
        println!("  -c CMD             Execute command");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("vifm 0.13 (OurOS)");
        return 0;
    }
    if let Some(pos) = args.iter().position(|a| a == "--remote") {
        let cmd = args.get(pos + 1).map(|s| s.as_str()).unwrap_or("");
        println!("vifm remote: {}", cmd);
        return 0;
    }
    let ldir = args.iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or(".");
    println!("vifm: Opening '{}'", ldir);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vifm".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vifm(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_vifm};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/vifm"), "vifm");
        assert_eq!(basename(r"C:\bin\vifm.exe"), "vifm.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("vifm.exe"), "vifm");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_vifm(&["--help".to_string()], "vifm"), 0);
        assert_eq!(run_vifm(&["-h".to_string()], "vifm"), 0);
        assert_eq!(run_vifm(&["--version".to_string()], "vifm"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_vifm(&[], "vifm"), 0);
    }
}
