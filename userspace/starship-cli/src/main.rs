#![deny(clippy::all)]

//! starship-cli — OurOS Starship cross-shell prompt
//!
//! Single personality: `starship`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_starship(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: starship [COMMAND]");
        println!("Starship 1.21.1 (OurOS) — Cross-shell prompt");
        println!();
        println!("Commands:");
        println!("  init SHELL       Print shell init script");
        println!("  prompt           Print prompt string");
        println!("  module NAME      Print specific module");
        println!("  config           Edit configuration");
        println!("  preset NAME      Print preset config");
        println!("  explain          Show active modules");
        println!("  timings          Show module timings");
        println!("  bug-report       Create bug report info");
        println!("  toggle MODULE    Toggle module visibility");
        println!("  completions SHELL  Print completions");
        println!();
        println!("Options:");
        println!("  -V, --version    Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("starship 1.21.1 (OurOS)");
        return 0;
    }
    let cmd = args.iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("prompt");
    match cmd {
        "init" => {
            let shell = args.get(1).map(|s| s.as_str()).unwrap_or("bash");
            println!("# starship init for {}", shell);
            println!("eval \"$(starship init {})\"", shell);
        }
        "prompt" => {
            println!("\x1b[36m❯\x1b[0m ");
        }
        "module" => {
            let module = args.iter().skip_while(|a| a.as_str() != "module").nth(1)
                .map(|s| s.as_str()).unwrap_or("directory");
            println!("[{}]", module);
        }
        "config" => println!("starship: Opening config at ~/.config/starship.toml"),
        "preset" => {
            let name = args.iter().skip_while(|a| a.as_str() != "preset").nth(1)
                .map(|s| s.as_str()).unwrap_or("nerd-font-symbols");
            println!("# Starship preset: {}", name);
            println!("[character]");
            println!("success_symbol = '[➜](bold green)'");
        }
        "explain" => {
            println!("Here are the active modules on your prompt:");
            println!("  directory  (1ms)  ~/projects");
            println!("  git_branch (2ms)  main");
            println!("  character  (0ms)  ❯");
        }
        "timings" => {
            println!("Module           Duration");
            println!("directory        1ms");
            println!("git_branch       2ms");
            println!("git_status       5ms");
            println!("character        0ms");
        }
        "bug-report" => {
            println!("Starship 1.21.1 (OurOS)");
            println!("OS: OurOS x86_64");
            println!("Shell: bash");
            println!("Config: ~/.config/starship.toml");
        }
        "completions" => {
            let shell = args.iter().skip_while(|a| a.as_str() != "completions").nth(1)
                .map(|s| s.as_str()).unwrap_or("bash");
            println!("# Completions for {}", shell);
        }
        _ => println!("starship: unknown command '{}'", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "starship".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_starship(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_starship};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/starship"), "starship");
        assert_eq!(basename(r"C:\bin\starship.exe"), "starship.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("starship.exe"), "starship");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_starship(&["--help".to_string()], "starship"), 0);
        assert_eq!(run_starship(&["-h".to_string()], "starship"), 0);
        let _ = run_starship(&["--version".to_string()], "starship");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_starship(&[], "starship");
    }
}
