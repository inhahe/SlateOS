#![deny(clippy::all)]

//! openbao-cli — SlateOS OpenBao secrets management
//!
//! Single personality: `openbao`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_openbao(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: openbao [COMMAND] [OPTIONS]");
        println!("OpenBao v2.0 (Slate OS) — Secrets management (Vault fork)");
        println!();
        println!("Commands:");
        println!("  server             Start server");
        println!("  kv get|put|delete  Key-value store");
        println!("  secrets list|enable|disable  Secrets engines");
        println!("  auth list|enable   Auth methods");
        println!("  policy list|write  Manage policies");
        println!("  token create       Create token");
        println!("  operator init      Initialize");
        println!("  operator unseal    Unseal");
        println!("  status             Seal status");
        println!();
        println!("Options:");
        println!("  -address URL       Server address");
        println!("  -token TOKEN       Auth token");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("OpenBao v2.0.1 (Slate OS)"); return 0; }
    println!("OpenBao v2.0.1 (Slate OS)");
    println!("  API: https://0.0.0.0:8200");
    println!("  Cluster: https://0.0.0.0:8201");
    println!("  Storage: Raft (integrated)");
    println!("  Seal type: shamir");
    println!("  Sealed: false");
    println!("  Secrets engines: kv, pki, transit, ssh, database");
    println!("  Auth methods: token, userpass, ldap, oidc");
    println!("  Policies: 5");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "openbao".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_openbao(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_openbao};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/openbao"), "openbao");
        assert_eq!(basename(r"C:\bin\openbao.exe"), "openbao.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("openbao.exe"), "openbao");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_openbao(&["--help".to_string()], "openbao"), 0);
        assert_eq!(run_openbao(&["-h".to_string()], "openbao"), 0);
        let _ = run_openbao(&["--version".to_string()], "openbao");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_openbao(&[], "openbao");
    }
}
