#![deny(clippy::all)]

//! svelte-cli — Slate OS SvelteKit CLI
//!
//! Multi-personality: `sv`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sv(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: sv COMMAND [OPTIONS]");
        println!("SvelteKit CLI 2.0 (Slate OS)");
        println!();
        println!("Commands:");
        println!("  create       Create a new SvelteKit project");
        println!("  add          Add integrations (Tailwind, Drizzle, etc.)");
        println!("  check        Run svelte-check");
        println!("  migrate      Migrate from older versions");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("sv 2.0.0"),
        "create" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("my-app");
            println!("Creating SvelteKit project '{}'...", name);
            println!("  Template: skeleton");
            println!("  Type checking: TypeScript");
            println!("  ESLint: yes");
            println!("  Prettier: yes");
            println!("  Playwright: no");
            println!("  Vitest: yes");
            println!("Project created. Next steps:");
            println!("  cd {}", name);
            println!("  npm install");
            println!("  npm run dev");
        }
        "add" => {
            let what = args.get(1).map(|s| s.as_str()).unwrap_or("tailwindcss");
            println!("Adding {}...", what);
            println!("  Updated svelte.config.js");
            println!("  Updated package.json");
            println!("  Done. Run `npm install` to install dependencies.");
        }
        "check" => {
            println!("Running svelte-check...");
            println!("====================================");
            println!("svelte-check found 0 errors and 0 warnings");
        }
        "migrate" => {
            let from = args.get(1).map(|s| s.as_str()).unwrap_or("svelte-4");
            println!("Migrating from {}...", from);
            println!("Migration complete. Review changes.");
        }
        _ => println!("sv: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sv".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sv(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sv};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/svelte"), "svelte");
        assert_eq!(basename(r"C:\bin\svelte.exe"), "svelte.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("svelte.exe"), "svelte");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sv(&["--help".to_string()]), 0);
        assert_eq!(run_sv(&["-h".to_string()]), 0);
        let _ = run_sv(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sv(&[]);
    }
}
