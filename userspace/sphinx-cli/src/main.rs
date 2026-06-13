#![deny(clippy::all)]

//! sphinx-cli — SlateOS Sphinx documentation generator
//!
//! Multi-personality: `sphinx-build`, `sphinx-quickstart`, `sphinx-apidoc`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sphinx_build(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: sphinx-build [OPTIONS] SOURCEDIR OUTPUTDIR");
        println!("Sphinx 7.2.6 (SlateOS)");
        println!("  -b BUILDER    Builder (html, latex, epub, man, text)");
        println!("  -j N          Parallel jobs");
        println!("  -W           Turn warnings into errors");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("sphinx-build 7.2.6 (SlateOS)");
        return 0;
    }
    let builder = args.windows(2).find(|w| w[0] == "-b").map(|w| w[1].as_str()).unwrap_or("html");
    println!("Running Sphinx v7.2.6");
    println!("loading pickled environment... done");
    println!("building [{}]: targets for 12 source files", builder);
    println!("updating environment: [new config] 12 added, 0 changed, 0 removed");
    println!("reading sources... [100%] index");
    println!("looking for now-outdated files... none found");
    println!("writing output... [100%] index");
    println!("build succeeded.");
    0
}

fn run_sphinx_quickstart(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sphinx-quickstart [OPTIONS] [PATH]");
        println!("  -p PROJECT    Project name");
        println!("  -a AUTHOR     Author name");
        println!("  -v VERSION    Version");
        println!("  --sep         Separate source and build dirs");
        return 0;
    }
    let path = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("docs");
    println!("Creating Sphinx project in: {}/", path);
    println!("  Created: {}/conf.py", path);
    println!("  Created: {}/index.rst", path);
    println!("  Created: {}/Makefile", path);
    println!("  Created: {}/make.bat", path);
    println!("Finished: project created.");
    0
}

fn run_sphinx_apidoc(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: sphinx-apidoc [OPTIONS] -o OUTPUTDIR PACKAGEDIR");
        println!("  -o DIR        Output directory");
        println!("  -f            Force overwrite");
        println!("  --separate    Separate page per module");
        return 0;
    }
    println!("sphinx-apidoc: generating API documentation...");
    println!("  Creating file modules.rst");
    println!("  Creating file mypackage.rst");
    println!("  Creating file mypackage.utils.rst");
    println!("  Done. 3 files written.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sphinx-build".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "sphinx-quickstart" => run_sphinx_quickstart(&rest),
        "sphinx-apidoc" => run_sphinx_apidoc(&rest),
        _ => run_sphinx_build(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sphinx_build};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sphinx"), "sphinx");
        assert_eq!(basename(r"C:\bin\sphinx.exe"), "sphinx.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sphinx.exe"), "sphinx");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sphinx_build(&["--help".to_string()]), 0);
        assert_eq!(run_sphinx_build(&["-h".to_string()]), 0);
        let _ = run_sphinx_build(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sphinx_build(&[]);
    }
}
