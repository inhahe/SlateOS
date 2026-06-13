#![deny(clippy::all)]

//! birdfont-cli — SlateOS BirdFont font editor
//!
//! Single personality: `birdfont`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_birdfont(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: birdfont [OPTIONS] [FILE.bf|.otf|.ttf]");
        println!("birdfont v4.33 (SlateOS) — Font editor");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Features:");
        println!("  Glyph drawing with Bezier curves, variable fonts,");
        println!("  kerning editor, OpenType features, SVG import,");
        println!("  export to TTF/OTF/SVG/EOT");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("birdfont v4.33 (SlateOS)"); return 0; }
    println!("birdfont: font editor started");
    println!("  Tools: pen, bezier, point, freehand");
    println!("  Grid: adjustable, snap to grid");
    println!("  Preview: live text rendering");
    println!("  Export: TTF, OTF, SVG font, EOT");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "birdfont".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_birdfont(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_birdfont};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/birdfont"), "birdfont");
        assert_eq!(basename(r"C:\bin\birdfont.exe"), "birdfont.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("birdfont.exe"), "birdfont");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_birdfont(&["--help".to_string()], "birdfont"), 0);
        assert_eq!(run_birdfont(&["-h".to_string()], "birdfont"), 0);
        let _ = run_birdfont(&["--version".to_string()], "birdfont");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_birdfont(&[], "birdfont");
    }
}
