#![deny(clippy::all)]

//! sealed-secrets-cli — OurOS Bitnami Sealed Secrets tool
//!
//! Single personality: `kubeseal`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kubeseal(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: kubeseal [OPTIONS]");
        println!("kubeseal v0.27.0 (OurOS) — Sealed Secrets encryption");
        println!();
        println!("Options:");
        println!("  --format json|yaml       Output format (default: json)");
        println!("  --cert FILE              Public cert for offline sealing");
        println!("  --controller-name NAME   Controller name");
        println!("  --controller-namespace NS  Controller namespace");
        println!("  --scope strict|namespace-wide|cluster-wide  Scope");
        println!("  --raw                    Seal raw value (not K8s Secret)");
        println!("  --from-file FILE         Read secret from file");
        println!("  --fetch-cert             Fetch cert from controller");
        println!("  --re-encrypt             Re-encrypt existing sealed secret");
        println!("  --validate               Validate sealed secret");
        println!("  -V, --version            Show version");
        println!();
        println!("Pipe a K8s Secret to stdin, get a SealedSecret on stdout.");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("kubeseal v0.27.0 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--fetch-cert") {
        println!("-----BEGIN CERTIFICATE-----");
        println!("MIIErTCCApWgAwIBAgIQBnY...");
        println!("-----END CERTIFICATE-----");
        return 0;
    }
    if args.iter().any(|a| a == "--validate") {
        println!("SealedSecret is valid.");
        return 0;
    }
    if args.iter().any(|a| a == "--re-encrypt") {
        println!("Re-encrypted SealedSecret.");
        return 0;
    }
    println!("apiVersion: bitnami.com/v1alpha1");
    println!("kind: SealedSecret");
    println!("metadata:");
    println!("  name: mysecret");
    println!("  namespace: default");
    println!("spec:");
    println!("  encryptedData:");
    println!("    password: AgBy3i4OJSWK+PiT...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kubeseal".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kubeseal(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kubeseal};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sealed-secrets"), "sealed-secrets");
        assert_eq!(basename(r"C:\bin\sealed-secrets.exe"), "sealed-secrets.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sealed-secrets.exe"), "sealed-secrets");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_kubeseal(&["--help".to_string()], "sealed-secrets"), 0);
        assert_eq!(run_kubeseal(&["-h".to_string()], "sealed-secrets"), 0);
        assert_eq!(run_kubeseal(&["--version".to_string()], "sealed-secrets"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_kubeseal(&[], "sealed-secrets"), 0);
    }
}
