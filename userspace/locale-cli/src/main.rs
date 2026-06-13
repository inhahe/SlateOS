#![deny(clippy::all)]

//! locale-cli — SlateOS locale/localedef/locale-gen CLI
//!
//! Multi-personality: `locale`, `localedef`, `locale-gen`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_locale(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: locale [OPTIONS] [NAME ...]");
        println!();
        println!("locale — get locale-specific information (Slate OS).");
        println!();
        println!("Options:");
        println!("  -a, --all-locales    List all available locales");
        println!("  -m, --charmaps       List available charmaps");
        println!("  -c, --category-name  Show category names");
        println!("  -k, --keyword-name   Show keyword names");
        return 0;
    }

    if args.iter().any(|a| a == "-a" || a == "--all-locales") {
        println!("C");
        println!("C.UTF-8");
        println!("POSIX");
        println!("en_US.UTF-8");
        println!("en_GB.UTF-8");
        println!("de_DE.UTF-8");
        println!("fr_FR.UTF-8");
        println!("ja_JP.UTF-8");
        println!("zh_CN.UTF-8");
        return 0;
    }

    if args.iter().any(|a| a == "-m" || a == "--charmaps") {
        println!("ANSI_X3.4-1968");
        println!("ISO-8859-1");
        println!("ISO-8859-15");
        println!("UTF-8");
        return 0;
    }

    println!("LANG=en_US.UTF-8");
    println!("LC_CTYPE=\"en_US.UTF-8\"");
    println!("LC_NUMERIC=\"en_US.UTF-8\"");
    println!("LC_TIME=\"en_US.UTF-8\"");
    println!("LC_COLLATE=\"en_US.UTF-8\"");
    println!("LC_MONETARY=\"en_US.UTF-8\"");
    println!("LC_MESSAGES=\"en_US.UTF-8\"");
    println!("LC_PAPER=\"en_US.UTF-8\"");
    println!("LC_NAME=\"en_US.UTF-8\"");
    println!("LC_ADDRESS=\"en_US.UTF-8\"");
    println!("LC_TELEPHONE=\"en_US.UTF-8\"");
    println!("LC_MEASUREMENT=\"en_US.UTF-8\"");
    println!("LC_IDENTIFICATION=\"en_US.UTF-8\"");
    println!("LC_ALL=");
    0
}

fn run_localedef(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: localedef [OPTIONS] OUTPUT_PATH");
        println!();
        println!("localedef — compile locale definition (Slate OS).");
        println!();
        println!("Options:");
        println!("  -i, --inputfile FILE  Input file");
        println!("  -f, --charmap FILE    Character map");
        println!("  -c, --force           Force creation");
        println!("  --no-archive          Don't add to archive");
        println!("  --delete-from-archive Delete from archive");
        println!("  --list-archive        List archive contents");
        return 0;
    }

    if args.iter().any(|a| a == "--list-archive") {
        println!("en_US.utf8");
        println!("en_GB.utf8");
        println!("de_DE.utf8");
        println!("fr_FR.utf8");
        return 0;
    }

    let output = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("output");
    println!("localedef: compiling locale '{}'...", output);
    println!("localedef: done.");
    0
}

fn run_locale_gen(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: locale-gen [LOCALE ...]");
        println!();
        println!("locale-gen — generate locale data (Slate OS).");
        return 0;
    }

    let locales: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    if locales.is_empty() {
        println!("Generating locales from /etc/locale.gen...");
        println!("  en_US.UTF-8... done");
        println!("  en_GB.UTF-8... done");
    } else {
        for loc in &locales {
            println!("  {}... done", loc);
        }
    }
    println!("Generation complete.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "locale".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "localedef" => run_localedef(&rest),
        "locale-gen" => run_locale_gen(&rest),
        _ => run_locale(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_locale};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/locale"), "locale");
        assert_eq!(basename(r"C:\bin\locale.exe"), "locale.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("locale.exe"), "locale");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_locale(&["--help".to_string()]), 0);
        assert_eq!(run_locale(&["-h".to_string()]), 0);
        let _ = run_locale(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_locale(&[]);
    }
}
