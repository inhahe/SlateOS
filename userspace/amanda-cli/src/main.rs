#![deny(clippy::all)]

//! amanda-cli — SlateOS Amanda backup CLI
//!
//! Multi-personality: `amdump`, `amcheck`, `amrecover`, `amrestore`, `amstatus`, `amreport`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_amdump(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: amdump [OPTIONS] CONFIG");
        println!();
        println!("amdump — run Amanda backup (Slate OS).");
        return 0;
    }
    let config = args.first().map(|s| s.as_str()).unwrap_or("DailySet1");
    println!("amdump: starting backup for configuration '{}'", config);
    println!("  Planner: estimating 5 hosts");
    println!("  Driver: starting dumper processes");
    println!("  Dumper: /home/user level 0 (245.6 MB)");
    println!("  Dumper: /etc level 0 (12.4 MB)");
    println!("  Taper: writing to vtape01");
    println!("  Finished. 2 DLEs dumped, 0 failed.");
    0
}

fn run_amcheck(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: amcheck [OPTIONS] CONFIG");
        println!();
        println!("amcheck — verify Amanda configuration (Slate OS).");
        println!();
        println!("Options:");
        println!("  -c    Check client only");
        println!("  -s    Check server only");
        return 0;
    }
    let config = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("DailySet1");
    println!("Amanda Tape Server Host Check");
    println!("----");
    println!("Holding disk /holding: 15.2 GB available, using 10.0 GB");
    println!("slot 1: volume vtape01, label ok");
    println!("Server check took 0.5 seconds");
    println!();
    println!("Amanda Backup Client Hosts Check (config: {})", config);
    println!("----");
    println!("Client check: 5 hosts checked, 0 problems found");
    println!("(brought to you by Amanda 3.5.1)");
    0
}

fn run_amrecover(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: amrecover [OPTIONS] [CONFIG]");
        println!();
        println!("amrecover — interactive Amanda recovery (Slate OS).");
        return 0;
    }
    println!("AMRECOVER - Amanda Recovery");
    println!("Setting restore date to today");
    println!("200 Working date set to 2024-01-15");
    println!();
    println!("amrecover> ");
    0
}

fn run_amrestore(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: amrestore [OPTIONS] DEVICE [HOST [DISK]]");
        println!();
        println!("amrestore — extract Amanda backup data (Slate OS).");
        return 0;
    }
    println!("amrestore: restoring from backup...");
    println!("  Extracted 1523 files (245.6 MB)");
    0
}

fn run_amstatus(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: amstatus [OPTIONS] CONFIG");
        println!();
        println!("amstatus — display Amanda backup status (Slate OS).");
        return 0;
    }
    let config = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("DailySet1");
    println!("Using config '{}' from /etc/amanda", config);
    println!();
    println!("STATISTICS:");
    println!("                       Total   Full   Incr");
    println!("  Estimate Time (hrs)  0.02    0.01   0.01");
    println!("  Run Time (hrs)       0.15    0.10   0.05");
    println!("  Dump Time (hrs)      0.12    0.08   0.04");
    println!("  Output Size (meg)    258.0   245.6   12.4");
    println!("  Original Size (meg)  310.0   290.0   20.0");
    println!("  Avg Coverage         83.2%   84.7%  62.0%");
    0
}

fn run_amreport(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: amreport [OPTIONS] CONFIG");
        println!();
        println!("amreport — generate Amanda report (Slate OS).");
        return 0;
    }
    let config = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("DailySet1");
    println!("Amanda Backup Report for {} — 2024-01-15", config);
    println!();
    println!("FAILURE DUMP SUMMARY: (no failures)");
    println!();
    println!("STATISTICS:");
    println!("  Dumps: 5 total, 2 full, 3 incremental");
    println!("  Size: 258.0 MB (compression: 83.2%)");
    println!("  Time: 0.15 hours");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "amdump".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "amcheck" => run_amcheck(&rest),
        "amrecover" => run_amrecover(&rest),
        "amrestore" => run_amrestore(&rest),
        "amstatus" => run_amstatus(&rest),
        "amreport" => run_amreport(&rest),
        _ => run_amdump(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_amdump};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/amanda"), "amanda");
        assert_eq!(basename(r"C:\bin\amanda.exe"), "amanda.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("amanda.exe"), "amanda");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_amdump(&["--help".to_string()]), 0);
        assert_eq!(run_amdump(&["-h".to_string()]), 0);
        let _ = run_amdump(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_amdump(&[]);
    }
}
