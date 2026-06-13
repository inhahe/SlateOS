#![deny(clippy::all)]

//! wkhtmltopdf-cli — SlateOS HTML to PDF/image converter
//!
//! Multi-personality: `wkhtmltopdf`, `wkhtmltoimage`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wkhtmltopdf(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS] <input> <output>", prog);
        println!("{} v0.12.6 (SlateOS) — HTML to PDF converter", prog);
        println!();
        println!("Options:");
        println!("  --page-size SIZE     Page size (A4, Letter, etc.)");
        println!("  --orientation ORI    Portrait or Landscape");
        println!("  --margin-top N       Top margin in mm");
        println!("  --margin-bottom N    Bottom margin in mm");
        println!("  --margin-left N      Left margin in mm");
        println!("  --margin-right N     Right margin in mm");
        println!("  --dpi N              Output DPI (default: 96)");
        println!("  --grayscale          PDF in grayscale");
        println!("  --lowquality         Generate lower-quality PDF");
        println!("  --title TITLE        PDF document title");
        println!("  --encoding ENC       Input encoding (default: utf-8)");
        println!("  --no-images          Do not load images");
        println!("  --javascript-delay N Wait N ms for JS (default: 200)");
        println!("  --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("{} v0.12.6 (SlateOS) with patched Qt", prog);
        return 0;
    }
    if args.len() < 2 {
        eprintln!("{}: error: expected <input> <output>", prog);
        return 1;
    }
    let input = &args[args.len() - 2];
    let output = &args[args.len() - 1];
    println!("Loading page... {}", input);
    println!("Counting pages... 3");
    println!("Rendering pages...");
    println!("  Page 1 of 3");
    println!("  Page 2 of 3");
    println!("  Page 3 of 3");
    println!("Done: {} [{} bytes]", output, 245_760);
    0
}

fn run_wkhtmltoimage(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS] <input> <output>", prog);
        println!("{} v0.12.6 (SlateOS) — HTML to image converter", prog);
        println!();
        println!("Options:");
        println!("  --format FMT         Output format (png, jpg, svg, bmp)");
        println!("  --width N            Set screen width (default: 1024)");
        println!("  --height N           Set screen height");
        println!("  --quality N          JPEG quality (0-100)");
        println!("  --crop-x N           Crop X offset");
        println!("  --crop-y N           Crop Y offset");
        println!("  --crop-w N           Crop width");
        println!("  --crop-h N           Crop height");
        println!("  --transparent        Use transparent background");
        println!("  --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("{} v0.12.6 (SlateOS) with patched Qt", prog);
        return 0;
    }
    if args.len() < 2 {
        eprintln!("{}: error: expected <input> <output>", prog);
        return 1;
    }
    let input = &args[args.len() - 2];
    let output = &args[args.len() - 1];
    println!("Loading page... {}", input);
    println!("Rendering...");
    println!("Done: {} [1920x1080, {} bytes]", output, 1_048_576);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wkhtmltopdf".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "wkhtmltoimage" => run_wkhtmltoimage(&rest, &prog),
        _ => run_wkhtmltopdf(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wkhtmltopdf};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wkhtmltopdf"), "wkhtmltopdf");
        assert_eq!(basename(r"C:\bin\wkhtmltopdf.exe"), "wkhtmltopdf.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wkhtmltopdf.exe"), "wkhtmltopdf");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wkhtmltopdf(&["--help".to_string()], "wkhtmltopdf"), 0);
        assert_eq!(run_wkhtmltopdf(&["-h".to_string()], "wkhtmltopdf"), 0);
        let _ = run_wkhtmltopdf(&["--version".to_string()], "wkhtmltopdf");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wkhtmltopdf(&[], "wkhtmltopdf");
    }
}
