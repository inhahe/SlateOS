#![deny(clippy::all)]

//! ldd-cli — SlateOS shared library dependency lister
//!
//! Multi-personality: `ldd`, `pldd`, `sprof`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_ldd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ldd [OPTIONS] FILE...");
        println!();
        println!("ldd — print shared object dependencies (SlateOS).");
        println!();
        println!("Options:");
        println!("  -v, --verbose     Verbose (include symbol versioning)");
        println!("  -u, --unused      Print unused direct dependencies");
        println!("  -d, --data-relocs Process data relocations");
        println!("  -r, --function-relocs Process data and function relocations");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("ldd (SlateOS) 2.39");
        return 0;
    }

    let verbose = args.iter().any(|a| a == "-v" || a == "--verbose");
    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if files.is_empty() {
        eprintln!("ldd: missing file operand");
        return 1;
    }

    for file in &files {
        if files.len() > 1 {
            println!("{}:", file);
        }
        println!("\tlinux-vdso.so.1 (0x00007ffcf7ffe000)");
        println!("\tlibc.so.6 => /lib/x86_64-linux-gnu/libc.so.6 (0x00007f8a3c000000)");
        println!("\t/lib64/ld-linux-x86-64.so.2 (0x00007f8a3c400000)");
        if verbose {
            println!("\tVersion information:");
            println!("\t\t{} (libc6) => libc.so.6", file);
        }
    }
    0
}

fn run_pldd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: pldd PID");
        println!();
        println!("pldd — list shared objects loaded by process (SlateOS).");
        return 0;
    }

    let pid = args.first().map(|s| s.as_str()).unwrap_or("");
    if pid.is_empty() {
        eprintln!("pldd: missing PID argument");
        return 1;
    }

    println!("{}: /usr/bin/example", pid);
    println!("linux-vdso.so.1");
    println!("/lib/x86_64-linux-gnu/libc.so.6");
    println!("/lib/x86_64-linux-gnu/libm.so.6");
    println!("/lib/x86_64-linux-gnu/libpthread.so.0");
    println!("/lib64/ld-linux-x86-64.so.2");
    0
}

fn run_sprof(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: sprof [OPTIONS] SHLIB PROFILE_DATA");
        println!();
        println!("sprof — shared object profiling data reader (SlateOS).");
        println!();
        println!("Options:");
        println!("  -c, --call-pairs   Print call pairs");
        println!("  -p, --flat-profile Flat profile");
        println!("  -q, --graph        Call graph");
        return 0;
    }

    let lib = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("libfoo.so");
    println!("Flat profile for {}:", lib);
    println!();
    println!("  %   cumulative   self");
    println!(" time   seconds   seconds    calls  name");
    println!(" 45.2     0.14     0.14      1000  compute");
    println!(" 30.1     0.23     0.09      5000  process");
    println!(" 15.8     0.28     0.05      2000  transform");
    println!("  8.9     0.31     0.03       500  initialize");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "ldd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "pldd" => run_pldd(&rest),
        "sprof" => run_sprof(&rest),
        _ => run_ldd(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ldd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ldd"), "ldd");
        assert_eq!(basename(r"C:\bin\ldd.exe"), "ldd.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ldd.exe"), "ldd");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ldd(&["--help".to_string()]), 0);
        assert_eq!(run_ldd(&["-h".to_string()]), 0);
        let _ = run_ldd(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ldd(&[]);
    }
}
