#![deny(clippy::all)]

//! matlab-cli — Slate OS MathWorks MATLAB technical computing
//!
//! Single personality: `matlab`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_matlab(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: matlab [OPTIONS]");
        println!("MathWorks MATLAB R2024b (Slate OS) — Numerical computing environment");
        println!();
        println!("Options:");
        println!("  -nodisplay -nojvm      Headless (no display, no Java)");
        println!("  -batch \"CMD\"           Run command and exit");
        println!("  -r \"CMD\"               Run command interactively");
        println!("  -nosplash              Skip splash screen");
        println!("  --toolbox TBX          Toolbox (signal/image/control/dsp/...)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("MathWorks MATLAB 9.17.0.2403282 (R2024b) (Slate OS)"); return 0; }
    println!("MathWorks MATLAB R2024b (Slate OS)");
    println!("  Language: high-level array-oriented (.m files)");
    println!("  Toolboxes: Signal Processing, Image Processing, Control Systems,");
    println!("             Communications, Deep Learning, Optimization, Statistics,");
    println!("             Parallel Computing, GPU Coder, Simulink, RoboticsSystem");
    println!("  Apps: APP Designer, Curve Fitter, System Identifier, Live Editor");
    println!("  Code gen: MATLAB Coder (C/C++), GPU Coder (CUDA), HDL Coder (VHDL/Verilog)");
    println!("  License: per-toolbox subscription (academic + commercial)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "matlab".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_matlab(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_matlab};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/matlab"), "matlab");
        assert_eq!(basename(r"C:\bin\matlab.exe"), "matlab.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("matlab.exe"), "matlab");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_matlab(&["--help".to_string()], "matlab"), 0);
        assert_eq!(run_matlab(&["-h".to_string()], "matlab"), 0);
        let _ = run_matlab(&["--version".to_string()], "matlab");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_matlab(&[], "matlab");
    }
}
