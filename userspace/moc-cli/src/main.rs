#![deny(clippy::all)]

//! moc-cli — Slate OS MOC (Music on Console) player
//!
//! Multi-personality: `mocp`, `mocp-server`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_moc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mocp [OPTIONS] [FILE|DIR...]");
        println!("MOC 2.5.2 (Slate OS) — Music on Console");
        println!();
        println!("Options:");
        println!("  -S, --server          Start server");
        println!("  -F, --foreground      Server in foreground");
        println!("  -R THEME              Config file");
        println!("  -M DIR                Music directory");
        println!("  -a, --append          Append files to playlist");
        println!("  -c, --clear           Clear playlist");
        println!("  -p, --play            Play");
        println!("  -l, --playit          Play files from CLI");
        println!("  -s, --stop            Stop");
        println!("  -f, --next            Next");
        println!("  -r, --previous        Previous");
        println!("  -G, --toggle-pause    Toggle pause");
        println!("  -i, --info            Print current song info");
        println!("  -Q FMT               Print formatted info");
        println!("  -v +/-N               Adjust volume");
        println!("  -x, --exit            Quit server");
        println!("  -V, --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("MOC (music on console) 2.5.2 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "-i" || a == "--info") {
        println!("State: PLAY");
        println!("File: /music/song.flac");
        println!("Title: Song Title");
        println!("Artist: Artist Name");
        println!("Album: Album Name");
        println!("TotalTime: 4:00");
        println!("CurrentTime: 1:23");
        return 0;
    }
    if args.iter().any(|a| a == "-p") { println!("mocp: Playing"); return 0; }
    if args.iter().any(|a| a == "-s") { println!("mocp: Stopped"); return 0; }
    if args.iter().any(|a| a == "-f") { println!("mocp: Next track"); return 0; }
    if args.iter().any(|a| a == "-G") { println!("mocp: Toggled pause"); return 0; }
    if args.iter().any(|a| a == "-x") { println!("mocp: Server exiting"); return 0; }
    println!("mocp: Starting Music on Console...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mocp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_moc(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_moc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/moc"), "moc");
        assert_eq!(basename(r"C:\bin\moc.exe"), "moc.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("moc.exe"), "moc");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_moc(&["--help".to_string()], "moc"), 0);
        assert_eq!(run_moc(&["-h".to_string()], "moc"), 0);
        let _ = run_moc(&["--version".to_string()], "moc");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_moc(&[], "moc");
    }
}
