#![deny(clippy::all)]

//! ibus-cli — OurOS IBus input method framework CLI
//!
//! Multi-personality: `ibus`, `ibus-daemon`, `ibus-setup`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_ibus(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ibus COMMAND [OPTIONS]");
        println!();
        println!("IBus — intelligent input bus (OurOS).");
        println!();
        println!("Commands:");
        println!("  engine [NAME]    Get/set input method engine");
        println!("  list-engine      List available engines");
        println!("  restart          Restart IBus daemon");
        println!("  exit             Exit IBus daemon");
        println!("  version          Show version");
        println!("  read-cache       Read engine cache");
        println!("  write-cache      Write engine cache");
        println!("  address          Print IBus address");
        println!("  emoji            Show emoji picker");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("IBus 1.5.29 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");

    match cmd {
        "version" => println!("IBus 1.5.29 (OurOS)"),
        "list-engine" => {
            println!("language: en");
            println!("  xkb:us::eng  - English (US)");
            println!("  xkb:gb::eng  - English (UK)");
            println!("language: de");
            println!("  xkb:de::deu  - German");
            println!("language: fr");
            println!("  xkb:fr::fra  - French");
            println!("language: ja");
            println!("  anthy        - Anthy");
            println!("  kkc          - Kana Kanji");
            println!("language: zh");
            println!("  pinyin       - Intelligent Pinyin");
            println!("  bopomofo     - Bopomofo");
            println!("language: ko");
            println!("  hangul       - Hangul");
        }
        "engine" => {
            if let Some(name) = args.get(1) {
                println!("Set engine to '{}'", name);
            } else {
                println!("xkb:us::eng");
            }
        }
        "restart" => println!("IBus daemon restarted."),
        "exit" => println!("IBus daemon stopped."),
        "address" => println!("unix:abstract=/tmp/dbus-ibus-12345,guid=abc123"),
        "read-cache" | "write-cache" => println!("Cache operation completed."),
        "emoji" => println!("Opening emoji picker..."),
        _ => {
            eprintln!("ibus: unknown command '{}'. See --help.", cmd);
            return 1;
        }
    }
    0
}

fn run_ibus_daemon(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ibus-daemon [OPTIONS]");
        println!();
        println!("IBus daemon (OurOS).");
        println!();
        println!("Options:");
        println!("  -d, --daemonize    Run as daemon");
        println!("  -s, --single       Single instance");
        println!("  -x, --xim          Start XIM server");
        println!("  -r, --replace      Replace existing daemon");
        println!("  -p, --panel PROG   Panel program");
        println!("  -v, --verbose      Verbose");
        return 0;
    }
    let xim = args.iter().any(|a| a == "-x" || a == "--xim");
    println!("IBus daemon 1.5.29 starting...");
    if xim {
        println!("  XIM server: enabled");
    }
    println!("  D-Bus: connected");
    println!("  Panel: ibus-ui-gtk3");
    println!("  Ready.");
    0
}

fn run_ibus_setup(_args: &[String]) -> i32 {
    println!("IBus Preferences");
    println!();
    println!("Input Method:");
    println!("  1. English (US) - xkb:us::eng");
    println!();
    println!("General:");
    println!("  Next input method: Super+Space");
    println!("  Show property panel: Do not show");
    println!("  Show icon on system tray: Yes");
    println!("  Embed preedit text: Yes");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "ibus".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "ibus-daemon" => run_ibus_daemon(&rest),
        "ibus-setup" => run_ibus_setup(&rest),
        _ => run_ibus(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ibus};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ibus"), "ibus");
        assert_eq!(basename(r"C:\bin\ibus.exe"), "ibus.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ibus.exe"), "ibus");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ibus(&["--help".to_string()]), 0);
        assert_eq!(run_ibus(&["-h".to_string()]), 0);
        let _ = run_ibus(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ibus(&[]);
    }
}
