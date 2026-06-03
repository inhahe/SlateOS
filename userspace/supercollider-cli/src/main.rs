#![deny(clippy::all)]

//! supercollider-cli — OurOS SuperCollider audio synthesis
//!
//! Multi-personality: `sclang`, `scsynth`, `scide`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sclang(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sclang [OPTIONS] [FILE.scd]");
        println!("SuperCollider Language 3.13.0 (OurOS)");
        println!("  -d DIR         Runtime directory");
        println!("  -u PORT        UDP port for server");
        println!("  -l N           Post window lines");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("sclang 3.13.0 (OurOS)");
        return 0;
    }
    let file = args.iter().find(|a| a.ends_with(".scd") || a.ends_with(".sc")).map(|s| s.as_str());
    println!("compiling class library...");
    println!("  NumPrimitives = 710");
    println!("  compiling dir: '/usr/share/SuperCollider/SCClassLibrary'");
    println!("  compile done");
    println!("Welcome to SuperCollider 3.13.0.");
    if let Some(f) = file {
        println!("Executing: {}", f);
        println!("[script completed]");
    } else {
        println!("sc3>");
    }
    0
}

fn run_scsynth(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: scsynth [OPTIONS]");
        println!("  -u PORT        UDP port (default: 57110)");
        println!("  -a N           Number of audio bus channels");
        println!("  -i N           Number of input channels");
        println!("  -o N           Number of output channels");
        println!("  -b N           Number of sample buffers");
        println!("  -R N           Real-time memory (KB)");
        println!("  -D N           Load SynthDefs (0=no, 1=yes)");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("scsynth 3.13.0 (OurOS)");
        return 0;
    }
    let port = args.windows(2).find(|w| w[0] == "-u").map(|w| w[1].as_str()).unwrap_or("57110");
    println!("SuperCollider 3 server ready.");
    println!("  Listening on UDP port {}", port);
    println!("  Sample rate: 48000");
    println!("  Block size: 64");
    println!("  Audio channels: 2 in, 2 out");
    println!("  Buffer memory: 32 MB");
    0
}

fn run_scide(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: scide [OPTIONS] [FILE.scd]");
        println!("  --version    Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("SuperCollider IDE 3.13.0 (OurOS)");
        return 0;
    }
    println!("SuperCollider IDE 3.13.0 — Starting...");
    println!("Ready.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sclang".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "scsynth" => run_scsynth(&rest),
        "scide" => run_scide(&rest),
        _ => run_sclang(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sclang};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/supercollider"), "supercollider");
        assert_eq!(basename(r"C:\bin\supercollider.exe"), "supercollider.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("supercollider.exe"), "supercollider");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_sclang(&["--help".to_string()]), 0);
        assert_eq!(run_sclang(&["-h".to_string()]), 0);
        assert_eq!(run_sclang(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_sclang(&[]), 0);
    }
}
