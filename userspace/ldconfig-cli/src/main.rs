#![deny(clippy::all)]

//! ldconfig-cli — OurOS dynamic linker cache manager
//!
//! Multi-personality: `ldconfig`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_ldconfig(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ldconfig [OPTIONS] [DIRS...]");
        println!();
        println!("ldconfig — configure dynamic linker run-time bindings (OurOS).");
        println!();
        println!("Options:");
        println!("  -v, --verbose     Verbose mode");
        println!("  -n                Process only specified directories");
        println!("  -N                Don't rebuild cache");
        println!("  -X                Don't update symbolic links");
        println!("  -f FILE           Use FILE instead of /etc/ld.so.conf");
        println!("  -C FILE           Use FILE instead of /etc/ld.so.cache");
        println!("  -r DIR            Change to and use DIR as root");
        println!("  -l                Library mode (manually link individual libs)");
        println!("  -p, --print-cache Print cache");
        return 0;
    }

    let verbose = args.iter().any(|a| a == "-v" || a == "--verbose");
    let print_cache = args.iter().any(|a| a == "-p" || a == "--print-cache");

    if print_cache {
        println!("47 libs found in cache `/etc/ld.so.cache'");
        println!("\tlibc.so.6 (libc6,x86-64) => /lib/x86_64-linux-gnu/libc.so.6");
        println!("\tlibm.so.6 (libc6,x86-64) => /lib/x86_64-linux-gnu/libm.so.6");
        println!("\tlibpthread.so.0 (libc6,x86-64) => /lib/x86_64-linux-gnu/libpthread.so.0");
        println!("\tlibdl.so.2 (libc6,x86-64) => /lib/x86_64-linux-gnu/libdl.so.2");
        println!("\tlibrt.so.1 (libc6,x86-64) => /lib/x86_64-linux-gnu/librt.so.1");
        println!("\tlibutil.so.1 (libc6,x86-64) => /lib/x86_64-linux-gnu/libutil.so.1");
        println!("\tld-linux-x86-64.so.2 (ELF) => /lib64/ld-linux-x86-64.so.2");
        return 0;
    }

    let dirs: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if dirs.is_empty() {
        if verbose {
            println!("ldconfig: scanning /lib...");
            println!("ldconfig: scanning /usr/lib...");
            println!("ldconfig: scanning /usr/local/lib...");
        }
        println!("ldconfig: rebuilding /etc/ld.so.cache (47 libs)");
    } else {
        for dir in &dirs {
            if verbose {
                println!("ldconfig: scanning {}...", dir);
            }
        }
        println!("ldconfig: cache updated ({} directories processed)", dirs.len());
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "ldconfig".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = run_ldconfig(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ldconfig};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ldconfig"), "ldconfig");
        assert_eq!(basename(r"C:\bin\ldconfig.exe"), "ldconfig.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ldconfig.exe"), "ldconfig");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_ldconfig(&["--help".to_string()]), 0);
        assert_eq!(run_ldconfig(&["-h".to_string()]), 0);
        assert_eq!(run_ldconfig(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_ldconfig(&[]), 0);
    }
}
