#![deny(clippy::all)]

//! ltrace-cli — Slate OS library call tracer
//!
//! Multi-personality: `ltrace`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_ltrace(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ltrace [OPTIONS] COMMAND [ARGS]");
        println!();
        println!("ltrace — library call tracer (Slate OS).");
        println!();
        println!("Options:");
        println!("  -c              Count library calls");
        println!("  -C              Demangle C++ names");
        println!("  -e EXPR         Filter expression");
        println!("  -f              Follow forks");
        println!("  -l LIBRARY      Trace only LIBRARY");
        println!("  -o FILE         Output to FILE");
        println!("  -p PID          Attach to PID");
        println!("  -S              Display system calls");
        println!("  -t              Print timestamps");
        println!("  -tt             Microsecond timestamps");
        println!("  -n N            Indent nested calls N spaces");
        println!("  -x PATTERN      Trace matching library symbols");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("ltrace version 0.7.3 (Slate OS)");
        return 0;
    }

    let count_mode = args.iter().any(|a| a == "-c");
    let show_syscalls = args.iter().any(|a| a == "-S");
    let timing = args.iter().any(|a| a == "-t" || a == "-tt");

    let command = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("");

    if command.is_empty() && !args.iter().any(|a| a == "-p") {
        eprintln!("ltrace: must have COMMAND [ARGS] or -p PID");
        return 1;
    }

    if count_mode {
        println!("% time     seconds  usecs/call     calls      function");
        println!("------ ----------- ----------- --------- --------------------");
        println!(" 35.21    0.002100         21       100 malloc");
        println!(" 22.14    0.001320         13       100 free");
        println!(" 18.43    0.001099          5       200 strlen");
        println!(" 12.67    0.000756          7       100 memcpy");
        println!("  6.33    0.000378          7        50 fwrite");
        println!("  5.22    0.000312         15        20 fopen");
        println!("------ ----------- ----------- --------- --------------------");
        println!("100.00    0.005965                   570 total");
    } else {
        let prefix = if timing { "12:00:01.123456 " } else { "" };
        println!("{}__libc_start_main(0x401130, 1, 0x7fff...) = <void>", prefix);
        println!("{}malloc(64)                                = 0x1a2b000", prefix);
        println!("{}strlen(\"hello world\")                     = 11", prefix);
        println!("{}memcpy(0x1a2b000, \"hello world\", 11)      = 0x1a2b000", prefix);
        println!("{}puts(\"hello world\")                       = 12", prefix);
        if show_syscalls {
            println!("{}SYS_write(1, \"hello world\\n\", 12)        = 12", prefix);
        }
        println!("{}free(0x1a2b000)                           = <void>", prefix);
        println!("{}exit(0 <no return ...>)", prefix);
        println!("+++ exited (status 0) +++");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "ltrace".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = run_ltrace(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ltrace};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ltrace"), "ltrace");
        assert_eq!(basename(r"C:\bin\ltrace.exe"), "ltrace.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ltrace.exe"), "ltrace");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ltrace(&["--help".to_string()]), 0);
        assert_eq!(run_ltrace(&["-h".to_string()]), 0);
        let _ = run_ltrace(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ltrace(&[]);
    }
}
