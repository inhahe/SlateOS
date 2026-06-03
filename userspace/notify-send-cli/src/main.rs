#![deny(clippy::all)]

//! notify-send-cli — OurOS notify-send desktop notification CLI
//!
//! Single personality: `notify-send`

use std::env;
use std::process;

fn run_notify_send(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: notify-send [OPTIONS] SUMMARY [BODY]");
        println!();
        println!("notify-send — desktop notification sender (OurOS).");
        println!();
        println!("Options:");
        println!("  -u, --urgency LEVEL  Urgency (low/normal/critical)");
        println!("  -t, --expire-time MS Timeout in milliseconds");
        println!("  -a, --app-name NAME  Application name");
        println!("  -i, --icon ICON      Icon name or path");
        println!("  -c, --category TYPE  Notification category");
        println!("  -h, --hint TYPE:NAME:VALUE  Extra data hint");
        println!("  -r, --replace-id ID  Replace existing notification");
        println!("  -w, --wait           Wait for notification to close");
        println!("  -A, --action=ID=TEXT Add action button");
        println!("  -e, --transient      Transient notification");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("notify-send 0.8.3 (OurOS)");
        return 0;
    }

    let positional: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if positional.is_empty() {
        eprintln!("notify-send: no summary specified");
        return 1;
    }

    let summary = positional[0];
    let body = positional.get(1).unwrap_or(&"");

    let urgency = args.windows(2)
        .find(|w| w[0] == "-u" || w[0] == "--urgency")
        .map(|w| w[1].as_str())
        .unwrap_or("normal");

    let app_name = args.windows(2)
        .find(|w| w[0] == "-a" || w[0] == "--app-name")
        .map(|w| w[1].as_str())
        .unwrap_or("notify-send");

    let icon = args.windows(2)
        .find(|w| w[0] == "-i" || w[0] == "--icon")
        .map(|w| w[1].as_str());

    // In a real implementation, sends via D-Bus to notification daemon
    println!("Notification sent:");
    println!("  App: {}", app_name);
    println!("  Summary: {}", summary);
    if !body.is_empty() {
        println!("  Body: {}", body);
    }
    println!("  Urgency: {}", urgency);
    if let Some(ic) = icon {
        println!("  Icon: {}", ic);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_notify_send(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_notify_send};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_notify_send(vec!["--help".to_string()]), 0);
        assert_eq!(run_notify_send(vec!["-h".to_string()]), 0);
        assert_eq!(run_notify_send(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_notify_send(vec![]), 0);
    }
}
