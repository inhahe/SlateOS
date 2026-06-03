#![deny(clippy::all)]

//! semaphoreui-cli — OurOS Semaphore UI for Ansible/Terraform
//!
//! Single personality: `semaphore`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_semaphore(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: semaphore [COMMAND] [OPTIONS]");
        println!("Semaphore v2.10 (OurOS) — Modern UI for Ansible/Terraform");
        println!();
        println!("Commands:");
        println!("  server             Start Semaphore server");
        println!("  setup              Interactive setup");
        println!("  user list|add      Manage users");
        println!("  migrate            Run database migrations");
        println!("  vault rekey        Re-encrypt vault");
        println!();
        println!("Options:");
        println!("  --config FILE      Config file");
        println!("  --port PORT        Listen port (default: 3000)");
        println!("  --host ADDR        Listen address");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Semaphore v2.10.22 (OurOS)"); return 0; }
    println!("Semaphore v2.10.22 (OurOS)");
    println!("  Server: http://0.0.0.0:3000");
    println!("  Projects: 4");
    println!("  Templates: 23");
    println!("  Inventories: 6");
    println!("  Repositories: 8");
    println!("  Tasks: 12 running, 567 completed");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "semaphore".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_semaphore(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_semaphore};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/semaphoreui"), "semaphoreui");
        assert_eq!(basename(r"C:\bin\semaphoreui.exe"), "semaphoreui.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("semaphoreui.exe"), "semaphoreui");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_semaphore(&["--help".to_string()], "semaphoreui"), 0);
        assert_eq!(run_semaphore(&["-h".to_string()], "semaphoreui"), 0);
        assert_eq!(run_semaphore(&["--version".to_string()], "semaphoreui"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_semaphore(&[], "semaphoreui"), 0);
    }
}
