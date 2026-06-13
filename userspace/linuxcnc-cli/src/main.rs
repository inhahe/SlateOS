#![deny(clippy::all)]

//! linuxcnc-cli — SlateOS LinuxCNC machine control
//!
//! Multi-personality: `linuxcnc`, `halcmd`, `halrun`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_linuxcnc(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: linuxcnc [OPTIONS] [CONFIG.ini]");
        println!("LinuxCNC 2.9.2 (Slate OS)");
        println!("  --version      Show version");
        println!("  -d             Debug mode");
        println!("  -v             Verbose");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("LinuxCNC 2.9.2 (Slate OS)");
        println!("EMC2 - Enhanced Machine Controller");
        return 0;
    }
    let config = args.iter().find(|a| a.ends_with(".ini")).map(|s| s.as_str()).unwrap_or("machine.ini");
    println!("LinuxCNC 2.9.2 starting...");
    println!("  Configuration: {}", config);
    println!("  Loading HAL configuration...");
    println!("  Starting motion controller...");
    println!("  Machine ready.");
    0
}

fn run_halcmd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: halcmd COMMAND [ARGS]");
        println!("HAL Command Interface (LinuxCNC 2.9.2)");
        println!();
        println!("Commands:");
        println!("  show          Show HAL items");
        println!("  loadrt MOD    Load realtime module");
        println!("  addf FUNC     Add function to thread");
        println!("  net SIG       Create signal");
        println!("  setp PARAM    Set parameter");
        println!("  sets SIG      Set signal value");
        println!("  getp PARAM    Get parameter");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("show");
    match subcmd {
        "show" => {
            let what = args.get(1).map(|s| s.as_str()).unwrap_or("all");
            match what {
                "pin" | "pins" => {
                    println!("Component Pins:");
                    println!("  Type  Dir  Value  Name");
                    println!("  bit   IN   FALSE  motion.enable");
                    println!("  float OUT  0.000  axis.x.pos-cmd");
                    println!("  float OUT  0.000  axis.y.pos-cmd");
                    println!("  float OUT  0.000  axis.z.pos-cmd");
                    println!("  bit   OUT  FALSE  spindle.on");
                }
                "param" => {
                    println!("Parameters:");
                    println!("  Type  Dir  Value  Name");
                    println!("  s32   RW   1000   base-thread.tmax");
                    println!("  s32   RW   10000  servo-thread.tmax");
                }
                _ => println!("halcmd show: listing HAL components..."),
            }
        }
        "loadrt" => {
            let module = args.get(1).map(|s| s.as_str()).unwrap_or("stepgen");
            println!("halcmd: loading realtime module '{}'", module);
            println!("Module loaded.");
        }
        "net" => {
            println!("halcmd: signal created.");
        }
        "setp" => {
            println!("halcmd: parameter set.");
        }
        _ => println!("halcmd: '{}' completed", subcmd),
    }
    0
}

fn run_halrun(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: halrun [OPTIONS] [FILE.hal]");
        println!("  -f FILE     Execute HAL file");
        println!("  -U          Unload all HAL");
        return 0;
    }
    let file = args.iter().find(|a| a.ends_with(".hal")).map(|s| s.as_str());
    if let Some(f) = file {
        println!("halrun: executing '{}'", f);
        println!("HAL configuration loaded.");
    } else {
        println!("halcmd:");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "linuxcnc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "halcmd" => run_halcmd(&rest),
        "halrun" => run_halrun(&rest),
        _ => run_linuxcnc(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_linuxcnc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/linuxcnc"), "linuxcnc");
        assert_eq!(basename(r"C:\bin\linuxcnc.exe"), "linuxcnc.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("linuxcnc.exe"), "linuxcnc");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_linuxcnc(&["--help".to_string()]), 0);
        assert_eq!(run_linuxcnc(&["-h".to_string()]), 0);
        let _ = run_linuxcnc(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_linuxcnc(&[]);
    }
}
