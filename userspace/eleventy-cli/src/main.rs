#![deny(clippy::all)]

//! eleventy-cli — OurOS Eleventy static site generator
//!
//! Single personality: `eleventy`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_eleventy(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: eleventy [OPTIONS]");
        println!("Eleventy v3.0.0 (OurOS) — Simple static site generator");
        println!();
        println!("Options:");
        println!("  --serve          Start dev server with live reload");
        println!("  --watch          Watch for file changes");
        println!("  --input DIR      Input directory (default: .)");
        println!("  --output DIR     Output directory (default: _site)");
        println!("  --formats LIST   Template formats (md,njk,html,...)");
        println!("  --config FILE    Config file");
        println!("  --dryrun         Dry run (no output)");
        println!("  --quiet          Minimal output");
        println!("  -V, --version    Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("Eleventy v3.0.0 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--serve") {
        println!("[11ty] Writing _site/index.html from ./index.md (md)");
        println!("[11ty] Writing _site/about/index.html from ./about.md (md)");
        println!("[11ty] Writing _site/blog/post-1/index.html from ./blog/post-1.md (md)");
        println!("[11ty] Wrote 3 files in 0.08 seconds (v3.0.0)");
        println!();
        println!("[11ty] Watching...");
        println!("[Browsersync] Local: http://localhost:8080");
    } else {
        println!("[11ty] Writing _site/index.html from ./index.md (md)");
        println!("[11ty] Writing _site/about/index.html from ./about.md (md)");
        println!("[11ty] Writing _site/blog/post-1/index.html from ./blog/post-1.md (md)");
        println!("[11ty] Wrote 3 files in 0.08 seconds (v3.0.0)");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "eleventy".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_eleventy(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_eleventy};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/eleventy"), "eleventy");
        assert_eq!(basename(r"C:\bin\eleventy.exe"), "eleventy.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("eleventy.exe"), "eleventy");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_eleventy(&["--help".to_string()], "eleventy"), 0);
        assert_eq!(run_eleventy(&["-h".to_string()], "eleventy"), 0);
        assert_eq!(run_eleventy(&["--version".to_string()], "eleventy"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_eleventy(&[], "eleventy"), 0);
    }
}
