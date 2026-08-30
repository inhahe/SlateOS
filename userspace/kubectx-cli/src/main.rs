#![deny(clippy::all)]

//! kubectx-cli — Slate OS kubectx/kubens context and namespace switcher
//!
//! Two personalities: `kubectx`, `kubens`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kubectx(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kubectx [CONTEXT | -c | -d NAME | -]");
        println!("kubectx (Slate OS) — Switch between kubectl contexts");
        println!();
        println!("Options:");
        println!("  (no args)       List contexts (current marked with *)");
        println!("  CONTEXT         Switch to context");
        println!("  -               Switch to previous context");
        println!("  -c, --current   Show current context");
        println!("  -d NAME         Delete context");
        println!("  -u, --unset     Unset current context");
        return 0;
    }
    if args.is_empty() {
        println!("  docker-desktop");
        println!("* minikube");
        println!("  production");
        println!("  staging");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "-c" | "--current" => println!("minikube"),
        "-" => println!("Switched to context \"docker-desktop\"."),
        "-d" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("unknown");
            println!("Deleted context \"{}\".", name);
        }
        "-u" | "--unset" => println!("Current context unset."),
        ctx => println!("Switched to context \"{}\".", ctx),
    }
    0
}

fn run_kubens(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kubens [NAMESPACE | -c | -]");
        println!("kubens (Slate OS) — Switch between Kubernetes namespaces");
        println!();
        println!("Options:");
        println!("  (no args)       List namespaces (current marked with *)");
        println!("  NAMESPACE       Switch to namespace");
        println!("  -               Switch to previous namespace");
        println!("  -c, --current   Show current namespace");
        return 0;
    }
    if args.is_empty() {
        println!("* default");
        println!("  kube-system");
        println!("  kube-public");
        println!("  kube-node-lease");
        println!("  monitoring");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "-c" | "--current" => println!("default"),
        "-" => println!("Active namespace is \"kube-system\"."),
        ns => println!("Active namespace is \"{}\".", ns),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kubectx".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "kubens" => run_kubens(&rest, &prog),
        _ => run_kubectx(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kubectx};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kubectx"), "kubectx");
        assert_eq!(basename(r"C:\bin\kubectx.exe"), "kubectx.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kubectx.exe"), "kubectx");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_kubectx(&["--help".to_string()], "kubectx"), 0);
        assert_eq!(run_kubectx(&["-h".to_string()], "kubectx"), 0);
        let _ = run_kubectx(&["--version".to_string()], "kubectx");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_kubectx(&[], "kubectx");
    }
}
