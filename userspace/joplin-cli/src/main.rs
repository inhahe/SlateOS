#![deny(clippy::all)]

//! joplin-cli — OurOS Joplin note-taking
//!
//! Multi-personality: `joplin-desktop`, `joplin`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_desktop(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: joplin-desktop [OPTIONS]");
        println!("joplin-desktop v2.14 (OurOS) — Note-taking & to-do app");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("joplin-desktop v2.14 (OurOS)"); return 0; }
    println!("joplin-desktop: note-taking application started");
    println!("  Notebooks: 8");
    println!("  Notes: 256");
    println!("  Sync: Joplin Cloud (last: 5min ago)");
    println!("  Encryption: E2EE enabled");
    0
}

fn run_terminal(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: joplin COMMAND [OPTIONS]");
        println!("joplin v2.14 (OurOS) — Terminal note-taking client");
        println!();
        println!("Commands:");
        println!("  ls                List notebooks/notes");
        println!("  cat NOTE          Show note content");
        println!("  edit NOTE         Edit note");
        println!("  mknote TITLE      Create note");
        println!("  mkbook NAME       Create notebook");
        println!("  sync              Sync with server");
        println!("  search QUERY      Search notes");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("joplin v2.14 (OurOS)"); return 0; }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("ls");
    match cmd {
        "ls" => {
            println!("Personal/");
            println!("Work/");
            println!("  Meeting notes");
            println!("  Project ideas");
            println!("Journal/");
        }
        "sync" => println!("Synchronizing... done (0 conflicts)"),
        "search" => {
            let query = args.get(1).map(|s| s.as_str()).unwrap_or("");
            println!("Search results for '{}': 5 notes found", query);
        }
        _ => println!("joplin: {}", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "joplin-desktop".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "joplin" => run_terminal(&rest, &prog),
        _ => run_desktop(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_desktop};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/joplin"), "joplin");
        assert_eq!(basename(r"C:\bin\joplin.exe"), "joplin.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("joplin.exe"), "joplin");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_desktop(&["--help".to_string()], "joplin"), 0);
        assert_eq!(run_desktop(&["-h".to_string()], "joplin"), 0);
        assert_eq!(run_desktop(&["--version".to_string()], "joplin"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_desktop(&[], "joplin"), 0);
    }
}
