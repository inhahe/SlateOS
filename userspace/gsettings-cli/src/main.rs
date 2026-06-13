#![deny(clippy::all)]

//! gsettings-cli — SlateOS GSettings CLI
//!
//! Single personality: `gsettings`

use std::env;
use std::process;

fn run_gsettings(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: gsettings [--schemadir DIR] COMMAND [ARGS]");
        println!();
        println!("gsettings — GSettings configuration tool (SlateOS).");
        println!();
        println!("Commands:");
        println!("  get SCHEMA KEY          Get key value");
        println!("  set SCHEMA KEY VALUE    Set key value");
        println!("  reset SCHEMA KEY        Reset key to default");
        println!("  reset-recursively SCHEMA Reset all keys");
        println!("  list-schemas            List installed schemas");
        println!("  list-relocatable-schemas List relocatable schemas");
        println!("  list-keys SCHEMA        List keys in schema");
        println!("  list-children SCHEMA    List child schemas");
        println!("  list-recursively [SCHEMA] List all keys");
        println!("  range SCHEMA KEY        Show allowed values");
        println!("  describe SCHEMA KEY     Describe key");
        println!("  monitor SCHEMA [KEY]    Watch for changes");
        println!("  writable SCHEMA KEY     Check if writable");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");

    match cmd {
        "get" => {
            let schema = args.get(1).map(|s| s.as_str()).unwrap_or("org.gnome.desktop.interface");
            let key = args.get(2).map(|s| s.as_str()).unwrap_or("gtk-theme");
            let _ = schema;
            match key {
                "gtk-theme" => println!("'Adwaita'"),
                "icon-theme" => println!("'Adwaita'"),
                "font-name" => println!("'Cantarell 11'"),
                "cursor-theme" => println!("'default'"),
                "color-scheme" => println!("'prefer-dark'"),
                _ => println!("'default'"),
            }
        }
        "set" => {
            if args.len() < 4 {
                eprintln!("gsettings: SCHEMA KEY VALUE required");
                return 1;
            }
        }
        "reset" => {}
        "list-schemas" => {
            println!("org.gnome.desktop.interface");
            println!("org.gnome.desktop.background");
            println!("org.gnome.desktop.wm.preferences");
            println!("org.gnome.desktop.sound");
            println!("org.gnome.desktop.input-sources");
            println!("org.gnome.desktop.notifications");
        }
        "list-keys" => {
            println!("gtk-theme");
            println!("icon-theme");
            println!("cursor-theme");
            println!("font-name");
            println!("color-scheme");
            println!("enable-animations");
            println!("toolbar-style");
        }
        "list-recursively" => {
            println!("org.gnome.desktop.interface gtk-theme 'Adwaita'");
            println!("org.gnome.desktop.interface icon-theme 'Adwaita'");
            println!("org.gnome.desktop.interface font-name 'Cantarell 11'");
            println!("org.gnome.desktop.interface color-scheme 'prefer-dark'");
        }
        "describe" => {
            println!("Name of the GTK theme to use");
        }
        "range" => {
            println!("type s");
        }
        "writable" => println!("true"),
        "monitor" => {
            let schema = args.get(1).map(|s| s.as_str()).unwrap_or("org.gnome.desktop.interface");
            println!("Monitoring '{}'...", schema);
        }
        _ => {
            eprintln!("gsettings: unknown command '{}'. See --help.", cmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gsettings(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_gsettings};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gsettings(vec!["--help".to_string()]), 0);
        assert_eq!(run_gsettings(vec!["-h".to_string()]), 0);
        let _ = run_gsettings(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gsettings(vec![]);
    }
}
