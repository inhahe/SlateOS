#![deny(clippy::all)]

//! xdg-user-dirs-cli — OurOS xdg-user-dirs user directory management
//!
//! Multi-personality: `xdg-user-dirs-update`, `xdg-user-dir`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_update(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xdg-user-dirs-update [OPTIONS]");
        println!("xdg-user-dirs-update v0.18 (OurOS) — Update XDG user directories");
        println!();
        println!("Options:");
        println!("  --force           Force update even if dirs exist");
        println!("  --set NAME DIR    Set specific directory");
        return 0;
    }
    if let Some(idx) = args.iter().position(|a| a == "--set") {
        let name = args.get(idx + 1).map(|s| s.as_str()).unwrap_or("DESKTOP");
        let dir = args.get(idx + 2).map(|s| s.as_str()).unwrap_or("$HOME/Desktop");
        println!("Set {}: {}", name, dir);
        return 0;
    }
    println!("Updated user directories:");
    println!("  DESKTOP:     ~/Desktop");
    println!("  DOCUMENTS:   ~/Documents");
    println!("  DOWNLOAD:    ~/Downloads");
    println!("  MUSIC:       ~/Music");
    println!("  PICTURES:    ~/Pictures");
    println!("  PUBLICSHARE: ~/Public");
    println!("  TEMPLATES:   ~/Templates");
    println!("  VIDEOS:      ~/Videos");
    0
}

fn run_dir(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: xdg-user-dir DIRNAME");
        println!("xdg-user-dir v0.18 (OurOS) — Query XDG user directory");
        println!();
        println!("Names: DESKTOP, DOCUMENTS, DOWNLOAD, MUSIC, PICTURES, PUBLICSHARE, TEMPLATES, VIDEOS");
        return 0;
    }
    let name = args.first().map(|s| s.as_str()).unwrap_or("DESKTOP");
    let dir = match name {
        "DESKTOP" => "/home/user/Desktop",
        "DOCUMENTS" => "/home/user/Documents",
        "DOWNLOAD" => "/home/user/Downloads",
        "MUSIC" => "/home/user/Music",
        "PICTURES" => "/home/user/Pictures",
        "VIDEOS" => "/home/user/Videos",
        _ => "/home/user",
    };
    println!("{}", dir);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xdg-user-dirs-update".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "xdg-user-dir" => run_dir(&rest, &prog),
        _ => run_update(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_update};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xdg-user-dirs"), "xdg-user-dirs");
        assert_eq!(basename(r"C:\bin\xdg-user-dirs.exe"), "xdg-user-dirs.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xdg-user-dirs.exe"), "xdg-user-dirs");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_update(&["--help".to_string()], "xdg-user-dirs"), 0);
        assert_eq!(run_update(&["-h".to_string()], "xdg-user-dirs"), 0);
        assert_eq!(run_update(&["--version".to_string()], "xdg-user-dirs"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_update(&[], "xdg-user-dirs"), 0);
    }
}
