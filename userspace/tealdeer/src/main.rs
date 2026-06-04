#![deny(clippy::all)]

//! tealdeer — OurOS fast tldr client (simplified man pages)
//!
//! Single personality: `tldr`

use std::env;
use std::process;

fn run_tldr(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tldr [OPTIONS] [COMMAND]...");
        println!();
        println!("A fast TLDR client for collaborative cheatsheets.");
        println!();
        println!("Options:");
        println!("  -l, --list             List all commands");
        println!("  -f, --render <FILE>    Render a specific TLDR file");
        println!("  -p, --platform <OS>    Override platform (linux/osx/windows/sunos)");
        println!("  -L, --language <LANG>  Override language");
        println!("  -u, --update           Update local cache");
        println!("  --seed-config          Create seed config file");
        println!("  --show-paths           Show used config/cache paths");
        println!("  --clean-cache          Clear local cache");
        println!("  -q, --quiet            Suppress info messages");
        println!("  --color <WHEN>         Color (auto/always/never)");
        println!("  -V, --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("tealdeer 1.6.1 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "-u" || a == "--update") {
        println!("Updating cache...");
        println!("  Downloaded tldr-pages archive (2.1 MiB)");
        println!("  Extracted 3,456 pages");
        println!("  Cache updated successfully.");
        return 0;
    }
    if args.iter().any(|a| a == "--clean-cache") {
        println!("Cache cleared.");
        return 0;
    }
    if args.iter().any(|a| a == "--show-paths") {
        println!("Config dir:  ~/.config/tealdeer/");
        println!("Config file: ~/.config/tealdeer/config.toml");
        println!("Cache dir:   ~/.cache/tealdeer/");
        println!("Pages dir:   ~/.cache/tealdeer/tldr-pages/");
        return 0;
    }
    if args.iter().any(|a| a == "-l" || a == "--list") {
        println!("awk, bash, cat, chmod, cp, curl, cut, date, dd, df, diff,");
        println!("du, echo, env, find, git, grep, head, htop, ip, kill, less,");
        println!("ln, ls, man, mkdir, mount, mv, nc, netstat, nmap, passwd,");
        println!("ping, ps, python, rm, rsync, scp, sed, sort, ssh, tail,");
        println!("tar, top, traceroute, uniq, vim, wc, wget, xargs, zip");
        println!("(... 3,456 pages available)");
        return 0;
    }

    let command: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if command.is_empty() {
        eprintln!("Error: command name required. See --help.");
        return 1;
    }

    let cmd = command.join("-");
    match cmd.as_str() {
        "tar" => {
            println!("  tar");
            println!("  Archiving utility.");
            println!();
            println!("  - Create an archive from files:");
            println!("    tar cf {{target.tar}} {{path/to/file1}} {{path/to/file2}}");
            println!();
            println!("  - Create a gzipped archive:");
            println!("    tar czf {{target.tar.gz}} {{path/to/file1}} {{path/to/file2}}");
            println!();
            println!("  - Extract an archive to the current directory:");
            println!("    tar xf {{source.tar[.gz|.bz2|.xz]}}");
            println!();
            println!("  - List files in an archive:");
            println!("    tar tf {{source.tar}}");
        }
        "git" => {
            println!("  git");
            println!("  Distributed version control system.");
            println!();
            println!("  - Clone a repository:");
            println!("    git clone {{url}}");
            println!();
            println!("  - Show working tree status:");
            println!("    git status");
            println!();
            println!("  - Stage all changed files:");
            println!("    git add -A");
            println!();
            println!("  - Commit with a message:");
            println!("    git commit -m \"{{message}}\"");
        }
        _ => {
            println!("  {}", cmd);
            println!("  (Summary for '{}' — simulated)", cmd);
            println!();
            println!("  - Common usage:");
            println!("    {} {{arguments}}", cmd);
            println!();
            println!("  - Show help:");
            println!("    {} --help", cmd);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tldr(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_tldr};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tldr(vec!["--help".to_string()]), 0);
        assert_eq!(run_tldr(vec!["-h".to_string()]), 0);
        let _ = run_tldr(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tldr(vec![]);
    }
}
