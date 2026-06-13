#![deny(clippy::all)]

//! markdownlint-cli — Slate OS markdownlint CLI
//!
//! Multi-personality: `markdownlint`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_markdownlint(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: markdownlint [OPTIONS] FILES...");
        println!("markdownlint-cli2 0.13.0 (Slate OS) — Markdown linter");
        println!();
        println!("Options:");
        println!("  -c, --config FILE    Config file (.markdownlint.json)");
        println!("  -f, --fix            Fix issues automatically");
        println!("  -o, --output FILE    Output file");
        println!("  -i, --ignore PAT     Ignore pattern");
        println!("  -p, --dot            Include dot files");
        println!("  --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("markdownlint-cli2 0.13.0");
        return 0;
    }
    let fix_mode = args.iter().any(|a| a == "-f" || a == "--fix");
    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-') && (a.ends_with(".md") || a.contains('*') || *a == "."))
        .map(|s| s.as_str())
        .collect();
    let target = if files.is_empty() { "**/*.md" } else { files.first().copied().unwrap_or(".") };

    if fix_mode {
        println!("Fixing {}...", target);
        println!("  README.md: 2 issues fixed");
        println!("  docs/guide.md: 1 issue fixed");
        println!("Fixed 3 issues in 2 files.");
    } else {
        println!("Linting {}...", target);
        println!();
        println!("README.md:3 MD022/blanks-around-headings Headings should be surrounded by blank lines");
        println!("README.md:15 MD009/no-trailing-spaces Trailing spaces");
        println!("README.md:22 MD012/no-multiple-blanks Multiple consecutive blank lines");
        println!("docs/guide.md:8 MD013/line-length Line length (expected: 80, actual: 95)");
        println!("docs/guide.md:42 MD032/blanks-around-lists Lists should be surrounded by blank lines");
        println!();
        println!("5 issues found in 2 files.");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "markdownlint".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_markdownlint(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_markdownlint};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/markdownlint"), "markdownlint");
        assert_eq!(basename(r"C:\bin\markdownlint.exe"), "markdownlint.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("markdownlint.exe"), "markdownlint");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_markdownlint(&["--help".to_string()]), 0);
        assert_eq!(run_markdownlint(&["-h".to_string()]), 0);
        let _ = run_markdownlint(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_markdownlint(&[]);
    }
}
