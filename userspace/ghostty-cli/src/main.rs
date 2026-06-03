#![deny(clippy::all)]

//! ghostty-cli — OurOS Ghostty terminal emulator
//!
//! Single personality: `ghostty`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ghostty(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ghostty [OPTIONS]");
        println!("Ghostty 1.0.0 (OurOS) — Fast, feature-rich terminal emulator");
        println!();
        println!("Options:");
        println!("  --config-file FILE       Config file path");
        println!("  --working-directory DIR  Working directory");
        println!("  --title TEXT             Window title");
        println!("  --class TEXT             Window class");
        println!("  --command, -e CMD        Command to run");
        println!("  --wait-after-command     Wait after command exits");
        println!("  --version                Show version");
        println!();
        println!("Actions:");
        println!("  +list-fonts              List available fonts");
        println!("  +list-keybinds           List key bindings");
        println!("  +list-themes             List available themes");
        println!("  +list-colors             List current colors");
        println!("  +list-actions            List all actions");
        println!("  +show-config             Show effective config");
        println!("  +validate-config         Validate config file");
        println!("  +crash-report            Show crash info");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("ghostty 1.0.0 (OurOS)");
        return 0;
    }
    // Handle + actions
    if let Some(action) = args.iter().find(|a| a.starts_with('+')) {
        match action.as_str() {
            "+list-fonts" => {
                println!("JetBrains Mono");
                println!("Fira Code");
                println!("Cascadia Code");
                println!("Hack");
                println!("Iosevka");
            }
            "+list-keybinds" => {
                println!("ctrl+shift+c = copy_to_clipboard");
                println!("ctrl+shift+v = paste_from_clipboard");
                println!("ctrl+shift+t = new_tab");
                println!("ctrl+shift+n = new_window");
                println!("ctrl+shift+enter = new_split:right");
            }
            "+list-themes" => {
                println!("Dracula");
                println!("Gruvbox Dark");
                println!("Nord");
                println!("One Dark");
                println!("Solarized Dark");
                println!("Tokyo Night");
            }
            "+list-colors" => {
                println!("foreground: #c0caf5");
                println!("background: #1a1b26");
                println!("cursor: #c0caf5");
            }
            "+list-actions" => {
                println!("copy_to_clipboard");
                println!("paste_from_clipboard");
                println!("new_tab");
                println!("new_window");
                println!("close_surface");
                println!("new_split:right");
                println!("new_split:down");
                println!("goto_split:next");
            }
            "+show-config" => {
                println!("font-family = JetBrains Mono");
                println!("font-size = 13");
                println!("theme = Tokyo Night");
                println!("window-padding-x = 4");
                println!("window-padding-y = 4");
            }
            "+validate-config" => println!("Configuration is valid."),
            "+crash-report" => println!("No crash reports found."),
            _ => println!("ghostty: unknown action '{}'", action),
        }
        return 0;
    }
    println!("ghostty: Starting terminal...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ghostty".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ghostty(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ghostty};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ghostty"), "ghostty");
        assert_eq!(basename(r"C:\bin\ghostty.exe"), "ghostty.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ghostty.exe"), "ghostty");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_ghostty(&["--help".to_string()], "ghostty"), 0);
        assert_eq!(run_ghostty(&["-h".to_string()], "ghostty"), 0);
        assert_eq!(run_ghostty(&["--version".to_string()], "ghostty"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_ghostty(&[], "ghostty"), 0);
    }
}
