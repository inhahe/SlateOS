#![deny(clippy::all)]

//! microsoft365-cli — OurOS Microsoft 365 / Office productivity suite
//!
//! Single personality: `microsoft365`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_m365(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: microsoft365 [OPTIONS]");
        println!("Microsoft 365 (OurOS) — Cloud productivity suite + Office apps");
        println!();
        println!("Options:");
        println!("  --app NAME             word/excel/powerpoint/outlook/onenote/teams");
        println!("  --copilot              Microsoft 365 Copilot (AI assistant)");
        println!("  --sharepoint           SharePoint Online (intranet/document mgmt)");
        println!("  --onedrive             OneDrive for Business (1-5 TB)");
        println!("  --plan PLAN            personal/family/business-basic/standard/premium/E3/E5");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Microsoft 365 Apps for Enterprise 2410 build 18227.20140 (OurOS)"); return 0; }
    println!("Microsoft 365 Apps for Enterprise 2410 (OurOS)");
    println!("  Vendor: Microsoft (Redmond, Washington)");
    println!("  Rebrand: Office 365 → Microsoft 365 (Apr 2020), Office 2024 perpetual still sold");
    println!("  Apps: Word, Excel, PowerPoint, Outlook, OneNote, Access, Publisher (Win),");
    println!("        Teams, OneDrive, SharePoint, Forms, Stream, Sway, Visio, Project");
    println!("  Consumer: Personal ($69.99/yr), Family ($99.99/yr, 6 users, 1TB each)");
    println!("  Business: Basic/Standard/Premium ($6/$12.50/$22 per user/mo)");
    println!("  Enterprise: E3 ($36) / E5 ($57) per user/mo — includes Defender, Power BI Pro");
    println!("  Copilot: $30/user/mo add-on — GPT-4 in Word/Excel/PowerPoint/Outlook/Teams");
    println!("  File formats: .docx (OOXML, ISO/IEC 29500), .xlsx, .pptx; legacy .doc/.xls/.ppt");
    println!("  Collaboration: real-time co-authoring, comments, @mentions, version history");
    println!("  Mobile: iOS/Android (free for personal use on phones, paid on tablets)");
    println!("  Web: office.com (free tier with basic editing, ad-supported)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "microsoft365".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_m365(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_m365};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/microsoft365"), "microsoft365");
        assert_eq!(basename(r"C:\bin\microsoft365.exe"), "microsoft365.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("microsoft365.exe"), "microsoft365");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_m365(&["--help".to_string()], "microsoft365"), 0);
        assert_eq!(run_m365(&["-h".to_string()], "microsoft365"), 0);
        assert_eq!(run_m365(&["--version".to_string()], "microsoft365"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_m365(&[], "microsoft365"), 0);
    }
}
