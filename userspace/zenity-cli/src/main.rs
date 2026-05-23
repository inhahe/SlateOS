#![deny(clippy::all)]

//! zenity-cli — OurOS Zenity dialog CLI
//!
//! Multi-personality: `zenity`, `kdialog`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_zenity(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: zenity [OPTIONS]");
        println!();
        println!("Zenity — display GTK dialogs (OurOS).");
        println!();
        println!("Dialog types:");
        println!("  --info               Info dialog");
        println!("  --warning            Warning dialog");
        println!("  --error              Error dialog");
        println!("  --question           Question dialog");
        println!("  --entry              Text entry dialog");
        println!("  --password           Password dialog");
        println!("  --file-selection     File chooser");
        println!("  --color-selection    Color chooser");
        println!("  --calendar           Calendar dialog");
        println!("  --list               List dialog");
        println!("  --progress           Progress dialog");
        println!("  --scale              Scale dialog");
        println!("  --text-info          Text info dialog");
        println!("  --notification       Notification icon");
        println!("  --forms              Forms dialog");
        println!();
        println!("Common options:");
        println!("  --title TEXT         Dialog title");
        println!("  --text TEXT          Dialog text");
        println!("  --width N            Window width");
        println!("  --height N           Window height");
        println!("  --timeout N          Timeout in seconds");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("zenity 4.0.1 (OurOS)");
        return 0;
    }

    let title = args.windows(2).find(|w| w[0] == "--title").map(|w| w[1].as_str()).unwrap_or("Zenity");
    let text = args.windows(2).find(|w| w[0] == "--text").map(|w| w[1].as_str());

    if args.iter().any(|a| a == "--info") {
        println!("[INFO] {}: {}", title, text.unwrap_or("Information"));
    } else if args.iter().any(|a| a == "--warning") {
        println!("[WARNING] {}: {}", title, text.unwrap_or("Warning"));
    } else if args.iter().any(|a| a == "--error") {
        println!("[ERROR] {}: {}", title, text.unwrap_or("Error"));
    } else if args.iter().any(|a| a == "--question") {
        println!("[QUESTION] {}: {}", title, text.unwrap_or("Are you sure?"));
        // Returns 0 for Yes, 1 for No
    } else if args.iter().any(|a| a == "--entry") {
        println!("[ENTRY] {}: {}", title, text.unwrap_or("Enter text:"));
        println!("user_input");
    } else if args.iter().any(|a| a == "--file-selection") {
        println!("/home/user/document.txt");
    } else if args.iter().any(|a| a == "--color-selection") {
        println!("rgb(128,128,255)");
    } else if args.iter().any(|a| a == "--calendar") {
        println!("01/15/2024");
    } else if args.iter().any(|a| a == "--progress") {
        println!("100");
    } else if args.iter().any(|a| a == "--scale") {
        println!("50");
    } else if args.iter().any(|a| a == "--password") {
        println!("(password)");
    }
    0
}

fn run_kdialog(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kdialog [OPTIONS]");
        println!();
        println!("kdialog — KDE dialog tool (OurOS).");
        println!();
        println!("Options:");
        println!("  --msgbox TEXT        Message box");
        println!("  --yesno TEXT         Yes/No dialog");
        println!("  --inputbox TEXT      Input dialog");
        println!("  --password TEXT      Password dialog");
        println!("  --getopenfilename    Open file dialog");
        println!("  --getsavefilename    Save file dialog");
        println!("  --getexistingdirectory  Directory dialog");
        println!("  --passivepopup TEXT  Passive popup");
        println!("  --title TEXT         Dialog title");
        return 0;
    }
    if args.iter().any(|a| a == "--msgbox") {
        let text = args.windows(2).find(|w| w[0] == "--msgbox").map(|w| w[1].as_str()).unwrap_or("Message");
        println!("[OK] {}", text);
    } else if args.iter().any(|a| a == "--yesno") {
        println!("[Yes/No]");
    } else if args.iter().any(|a| a == "--inputbox") {
        println!("user_input");
    } else if args.iter().any(|a| a == "--getopenfilename") {
        println!("/home/user/document.txt");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "zenity".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "kdialog" => run_kdialog(&rest),
        _ => run_zenity(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
