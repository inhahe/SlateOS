#![deny(clippy::all)]

//! fastapi-cli — SlateOS FastAPI CLI tools
//!
//! Multi-personality: `fastapi`, `uvicorn`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fastapi(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: fastapi COMMAND [OPTIONS]");
        println!("FastAPI CLI 0.0.4 (Slate OS)");
        println!();
        println!("Commands:");
        println!("  dev          Run development server");
        println!("  run          Run production server");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("fastapi-cli 0.0.4, FastAPI 0.111.0"),
        "dev" => {
            let app = args.get(1).map(|s| s.as_str()).unwrap_or("main:app");
            println!("FastAPI Starting development server...");
            println!("  Uvicorn running on http://127.0.0.1:8000");
            println!("  Application: {}", app);
            println!("  Reload: enabled");
        }
        "run" => {
            let app = args.get(1).map(|s| s.as_str()).unwrap_or("main:app");
            let workers = args.windows(2).find(|w| w[0] == "--workers")
                .map(|w| w[1].as_str()).unwrap_or("1");
            println!("FastAPI Starting production server...");
            println!("  Uvicorn running on http://0.0.0.0:8000");
            println!("  Application: {}", app);
            println!("  Workers: {}", workers);
        }
        _ => println!("fastapi: '{}' completed", subcmd),
    }
    0
}

fn run_uvicorn(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: uvicorn [OPTIONS] APP");
        println!("Uvicorn 0.30.1 (Slate OS)");
        println!("  --host HOST      Bind address (default: 127.0.0.1)");
        println!("  --port PORT      Port (default: 8000)");
        println!("  --workers N      Worker processes");
        println!("  --reload         Enable auto-reload");
        println!("  --log-level LVL  Log level");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Running uvicorn 0.30.1");
        return 0;
    }
    let app = args.iter().find(|a| a.contains(':')).map(|s| s.as_str()).unwrap_or("main:app");
    let host = args.windows(2).find(|w| w[0] == "--host").map(|w| w[1].as_str()).unwrap_or("127.0.0.1");
    let port = args.windows(2).find(|w| w[0] == "--port").map(|w| w[1].as_str()).unwrap_or("8000");
    let reload = args.iter().any(|a| a == "--reload");
    println!("INFO:     Uvicorn running on http://{}:{} (Press CTRL+C to quit)", host, port);
    println!("INFO:     Started server process");
    println!("INFO:     Application: {}", app);
    if reload {
        println!("INFO:     Started reloader process");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fastapi".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "uvicorn" => run_uvicorn(&rest),
        _ => run_fastapi(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fastapi};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fastapi"), "fastapi");
        assert_eq!(basename(r"C:\bin\fastapi.exe"), "fastapi.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fastapi.exe"), "fastapi");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fastapi(&["--help".to_string()]), 0);
        assert_eq!(run_fastapi(&["-h".to_string()]), 0);
        let _ = run_fastapi(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fastapi(&[]);
    }
}
