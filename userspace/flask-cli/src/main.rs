#![deny(clippy::all)]

//! flask-cli — Slate OS Flask web framework CLI
//!
//! Multi-personality: `flask`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_flask(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: flask COMMAND [OPTIONS]");
        println!("Flask 3.0.3 (Slate OS)");
        println!();
        println!("Commands:");
        println!("  run          Run development server");
        println!("  shell        Start Python shell with app context");
        println!("  routes       Show registered routes");
        println!("  db           Database commands (Flask-Migrate)");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("Python 3.12.4, Flask 3.0.3, Werkzeug 3.0.3"),
        "run" => {
            let host = args.windows(2).find(|w| w[0] == "--host" || w[0] == "-h")
                .map(|w| w[1].as_str()).unwrap_or("127.0.0.1");
            let port = args.windows(2).find(|w| w[0] == "--port" || w[0] == "-p")
                .map(|w| w[1].as_str()).unwrap_or("5000");
            let debug = args.iter().any(|a| a == "--debug");
            println!(" * Serving Flask app 'app'");
            if debug {
                println!(" * Debug mode: on");
            }
            println!(" * Running on http://{}:{}", host, port);
            println!(" * Restarting with stat");
        }
        "shell" => {
            println!("Python 3.12.4");
            println!("App: app");
            println!("Instance: /home/user/myapp/instance");
            println!(">>> ");
        }
        "routes" => {
            println!("Endpoint       Methods    Rule");
            println!("-----------    --------   -------------------------");
            println!("static         GET        /static/<path:filename>");
            println!("index          GET        /");
            println!("api.users      GET, POST  /api/users");
            println!("api.user       GET, PUT   /api/users/<int:id>");
        }
        "db" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("help");
            match sub {
                "init" => println!("Created migration directory."),
                "migrate" => {
                    println!("Generating migration...");
                    println!("  Running revision abc12345 -> def67890, 'add user table'");
                }
                "upgrade" => println!("Running upgrade: head"),
                "downgrade" => println!("Running downgrade: -1"),
                _ => println!("flask db: '{}' completed", sub),
            }
        }
        _ => println!("flask: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "flask".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_flask(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_flask};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/flask"), "flask");
        assert_eq!(basename(r"C:\bin\flask.exe"), "flask.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("flask.exe"), "flask");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_flask(&["--help".to_string()]), 0);
        assert_eq!(run_flask(&["-h".to_string()]), 0);
        let _ = run_flask(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_flask(&[]);
    }
}
