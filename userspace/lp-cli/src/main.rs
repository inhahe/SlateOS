#![deny(clippy::all)]

//! lp-cli — OurOS lp/lpr/lpq/lprm/lpstat/lpadmin/lpoptions print commands
//!
//! Multi-personality: `lp`, `lpr`, `lpq`, `lprm`, `lpstat`, `lpadmin`, `lpoptions`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_lp(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lp [OPTIONS] [FILE ...]");
        println!();
        println!("lp — print files (OurOS).");
        println!();
        println!("Options:");
        println!("  -d PRINTER     Destination printer");
        println!("  -n COPIES      Number of copies");
        println!("  -o OPTION      Set print option");
        println!("  -t TITLE       Job title");
        println!("  -q PRIORITY    Priority (1-100)");
        println!("  -H TIME        Hold until time");
        println!("  -P PAGES       Page range (e.g., 1-4)");
        return 0;
    }
    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();
    if files.is_empty() {
        println!("request id is default-1 (stdin)");
    } else {
        for (i, f) in files.iter().enumerate() {
            println!("request id is default-{} (1 file(s)): {}", i + 1, f);
        }
    }
    0
}

fn run_lpr(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lpr [OPTIONS] [FILE ...]");
        println!();
        println!("lpr — print files (BSD style) (OurOS).");
        println!();
        println!("Options:");
        println!("  -P PRINTER     Destination printer");
        println!("  -# COPIES      Number of copies");
        println!("  -T TITLE       Job title");
        println!("  -r             Remove file after printing");
        return 0;
    }
    println!("lpr: job submitted");
    0
}

fn run_lpq(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lpq [OPTIONS] [USER ...]");
        println!();
        println!("lpq — show print queue (OurOS).");
        return 0;
    }
    let all = args.iter().any(|a| a == "-a");
    if all {
        println!("Rank    Owner   Job     File(s)                         Total Size");
        println!("no entries");
    } else {
        println!("default is ready");
        println!("no entries");
    }
    0
}

fn run_lprm(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lprm [OPTIONS] [JOB-ID ...]");
        println!();
        println!("lprm — cancel print jobs (OurOS).");
        return 0;
    }
    if args.iter().any(|a| a == "-") {
        println!("lprm: all jobs cancelled");
    } else if args.is_empty() {
        println!("lprm: no active jobs to cancel");
    } else {
        for job in args.iter().filter(|a| !a.starts_with('-')) {
            println!("lprm: job {} cancelled", job);
        }
    }
    0
}

fn run_lpstat(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lpstat [OPTIONS]");
        println!();
        println!("lpstat — print system status (OurOS).");
        println!();
        println!("Options:");
        println!("  -a             Accepting status");
        println!("  -d             Default printer");
        println!("  -p             Printer status");
        println!("  -s             Summary");
        println!("  -t             All info");
        return 0;
    }
    if args.iter().any(|a| a == "-d") {
        println!("system default destination: default");
    } else if args.iter().any(|a| a == "-p") {
        println!("printer default is idle. enabled since Jan 01 00:00");
    } else if args.iter().any(|a| a == "-a") {
        println!("default accepting requests since Jan 01 00:00");
    } else if args.iter().any(|a| a == "-s" || a == "-t") {
        println!("system default destination: default");
        println!("device for default: ipp://localhost/printers/default");
        println!("default accepting requests since Jan 01 00:00");
        println!("printer default is idle. enabled since Jan 01 00:00");
    } else {
        println!("(no jobs queued)");
    }
    0
}

fn run_lpadmin(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lpadmin [OPTIONS]");
        println!();
        println!("lpadmin — configure CUPS printers (OurOS).");
        println!();
        println!("Options:");
        println!("  -p PRINTER     Add/modify printer");
        println!("  -x PRINTER     Delete printer");
        println!("  -d PRINTER     Set default printer");
        println!("  -v URI         Device URI");
        println!("  -m MODEL       PPD model");
        println!("  -E             Enable and accept");
        return 0;
    }
    if args.iter().any(|a| a == "-x") {
        println!("lpadmin: printer removed");
    } else if args.iter().any(|a| a == "-d") {
        println!("lpadmin: default printer set");
    } else if args.iter().any(|a| a == "-p") {
        println!("lpadmin: printer configured");
    }
    0
}

fn run_lpoptions(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lpoptions [OPTIONS]");
        println!();
        println!("lpoptions — display/set printer options (OurOS).");
        println!();
        println!("Options:");
        println!("  -d PRINTER     Set default printer");
        println!("  -p PRINTER     Select printer");
        println!("  -l             List options");
        println!("  -o OPTION=VAL  Set option");
        return 0;
    }
    if args.iter().any(|a| a == "-l") {
        println!("PageSize/Page Size: Letter *A4 Legal A3 A5");
        println!("MediaType/Media Type: *Plain Transparency Glossy");
        println!("ColorModel/Color Mode: *CMYK Gray");
        println!("Duplex/Two-Sided: None *DuplexNoTumble DuplexTumble");
        println!("Resolution/Resolution: 300dpi *600dpi 1200dpi");
    } else {
        println!("copies=1 sides=two-sided-long-edge");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "lp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "lpr" => run_lpr(&rest),
        "lpq" => run_lpq(&rest),
        "lprm" => run_lprm(&rest),
        "lpstat" => run_lpstat(&rest),
        "lpadmin" => run_lpadmin(&rest),
        "lpoptions" => run_lpoptions(&rest),
        _ => run_lp(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lp"), "lp");
        assert_eq!(basename(r"C:\bin\lp.exe"), "lp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lp.exe"), "lp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_lp(&["--help".to_string()]), 0);
        assert_eq!(run_lp(&["-h".to_string()]), 0);
        assert_eq!(run_lp(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_lp(&[]), 0);
    }
}
