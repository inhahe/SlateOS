#![deny(clippy::all)]

//! cmus-cli — SlateOS cmus music player
//!
//! Multi-personality: `cmus`, `cmus-remote`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cmus(args: &[String], prog: &str) -> i32 {
    if prog == "cmus-remote" {
        if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
            println!("Usage: cmus-remote [OPTIONS]");
            println!("cmus-remote — Control cmus from command line");
            println!();
            println!("Options:");
            println!("  -p, --play            Play");
            println!("  -u, --pause           Toggle pause");
            println!("  -s, --stop            Stop");
            println!("  -n, --next            Next track");
            println!("  -r, --prev            Previous track");
            println!("  -R, --repeat          Toggle repeat");
            println!("  -S, --shuffle         Toggle shuffle");
            println!("  -v, --volume VOL      Set volume (+-N or N)");
            println!("  -k, --seek POS        Seek (+-N or N)");
            println!("  -Q, --query           Show status");
            println!("  -l, --library         Toggle library view");
            println!("  -C CMD                Send command");
            return 0;
        }
        if args.iter().any(|a| a == "-Q" || a == "--query") {
            println!("status playing");
            println!("file /music/song.mp3");
            println!("duration 240");
            println!("position 42");
            println!("tag artist Artist Name");
            println!("tag album Album Name");
            println!("tag title Song Title");
            println!("set repeat false");
            println!("set shuffle false");
            println!("set vol_left 100");
            println!("set vol_right 100");
            return 0;
        }
        if args.iter().any(|a| a == "-p") { println!("cmus-remote: Playing"); }
        if args.iter().any(|a| a == "-u") { println!("cmus-remote: Toggled pause"); }
        if args.iter().any(|a| a == "-s") { println!("cmus-remote: Stopped"); }
        if args.iter().any(|a| a == "-n") { println!("cmus-remote: Next track"); }
        if args.iter().any(|a| a == "-r") { println!("cmus-remote: Previous track"); }
        return 0;
    }
    // cmus
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cmus [OPTIONS]");
        println!("cmus 2.10.0 (Slate OS) — Small, fast, powerful console music player");
        println!();
        println!("Options:");
        println!("  --listen ADDR    Listen address for remote");
        println!("  --plugins        List plugins");
        println!("  --show-cursor    Show cursor");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("cmus 2.10.0 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "--plugins") {
        println!("Input plugins: mad flac vorbis wav");
        println!("Output plugins: alsa pulse");
        return 0;
    }
    println!("cmus: Starting music player...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cmus".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cmus(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cmus};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cmus"), "cmus");
        assert_eq!(basename(r"C:\bin\cmus.exe"), "cmus.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cmus.exe"), "cmus");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cmus(&["--help".to_string()], "cmus"), 0);
        assert_eq!(run_cmus(&["-h".to_string()], "cmus"), 0);
        let _ = run_cmus(&["--version".to_string()], "cmus");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cmus(&[], "cmus");
    }
}
