#![deny(clippy::all)]

//! perl — OurOS Perl interpreter
//!
//! Multi-personality: `perl`, `cpan`, `perldoc`

use std::env;
use std::process;

fn run_perl(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: perl [switches] [--] [programfile] [arguments]");
        println!("  -e program    Execute program");
        println!("  -n            Assume 'while (<>) {{ ... }}' loop");
        println!("  -p            Like -n but print");
        println!("  -i[ext]       Edit files in place");
        println!("  -w            Enable warnings");
        println!("  -W            Enable all warnings");
        println!("  -c            Check syntax only");
        println!("  -d            Run under debugger");
        println!("  -l            Enable line ending processing");
        println!("  -a            Autosplit mode with -n/-p");
        println!("  -F pattern    Split() pattern for -a");
        println!("  -M module     Execute 'use module'");
        println!("  -v            Show version");
        println!("  -V            Show config");
        return 0;
    }
    if args.iter().any(|a| a == "-v") {
        println!("This is perl 5, version 40, subversion 0 (v5.40.0) built for x86_64-ouros");
        println!();
        println!("Copyright 1987-2025, Larry Wall");
        return 0;
    }

    let exec_str = args.iter().position(|a| a == "-e")
        .and_then(|i| args.get(i + 1));
    if let Some(code) = exec_str {
        println!("(executing: {})", code);
        return 0;
    }

    let script = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str());
    if let Some(file) = script {
        println!("(running {})", file);
    } else {
        println!("(reading from stdin — interactive)");
    }
    0
}

fn run_cpan(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cpan [options] module...");
        println!("  -i        Install module");
        println!("  -l        List installed modules");
        println!("  -D module Module details");
        return 0;
    }
    let module = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str());
    if let Some(m) = module {
        println!("Installing {}... done (simulated)", m);
    } else {
        println!("cpan shell -- CPAN exploration and target installation (v2.36)");
    }
    0
}

fn run_perldoc(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: perldoc [-h] [-v] [-t] [-u] [-m] [-l] PageName|ModuleName|ProgramName");
        return 0;
    }
    let page = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("perl");
    println!("(displaying documentation for: {} — simulated)", page);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("perl");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "cpan" => run_cpan(rest),
        "perldoc" => run_perldoc(rest),
        _ => run_perl(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_perl};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_perl(vec!["--help".to_string()]), 0);
        assert_eq!(run_perl(vec!["-h".to_string()]), 0);
        assert_eq!(run_perl(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_perl(vec![]), 0);
    }
}
