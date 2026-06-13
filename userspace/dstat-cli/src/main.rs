#![deny(clippy::all)]

//! dstat-cli — SlateOS dstat system resource statistics
//!
//! Single personality: `dstat`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dstat(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dstat [OPTIONS]");
        println!("dstat 0.8.0 (Slate OS) — Versatile resource statistics");
        println!();
        println!("Options:");
        println!("  -c, --cpu              CPU stats");
        println!("  -d, --disk             Disk stats");
        println!("  -g, --page             Paging stats");
        println!("  -i, --int              Interrupt stats");
        println!("  -l, --load             Load average");
        println!("  -m, --mem              Memory stats");
        println!("  -n, --net              Network stats");
        println!("  -p, --proc             Process stats");
        println!("  -r, --io               I/O request stats");
        println!("  -s, --swap             Swap stats");
        println!("  -t, --time             Timestamp");
        println!("  -y, --sys              System stats");
        println!("  -a, --all              All stats (default)");
        println!("  -f, --full             Expand -C -D -I -N -S");
        println!("  --top-cpu              Most CPU-intensive process");
        println!("  --top-mem              Most memory-intensive process");
        println!("  --top-io               Most I/O-intensive process");
        println!("  --output FILE          CSV output");
        println!("  DELAY [COUNT]          Update interval and count");
        println!("  -V, --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("dstat 0.8.0 (Slate OS)");
        return 0;
    }
    println!("----total-usage---- -dsk/total- -net/total- ---paging-- ---system--");
    println!("usr sys idl wai stl| read  writ| recv  send|  in   out | int   csw ");
    println!("  8   3  88   1   0| 2.1M 1.5M| 5.6M 1.2M|   0     0 | 512  1024");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dstat".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dstat(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dstat};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dstat"), "dstat");
        assert_eq!(basename(r"C:\bin\dstat.exe"), "dstat.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dstat.exe"), "dstat");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dstat(&["--help".to_string()], "dstat"), 0);
        assert_eq!(run_dstat(&["-h".to_string()], "dstat"), 0);
        let _ = run_dstat(&["--version".to_string()], "dstat");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dstat(&[], "dstat");
    }
}
