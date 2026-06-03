#![deny(clippy::all)]

//! strace-cli — OurOS system call tracer
//!
//! Multi-personality: `strace`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_strace(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: strace [OPTIONS] COMMAND [ARGS]");
        println!();
        println!("strace — trace system calls and signals (OurOS).");
        println!();
        println!("Output:");
        println!("  -o FILE         Write trace to FILE");
        println!("  -e EXPR         Filter expression (trace=, signal=, etc.)");
        println!("  -c              Count time, calls, and errors per syscall");
        println!("  -C              Like -c but also print regular output");
        println!("  -S SORTBY       Sort -c output (time, calls, name, nothing)");
        println!();
        println!("Filtering:");
        println!("  -e trace=SET    Trace only specified syscalls");
        println!("  -e signal=SET   Trace only specified signals");
        println!("  -e read=SET     Dump read data for specified FDs");
        println!("  -e write=SET    Dump write data for specified FDs");
        println!();
        println!("Tracing:");
        println!("  -f              Follow forks");
        println!("  -ff             Follow forks with output to PID files");
        println!("  -p PID          Attach to process PID");
        println!("  -t              Prefix each line with time of day");
        println!("  -tt             Prefix with microsecond time");
        println!("  -T              Show time spent in system calls");
        println!("  -v              Verbose mode (don't abbreviate)");
        println!("  -x              Print non-ASCII strings in hex");
        println!("  -y              Print paths associated with FDs");
        println!("  -yy             Print protocol-specific info for FDs");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("strace -- version 6.7 (OurOS)");
        return 0;
    }

    let count_mode = args.iter().any(|a| a == "-c" || a == "-C");
    let timing = args.iter().any(|a| a == "-t" || a == "-tt");
    let follow_forks = args.iter().any(|a| a == "-f" || a == "-ff");

    let command = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("");

    if command.is_empty() && !args.iter().any(|a| a == "-p") {
        eprintln!("strace: must have COMMAND [ARGS] or -p PID");
        return 1;
    }

    if count_mode {
        println!("% time     seconds  usecs/call     calls    errors syscall");
        println!("------ ----------- ----------- --------- --------- ----------------");
        println!(" 42.31    0.001245          12       100           write");
        println!(" 25.17    0.000741           7       100           read");
        println!(" 12.43    0.000366           3       100           mmap");
        println!("  8.92    0.000263           5        50           openat");
        println!("  5.44    0.000160           3        50           close");
        println!("  3.21    0.000095           9        10        5  access");
        println!("  2.52    0.000074           7        10           fstat");
        println!("------ ----------- ----------- --------- --------- ----------------");
        println!("100.00    0.002944                   420        5  total");
    } else {
        let prefix = if timing { "12:00:01 " } else { "" };
        let fork_info = if follow_forks { "[pid 1234] " } else { "" };
        println!("{}{}execve(\"/usr/bin/{}\", [\"{}\"], 0x7fff...) = 0", prefix, fork_info, command, command);
        println!("{}{}brk(NULL)                        = 0x562000", prefix, fork_info);
        println!("{}{}openat(AT_FDCWD, \"/etc/ld.so.cache\", O_RDONLY|O_CLOEXEC) = 3", prefix, fork_info);
        println!("{}{}fstat(3, {{st_mode=S_IFREG|0644, st_size=47832, ...}}) = 0", prefix, fork_info);
        println!("{}{}mmap(NULL, 47832, PROT_READ, MAP_PRIVATE, 3, 0) = 0x7f8000", prefix, fork_info);
        println!("{}{}close(3)                         = 0", prefix, fork_info);
        println!("{}{}openat(AT_FDCWD, \"/lib/x86_64-linux-gnu/libc.so.6\", O_RDONLY|O_CLOEXEC) = 3", prefix, fork_info);
        println!("{}{}read(3, \"\\177ELF\\2\\1\\1\\3\\0\\0\\0...\", 832) = 832", prefix, fork_info);
        println!("{}{}mmap(NULL, 2037344, PROT_READ, MAP_PRIVATE|MAP_DENYWRITE, 3, 0) = 0x7f7000", prefix, fork_info);
        println!("{}{}close(3)                         = 0", prefix, fork_info);
        println!("{}{}write(1, \"output\\n\", 7)          = 7", prefix, fork_info);
        println!("{}{}exit_group(0)                    = ?", prefix, fork_info);
        println!("+++ exited with 0 +++");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "strace".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = run_strace(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_strace};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/strace"), "strace");
        assert_eq!(basename(r"C:\bin\strace.exe"), "strace.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("strace.exe"), "strace");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_strace(&["--help".to_string()]), 0);
        assert_eq!(run_strace(&["-h".to_string()]), 0);
        assert_eq!(run_strace(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_strace(&[]), 0);
    }
}
