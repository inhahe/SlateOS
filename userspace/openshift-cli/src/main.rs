#![deny(clippy::all)]

//! openshift-cli — SlateOS Red Hat OpenShift Container Platform (oc)
//!
//! Single personality: `openshift`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_oc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: openshift [OPTIONS] [SUBCMD]");
        println!("Red Hat OpenShift Container Platform 4.17 / oc 4.17 (SlateOS)");
        println!();
        println!("Options:");
        println!("  oc login URL           Login to OCP cluster");
        println!("  oc new-app SOURCE      Create app from Git source (S2I)");
        println!("  oc rollout              Manage rollouts");
        println!("  oc adm                 Cluster administration");
        println!("  --rosa                 Red Hat OpenShift on AWS");
        println!("  --aro                  Azure Red Hat OpenShift");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Client Version: 4.17.4 / Kubernetes Version: v1.30.5 / Server Version: 4.17.4 (SlateOS)"); return 0; }
    println!("Red Hat OpenShift Container Platform 4.17 (SlateOS)");
    println!("  Foundation: Kubernetes 1.30 + Red Hat opinionated additions");
    println!("  CRI-O container runtime (not Docker); RHEL CoreOS host OS");
    println!("  S2I: Source-to-Image — Git URL → built container image");
    println!("  Routes: native ingress with HAProxy + Edge/Reencrypt/Passthrough TLS");
    println!("  Operators: OperatorHub catalog, certified operators, operator-sdk");
    println!("  Pipelines: OpenShift Pipelines (Tekton), GitOps (Argo CD)");
    println!("  Service Mesh: OpenShift Service Mesh (Istio + Kiali + Jaeger)");
    println!("  Storage: ODF (OpenShift Data Foundation, Ceph-based)");
    println!("  Editions: OCP (self-managed), ROSA (AWS), ARO (Azure), OSD (Dedicated)");
    println!("  License: Red Hat subscription (per-core or per-socket)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "openshift".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_oc(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_oc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/openshift"), "openshift");
        assert_eq!(basename(r"C:\bin\openshift.exe"), "openshift.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("openshift.exe"), "openshift");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_oc(&["--help".to_string()], "openshift"), 0);
        assert_eq!(run_oc(&["-h".to_string()], "openshift"), 0);
        let _ = run_oc(&["--version".to_string()], "openshift");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_oc(&[], "openshift");
    }
}
