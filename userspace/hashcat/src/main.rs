#![deny(clippy::all)]

//! hashcat — SlateOS password recovery tool
//!
//! Single personality: `hashcat`

use std::env;
use std::process;

fn run_hashcat(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hashcat [options] hashfile [mask|wordfiles|directories]");
        println!();
        println!("Options:");
        println!("  -m, --hash-type <n>      Hash type (e.g., 0=MD5, 100=SHA1, 1400=SHA256)");
        println!("  -a, --attack-mode <n>    Attack mode (0=dict, 1=combo, 3=brute, 6=hybrid, 7=hybrid)");
        println!("  -o, --outfile <file>     Output file for recovered hashes");
        println!("  --show                   Show cracked passwords");
        println!("  --left                   Show uncracked hashes");
        println!("  -r, --rules-file <file>  Rules file");
        println!("  -w, --workload-profile   Workload profile (1=low, 2=default, 3=high, 4=nightmare)");
        println!("  -O, --optimized-kernel   Enable optimized kernels");
        println!("  --force                  Ignore warnings");
        println!("  --session <name>         Session name");
        println!("  --restore                Restore session");
        println!("  --benchmark              Benchmark mode");
        println!("  -I, --backend-info       Show backend info (GPU/CPU)");
        println!("  --version                Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("hashcat (v6.2.6) starting in version mode");
        println!();
        println!("hashcat (v6.2.6) (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "--benchmark") {
        println!("hashcat (v6.2.6) starting in benchmark mode");
        println!();
        println!("Benchmarking uses all-zeros hash and all-zeros input.");
        println!();
        println!("Hashmode: 0 - MD5");
        println!("Speed.#1.........: 12345.6 MH/s (67.89ms) @ Accel:1024 Loops:1024 Thr:256 Vec:1");
        println!();
        println!("Hashmode: 100 - SHA1");
        println!("Speed.#1.........:  4567.8 MH/s (89.01ms) @ Accel:512 Loops:512 Thr:256 Vec:1");
        println!();
        println!("Hashmode: 1400 - SHA2-256");
        println!("Speed.#1.........:  1234.5 MH/s (90.12ms) @ Accel:256 Loops:256 Thr:256 Vec:1");
        return 0;
    }
    if args.iter().any(|a| a == "-I" || a == "--backend-info") {
        println!("hashcat (v6.2.6) starting in backend information mode");
        println!();
        println!("Backend Device #1");
        println!("  Type...........: CPU");
        println!("  Vendor.ID......: 1");
        println!("  Vendor.........: Slate OS");
        println!("  Name...........: CPU (simulated)");
        println!("  Processor(s)...: 8");
        return 0;
    }
    if args.iter().any(|a| a == "--show") {
        println!("5f4dcc3b5aa765d61d8327deb882cf99:password");
        println!("e10adc3949ba59abbe56e057f20f883e:123456");
        return 0;
    }

    println!("hashcat (v6.2.6) starting (Slate OS)");
    println!();
    println!("Session..........: hashcat");
    println!("Status...........: Running");
    println!("Hash.Mode........: 0 (MD5)");
    println!("Hash.Target......: (simulated)");
    println!("Time.Started.....: Thu May 22 10:00:00 2025");
    println!("Speed.#1.........:   123.4 MH/s");
    println!("Recovered........: 1/2 (50.00%)");
    println!("Progress.........: 1234567/9999999 (12.35%)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hashcat(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_hashcat};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_hashcat(vec!["--help".to_string()]), 0);
        assert_eq!(run_hashcat(vec!["-h".to_string()]), 0);
        let _ = run_hashcat(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_hashcat(vec![]);
    }
}
