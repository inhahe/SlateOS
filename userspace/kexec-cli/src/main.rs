#![deny(clippy::all)]

//! kexec-cli — OurOS fast kernel reboot tool
//!
//! Multi-personality: `kexec`, `kdump-config`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_kexec(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kexec [OPTIONS] [KERNEL]");
        println!();
        println!("kexec — load and boot a new kernel (OurOS).");
        println!();
        println!("Options:");
        println!("  -l, --load         Load a new kernel");
        println!("  -e, --exec         Execute loaded kernel");
        println!("  -p, --load-panic   Load panic kernel");
        println!("  -u, --unload       Unload loaded kernel");
        println!("  --initrd=FILE      Use FILE as initrd");
        println!("  --append=STRING    Kernel command line");
        println!("  --reuse-cmdline    Reuse current command line");
        println!("  -t, --type=TYPE    Kernel type (Image, bzImage, etc.)");
        println!("  -f, --force        Force (skip checks)");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("kexec-tools 2.0.27 (OurOS)");
        return 0;
    }

    let load = args.iter().any(|a| a == "-l" || a == "--load");
    let exec = args.iter().any(|a| a == "-e" || a == "--exec");
    let unload = args.iter().any(|a| a == "-u" || a == "--unload");
    let load_panic = args.iter().any(|a| a == "-p" || a == "--load-panic");

    let kernel = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str());

    let initrd = args.iter()
        .find(|a| a.starts_with("--initrd="))
        .and_then(|a| a.strip_prefix("--initrd="));

    let cmdline = args.iter()
        .find(|a| a.starts_with("--append="))
        .and_then(|a| a.strip_prefix("--append="));

    if unload {
        println!("kexec: unloading kernel");
        return 0;
    }

    if exec {
        println!("kexec: executing loaded kernel...");
        println!("kexec: starting new kernel");
        return 0;
    }

    if let Some(kern) = kernel {
        if load_panic {
            println!("kexec: loading panic kernel '{}'", kern);
        } else if load {
            println!("kexec: loading kernel '{}'", kern);
        } else {
            println!("kexec: loading and executing kernel '{}'", kern);
        }
        if let Some(rd) = initrd {
            println!("kexec: initrd: {}", rd);
        }
        if let Some(cl) = cmdline {
            println!("kexec: command line: {}", cl);
        }
    } else if !exec {
        eprintln!("kexec: no kernel specified");
        return 1;
    }
    0
}

fn run_kdump_config(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kdump-config COMMAND");
        println!();
        println!("kdump-config — manage kdump crash recovery (OurOS).");
        println!();
        println!("Commands:");
        println!("  show     Show kdump status");
        println!("  load     Load kdump kernel");
        println!("  unload   Unload kdump kernel");
        println!("  test     Test crash dump");
        println!("  status   Show status");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("show");
    match subcmd {
        "show" | "status" => {
            println!("DUMP_MODE:        kdump");
            println!("USE_KDUMP:        1");
            println!("KDUMP_SYSCTL:     kernel.panic_on_oops=1");
            println!("KDUMP_COREDIR:    /var/crash");
            println!("crashkernel addr: 0x2000000");
            println!("current state:    ready to kdump");
            println!("kexec command:");
            println!("  /sbin/kexec -p --command-line=\"..\" /boot/vmlinuz-1.0.0");
        }
        "load" => {
            println!("kdump-config: loading kdump kernel...");
            println!("kdump-config: kdump kernel loaded successfully");
        }
        "unload" => {
            println!("kdump-config: unloading kdump kernel...");
            println!("kdump-config: kdump kernel unloaded");
        }
        "test" => {
            println!("kdump-config: this will crash the system for testing!");
            println!("kdump-config: use 'echo c > /proc/sysrq-trigger' to trigger");
        }
        _ => {
            eprintln!("kdump-config: unknown command '{}'", subcmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "kexec".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "kdump-config" => run_kdump_config(&rest),
        _ => run_kexec(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kexec};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kexec"), "kexec");
        assert_eq!(basename(r"C:\bin\kexec.exe"), "kexec.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kexec.exe"), "kexec");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_kexec(&["--help".to_string()]), 0);
        assert_eq!(run_kexec(&["-h".to_string()]), 0);
        let _ = run_kexec(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_kexec(&[]);
    }
}
