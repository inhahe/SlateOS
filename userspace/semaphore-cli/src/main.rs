#![deny(clippy::all)]

//! semaphore-cli — SlateOS Semaphore CI/CD (Rendered Text, performance-focused)
//!
//! Single personality: `semaphore`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sem(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: semaphore [OPTIONS]");
        println!("Semaphore CI 2.0 (SlateOS) — Fast continuous integration");
        println!();
        println!("Options:");
        println!("  jobs                   List jobs");
        println!("  pipelines              List pipelines");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Semaphore CI 2.0 (SlateOS)"); return 0; }
    println!("Semaphore CI 2.0 (SlateOS)");
    println!("  Vendor: Rendered Text d.o.o. (Novi Sad, Serbia + Brooklyn, NY)");
    println!("  Founders: Marko Anastasov + Aleksandar Diklic (Serbian dev shop turned product co.)");
    println!("  History: started as Rendered Text — Rails consultancy 2009");
    println!("          launched Semaphore 1.0 in 2012 (first 'fast' hosted CI for Rails)");
    println!("          Semaphore 2.0 launched 2018 — full rewrite, YAML config, fan-out pipelines");
    println!("  Pricing: Free tier — 1,300 minutes/mo for OSS + private");
    println!("          Startup $20/mo, Boutique $99/mo, Scale custom");
    println!("          unique: pay for capacity (machines) not minutes — predictable bills");
    println!("  Performance angle: marketed as 'fastest CI on the market'");
    println!("                     parallel job graphs, fast Linux/macOS machines, fast startup");
    println!("                     custom Docker images cached layer-by-layer");
    println!("  Features:");
    println!("    - YAML pipelines + visual editor (Semaphore Visual Builder)");
    println!("    - Fan-in/fan-out job graphs (DAG-based)");
    println!("    - Built-in test reporters (JUnit XML auto-parsing + flaky test detection)");
    println!("    - Code review integrations: GitHub + Bitbucket + GitLab status checks");
    println!("    - Machines: Linux Ubuntu 20.04/22.04, macOS Xcode 14/15/16, Apple Silicon");
    println!("    - Cache + artifacts API + parameterized workflows");
    println!("    - 'Deliver' — deployment dashboards with approval gates");
    println!("    - Semaphore CLI (`sem`) — local dev + scripting");
    println!("  Strong communities: Ruby, Elixir, Crystal — first-class language support since Rails-era");
    println!("                      'best CI for Phoenix/Elixir' reputation");
    println!("  Critique: smaller mindshare than GitHub Actions / GitLab CI");
    println!("           UI sometimes less polished than competitors");
    println!("  Differentiator: capacity-priced billing model + fastest cold-start + Elixir/Ruby community");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "semaphore".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sem(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sem};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/semaphore"), "semaphore");
        assert_eq!(basename(r"C:\bin\semaphore.exe"), "semaphore.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("semaphore.exe"), "semaphore");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sem(&["--help".to_string()], "semaphore"), 0);
        assert_eq!(run_sem(&["-h".to_string()], "semaphore"), 0);
        let _ = run_sem(&["--version".to_string()], "semaphore");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sem(&[], "semaphore");
    }
}
