#![deny(clippy::all)]

//! wps-cli — SlateOS WPS Office (Kingsoft) productivity suite
//!
//! Single personality: `wps`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wps(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wps [OPTIONS]");
        println!("WPS Office 2024 (Slate OS) — Kingsoft cross-platform office suite");
        println!();
        println!("Options:");
        println!("  --app NAME             writer/spreadsheets/presentation/pdf");
        println!("  --ai                   WPS AI assistant (LLM-powered)");
        println!("  --cloud                WPS Cloud (1GB free, more with premium)");
        println!("  --premium              WPS Premium subscription");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("WPS Office 2024 12.2.0.16909 (Slate OS)"); return 0; }
    println!("WPS Office 2024 12.2.0.16909 (Slate OS)");
    println!("  Vendor: Kingsoft Office Software (Beijing, founded 1988)");
    println!("  History: WPS = 'Word Processing System' — predates Microsoft Word in China");
    println!("  Components: Writer (Word-compat), Spreadsheets (Excel-compat), Presentation");
    println!("              (PPT-compat), PDF tools (view/edit/convert/sign/OCR)");
    println!("  File formats: .docx/.xlsx/.pptx native, .doc/.xls/.ppt, ODF, PDF");
    println!("              + Kingsoft's .wps/.et/.dps formats");
    println!("  Platforms: Windows, macOS, Linux, Android, iOS, Web");
    println!("  Free tier: full features, ad-supported (banner ads in toolbar)");
    println!("  Premium: $35.99/yr — no ads, PDF tools unlimited, more cloud storage");
    println!("  Enterprise: WPS Office for Enterprise — central deployment, no telemetry");
    println!("  Market: dominant in China (90%+ share), strong in SE Asia/Africa/Russia");
    println!("  UI: ribbon similar to MS Office, light + dark themes");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wps".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wps(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wps};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wps"), "wps");
        assert_eq!(basename(r"C:\bin\wps.exe"), "wps.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wps.exe"), "wps");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wps(&["--help".to_string()], "wps"), 0);
        assert_eq!(run_wps(&["-h".to_string()], "wps"), 0);
        let _ = run_wps(&["--version".to_string()], "wps");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wps(&[], "wps");
    }
}
