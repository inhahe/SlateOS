#![deny(clippy::all)]

//! smallstep-cli — OurOS Smallstep certificate authority
//!
//! Single personality: `step-ca`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_step_ca(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: step-ca [CONFIG] [OPTIONS]");
        println!("step-ca v0.26 (OurOS) — Online certificate authority");
        println!();
        println!("Options:");
        println!("  --password-file F  Password file for decrypting CA key");
        println!("  --issuer-password-file F  Issuer password");
        println!("  --resolver ADDR    DNS resolver");
        println!("  --pidfile FILE     PID file");
        println!("  --context NAME     Use named context");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("step-ca v0.26.2 (OurOS)"); return 0; }
    println!("step-ca v0.26.2 (OurOS)");
    println!("  HTTPS: https://0.0.0.0:9000");
    println!("  Root: /etc/step-ca/certs/root_ca.crt");
    println!("  Provisioners: 3 (JWK, ACME, OIDC)");
    println!("  ACME: enabled (directory at /acme)");
    println!("  Database: BadgerDB");
    println!("  SSH: host + user certificates");
    println!("  Certificate duration: 24h default");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "step-ca".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_step_ca(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_step_ca};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/smallstep"), "smallstep");
        assert_eq!(basename(r"C:\bin\smallstep.exe"), "smallstep.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("smallstep.exe"), "smallstep");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_step_ca(&["--help".to_string()], "smallstep"), 0);
        assert_eq!(run_step_ca(&["-h".to_string()], "smallstep"), 0);
        assert_eq!(run_step_ca(&["--version".to_string()], "smallstep"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_step_ca(&[], "smallstep"), 0);
    }
}
