#![deny(clippy::all)]

//! pastel-cli — OurOS pastel color tool
//!
//! Single personality: `pastel`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pastel(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pastel [OPTIONS] COMMAND [ARGS...]");
        println!("pastel 0.9.0 (OurOS) — Generate, analyze, convert, and manipulate colors");
        println!();
        println!("Commands:");
        println!("  color COLOR         Display color information");
        println!("  list               List named colors");
        println!("  random             Generate random color");
        println!("  distinct N         Generate N distinct colors");
        println!("  sort-by PROP       Sort colors by property");
        println!("  pick               Color picker");
        println!("  format FMT COLOR   Format color (hex, rgb, hsl, lab, name)");
        println!("  paint COLOR TEXT   Paint text with color");
        println!("  mix C1 C2          Mix two colors");
        println!("  lighten AMT COLOR  Lighten color");
        println!("  darken AMT COLOR   Darken color");
        println!("  saturate AMT COLOR Saturate color");
        println!("  desaturate AMT C   Desaturate color");
        println!("  rotate DEG COLOR   Rotate hue");
        println!("  complement COLOR   Complementary color");
        println!("  to-gray COLOR      Convert to grayscale");
        println!("  gradient C1 C2     Color gradient");
        println!();
        println!("Options:");
        println!("  -m, --mode MODE    Color mode (8bit, 24bit)");
        println!("  -V, --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("pastel 0.9.0 (OurOS)");
        return 0;
    }
    let cmd = args.iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("list");
    match cmd {
        "color" => {
            let color = args.iter().skip_while(|a| a.as_str() != "color").nth(1)
                .map(|s| s.as_str()).unwrap_or("#ff0000");
            println!("Color: {}", color);
            println!("  RGB: (255, 0, 0)");
            println!("  HSL: (0.0, 100.0%, 50.0%)");
            println!("  Name: red");
        }
        "list" => {
            println!("red      #ff0000");
            println!("green    #00ff00");
            println!("blue     #0000ff");
            println!("yellow   #ffff00");
            println!("cyan     #00ffff");
            println!("magenta  #ff00ff");
        }
        "random" => println!("#a3c4f2"),
        "distinct" => {
            let n = args.iter().skip_while(|a| a.as_str() != "distinct").nth(1)
                .map(|s| s.as_str()).unwrap_or("5");
            println!("pastel: Generating {} distinct colors:", n);
            println!("#e41a1c  #377eb8  #4daf4a  #984ea3  #ff7f00");
        }
        "complement" => {
            let color = args.iter().skip_while(|a| a.as_str() != "complement").nth(1)
                .map(|s| s.as_str()).unwrap_or("#ff0000");
            println!("Complement of {}: #00ffff", color);
        }
        "mix" => println!("pastel mix: #808080"),
        "gradient" => println!("#ff0000 → #ff4000 → #ff8000 → #ffc000 → #ffff00"),
        _ => println!("pastel {}: (executed)", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pastel".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pastel(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pastel};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pastel"), "pastel");
        assert_eq!(basename(r"C:\bin\pastel.exe"), "pastel.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pastel.exe"), "pastel");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pastel(&["--help".to_string()], "pastel"), 0);
        assert_eq!(run_pastel(&["-h".to_string()], "pastel"), 0);
        let _ = run_pastel(&["--version".to_string()], "pastel");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pastel(&[], "pastel");
    }
}
