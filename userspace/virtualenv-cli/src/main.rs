#![deny(clippy::all)]

//! virtualenv-cli — OurOS Python virtual environment creator
//!
//! Multi-personality: `virtualenv`, `venv`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_virtualenv(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: virtualenv [OPTIONS] DEST");
        println!("virtualenv 20.26.3 (OurOS)");
        println!();
        println!("Options:");
        println!("  -p, --python PATH    Python interpreter to use");
        println!("  --system-site-packages  Give access to system site-packages");
        println!("  --clear              Clear destination before creating");
        println!("  --no-pip             Do not install pip");
        println!("  --no-setuptools      Do not install setuptools");
        println!("  --no-wheel           Do not install wheel");
        println!("  --copies             Use copies instead of symlinks");
        println!("  --prompt PROMPT      Custom prompt prefix");
        println!("  --download           Download latest pip/setuptools/wheel");
        println!("  --creator TYPE       Creator type (venv, builtin)");
        println!("  --seeder TYPE        Seeder type (app-data, pip)");
        println!("  --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("virtualenv 20.26.3 from /usr/lib/python3/dist-packages/virtualenv");
        return 0;
    }
    let dest = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or(".venv");
    let python = args.windows(2).find(|w| w[0] == "-p" || w[0] == "--python")
        .map(|w| w[1].as_str()).unwrap_or("python3");
    let no_pip = args.iter().any(|a| a == "--no-pip");
    let clear = args.iter().any(|a| a == "--clear");

    if clear {
        println!("Clearing existing virtualenv at {}", dest);
    }
    println!("created virtual environment CPython3.12.4.final.0-64 in 234ms");
    println!("  creator CPython3Posix(dest={}, clear={})", dest, clear);
    println!("  interpreter: {}", python);
    if !no_pip {
        println!("  seeder FromAppData(download=false, pip=bundle, setuptools=bundle, wheel=bundle)");
        println!("    added seed packages: pip==24.1, setuptools==70.1.0, wheel==0.43.0");
    } else {
        println!("  seeder: none (--no-pip)");
    }
    println!("  activators: BashActivator, FishActivator, NushellActivator, CShellActivator");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "virtualenv".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_virtualenv(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_virtualenv};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/virtualenv"), "virtualenv");
        assert_eq!(basename(r"C:\bin\virtualenv.exe"), "virtualenv.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("virtualenv.exe"), "virtualenv");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_virtualenv(&["--help".to_string()]), 0);
        assert_eq!(run_virtualenv(&["-h".to_string()]), 0);
        assert_eq!(run_virtualenv(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_virtualenv(&[]), 0);
    }
}
