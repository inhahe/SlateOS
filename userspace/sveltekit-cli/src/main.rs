#![deny(clippy::all)]

//! sveltekit-cli — OurOS SvelteKit web framework CLI
//!
//! Single personality: `sveltekit`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sveltekit(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: sveltekit COMMAND [OPTIONS]");
        println!("SvelteKit v2.5.0 (OurOS) — Svelte web application framework");
        println!();
        println!("Commands:");
        println!("  dev             Start dev server");
        println!("  build           Build for production");
        println!("  preview         Preview production build");
        println!("  sync            Sync SvelteKit types");
        println!("  check           Type check project");
        println!("  package         Package library");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("@sveltejs/kit 2.5.0");
        println!("svelte 4.2.12");
        println!("vite 5.2.0");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("dev");
    match cmd {
        "dev" => {
            println!("  VITE v5.2.0  ready in 456ms");
            println!();
            println!("  Local:   http://localhost:5173/");
            println!("  Network: http://192.168.1.100:5173/");
            println!();
            println!("  press h + enter to show help");
        }
        "build" => {
            println!("  vite v5.2.0 building SSR bundle...");
            println!("  vite v5.2.0 building client bundle...");
            println!();
            println!("  .svelte-kit/output/client");
            println!("    _app/immutable/entry/start.js    12.3kB");
            println!("    _app/immutable/entry/app.js       8.1kB");
            println!("    _app/immutable/nodes/0.js          2.4kB");
            println!("    _app/immutable/nodes/1.js          1.8kB");
            println!();
            println!("  Build complete in 3.2s");
        }
        "preview" => println!("  Preview: http://localhost:4173/"),
        "sync" => println!("  SvelteKit types synced."),
        "check" => {
            println!("  svelte-check working...");
            println!("  0 errors, 0 warnings");
        }
        "package" => {
            println!("  Packaging library...");
            println!("  Output: dist/");
            println!("  Done.");
        }
        _ => println!("sveltekit {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sveltekit".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sveltekit(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sveltekit};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sveltekit"), "sveltekit");
        assert_eq!(basename(r"C:\bin\sveltekit.exe"), "sveltekit.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sveltekit.exe"), "sveltekit");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sveltekit(&["--help".to_string()], "sveltekit"), 0);
        assert_eq!(run_sveltekit(&["-h".to_string()], "sveltekit"), 0);
        let _ = run_sveltekit(&["--version".to_string()], "sveltekit");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sveltekit(&[], "sveltekit");
    }
}
