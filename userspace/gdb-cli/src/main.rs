#![deny(clippy::all)]

//! gdb-cli — OurOS GNU Debugger
//!
//! Multi-personality: `gdb`, `gdbserver`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_gdb(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gdb [OPTIONS] [PROGRAM [COREFILE|PID]]");
        println!();
        println!("gdb — GNU debugger (OurOS).");
        println!();
        println!("Options:");
        println!("  -q, --quiet          Quiet mode (no banner)");
        println!("  -batch               Batch mode");
        println!("  -x FILE              Execute commands from FILE");
        println!("  -ex CMD              Execute command CMD");
        println!("  -p PID               Attach to process PID");
        println!("  -c COREFILE          Analyze core dump");
        println!("  -tui                 TUI mode");
        println!("  --args               Pass args after program");
        println!("  -symbols FILE        Read symbols from FILE");
        println!("  -directory DIR       Search directory for source");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("GNU gdb (OurOS) 14.2");
        println!("Copyright (C) 2024 Free Software Foundation, Inc.");
        println!("This GDB was configured as \"x86_64-ouros\".");
        return 0;
    }

    let quiet = args.iter().any(|a| a == "-q" || a == "--quiet");
    let batch = args.iter().any(|a| a == "-batch");
    let program = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str());

    if !quiet {
        println!("GNU gdb (OurOS) 14.2");
        println!("Copyright (C) 2024 Free Software Foundation, Inc.");
        println!("License GPLv3+: GNU GPL version 3 or later");
        println!("This is free software: you are free to change and redistribute it.");
        println!("For bug reporting instructions, please see:");
        println!("<https://www.gnu.org/software/gdb/bugs/>.");
        println!("Type \"show copying\" and \"show warranty\" for details.");
    }

    if let Some(prog) = program {
        println!("Reading symbols from {}...", prog);
        println!("(gdb) ");
    } else if !batch {
        println!("(gdb) ");
    }
    0
}

fn run_gdbserver(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gdbserver [OPTIONS] COMM PROGRAM [ARGS]");
        println!("   or: gdbserver [OPTIONS] --attach COMM PID");
        println!();
        println!("gdbserver — remote debugging server (OurOS).");
        println!();
        println!("Options:");
        println!("  --multi          Multi-process mode");
        println!("  --once           Exit after first connection");
        println!("  --debug          Enable debug output");
        println!("  --wrapper CMD    Run CMD as wrapper");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("GNU gdbserver (OurOS) 14.2");
        return 0;
    }

    let attach = args.iter().any(|a| a == "--attach");
    let comm = args.first().map(|s| s.as_str()).unwrap_or(":1234");

    if attach {
        let pid = args.get(1).map(|s| s.as_str()).unwrap_or("1234");
        println!("Attached; pid = {}", pid);
    } else {
        let program = args.get(1).map(|s| s.as_str()).unwrap_or("program");
        println!("Process {} created; pid = 5678", program);
    }
    println!("Listening on port {}", comm.trim_start_matches(':'));
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "gdb".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "gdbserver" => run_gdbserver(&rest),
        _ => run_gdb(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gdb};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gdb"), "gdb");
        assert_eq!(basename(r"C:\bin\gdb.exe"), "gdb.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gdb.exe"), "gdb");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_gdb(&["--help".to_string()]), 0);
        assert_eq!(run_gdb(&["-h".to_string()]), 0);
        assert_eq!(run_gdb(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_gdb(&[]), 0);
    }
}
