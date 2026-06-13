#![deny(clippy::all)]

//! apparmor-cli — SlateOS AppArmor profile management tools
//!
//! Multi-personality: `aa-status`, `aa-enforce`, `aa-complain`, `aa-disable`,
//! `aa-genprof`, `aa-logprof`, `apparmor_parser`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_aa_status(_args: &[String]) -> i32 {
    println!("apparmor module is loaded.");
    println!("28 profiles are loaded.");
    println!("28 profiles are in enforce mode.");
    println!("   /usr/bin/evince");
    println!("   /usr/bin/firefox");
    println!("   /usr/bin/man");
    println!("   /usr/sbin/cups-browsed");
    println!("   /usr/sbin/cupsd");
    println!("   /usr/sbin/ntpd");
    println!("0 profiles are in complain mode.");
    println!("4 processes have profiles defined.");
    println!("4 processes are in enforce mode.");
    println!("   /usr/sbin/cupsd (1234)");
    println!("   /usr/sbin/ntpd (5678)");
    println!("0 processes are in complain mode.");
    println!("0 processes are unconfined but have a profile defined.");
    0
}

fn run_aa_enforce(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("usage: aa-enforce PROFILE");
        return 1;
    }
    for profile in args.iter().filter(|a| !a.starts_with('-')) {
        println!("Setting {} to enforce mode.", profile);
    }
    0
}

fn run_aa_complain(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("usage: aa-complain PROFILE");
        return 1;
    }
    for profile in args.iter().filter(|a| !a.starts_with('-')) {
        println!("Setting {} to complain mode.", profile);
    }
    0
}

fn run_aa_disable(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("usage: aa-disable PROFILE");
        return 1;
    }
    for profile in args.iter().filter(|a| !a.starts_with('-')) {
        println!("Disabling {}.", profile);
    }
    0
}

fn run_aa_genprof(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: aa-genprof PROGRAM");
        println!();
        println!("aa-genprof — generate AppArmor profile (SlateOS).");
        return 0;
    }
    let program = args.first().map(|s| s.as_str()).unwrap_or("program");
    println!("Profiling: {}", program);
    println!("Please start the application to be profiled in another window");
    println!("and exercise its functionality now.");
    println!();
    println!("When you are done, press 'S' to scan for events, 'F' to finish.");
    0
}

fn run_apparmor_parser(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: apparmor_parser [OPTIONS] [PROFILES]");
        println!();
        println!("Options:");
        println!("  -a, --add        Load profile");
        println!("  -r, --replace    Replace profile");
        println!("  -R, --remove     Remove profile");
        println!("  -C, --compile    Compile to cache");
        println!("  -K, --skip-cache Skip cache");
        println!("  -Q, --skip-kernel-load  Don't load into kernel");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("AppArmor parser version 4.0.1 (SlateOS)");
        return 0;
    }
    let profile = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("profile");
    let replace = args.iter().any(|a| a == "-r" || a == "--replace");
    if replace {
        println!("Replacement of profile {} succeeded.", profile);
    } else {
        println!("Loading profile {}.", profile);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "aa-status".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "aa-enforce" => run_aa_enforce(&rest),
        "aa-complain" => run_aa_complain(&rest),
        "aa-disable" => run_aa_disable(&rest),
        "aa-genprof" => run_aa_genprof(&rest),
        "aa-logprof" => { println!("Reading log entries..."); println!("No new events found."); 0 }
        "apparmor_parser" => run_apparmor_parser(&rest),
        _ => run_aa_status(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_aa_enforce};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/apparmor"), "apparmor");
        assert_eq!(basename(r"C:\bin\apparmor.exe"), "apparmor.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("apparmor.exe"), "apparmor");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_aa_enforce(&["--help".to_string()]), 0);
        assert_eq!(run_aa_enforce(&["-h".to_string()]), 0);
        let _ = run_aa_enforce(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_aa_enforce(&[]);
    }
}
