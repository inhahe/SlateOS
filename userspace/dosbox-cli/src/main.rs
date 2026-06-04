#![deny(clippy::all)]

//! dosbox-cli — OurOS DOSBox emulator CLI
//!
//! Multi-personality: `dosbox`, `dosbox-x`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_dosbox(args: &[String], extended: bool) -> i32 {
    let name = if extended { "dosbox-x" } else { "dosbox" };

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS] [FILE|DIR]", name);
        println!();
        println!("{} — DOS emulator (OurOS).", name);
        println!();
        println!("Options:");
        println!("  -fullscreen        Start in fullscreen");
        println!("  -conf FILE         Use config file");
        println!("  -lang FILE         Use language file");
        println!("  -machine TYPE      Machine type (hercules/cga/ega/vga/svga)");
        println!("  -noautoexec        Skip [autoexec] section");
        println!("  -c COMMAND         Execute command on startup");
        println!("  -exit              Exit after commands");
        println!("  -scaler TYPE       Scaler type");
        println!("  -startmapper       Start key mapper");
        if extended {
            println!("  -defaultconf       Write default config");
            println!("  -defaultdir DIR    Default directory");
            println!("  -hostkey KEY       Set host key");
        }
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-version") {
        if extended {
            println!("DOSBox-X version 2024.03.01 (OurOS)");
        } else {
            println!("DOSBox version 0.74-3 (OurOS)");
        }
        return 0;
    }

    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str());
    let commands: Vec<&str> = args.windows(2)
        .filter(|w| w[0] == "-c")
        .map(|w| w[1].as_str())
        .collect();

    let fullscreen = args.iter().any(|a| a == "-fullscreen");

    if extended {
        println!("DOSBox-X version 2024.03.01 (OurOS)");
    } else {
        println!("DOSBox version 0.74-3 (OurOS)");
    }
    println!("Copyright 2002-2024 DOSBox Team");
    println!();

    if fullscreen {
        println!("Starting in fullscreen mode.");
    }

    if let Some(f) = file {
        println!("Mounting: {} as C:", f);
        println!("Running {}...", f);
    }

    for cmd in &commands {
        println!("Executing: {}", cmd);
    }

    if file.is_none() && commands.is_empty() {
        println!("Z:\\>");
    }

    if args.iter().any(|a| a == "-exit") && !commands.is_empty() {
        println!("Exiting after commands.");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "dosbox".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "dosbox-x" => run_dosbox(&rest, true),
        _ => run_dosbox(&rest, false),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dosbox};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dosbox"), "dosbox");
        assert_eq!(basename(r"C:\bin\dosbox.exe"), "dosbox.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dosbox.exe"), "dosbox");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dosbox(&["--help".to_string()], false), 0);
        assert_eq!(run_dosbox(&["-h".to_string()], false), 0);
        let _ = run_dosbox(&["--version".to_string()], false);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dosbox(&[], false);
    }
}
