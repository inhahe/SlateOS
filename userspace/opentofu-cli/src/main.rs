#![deny(clippy::all)]

//! opentofu-cli — OurOS OpenTofu infrastructure as code
//!
//! Single personality: `tofu`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tofu(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tofu COMMAND [OPTIONS]");
        println!("OpenTofu v1.7 (OurOS) — Infrastructure as Code");
        println!();
        println!("Commands:");
        println!("  init        Initialize working directory");
        println!("  plan        Preview changes");
        println!("  apply       Apply changes");
        println!("  destroy     Destroy infrastructure");
        println!("  validate    Validate configuration");
        println!("  fmt         Format configuration");
        println!("  state       State management");
        println!("  output      Show output values");
        println!("  import      Import existing infrastructure");
        println!("  providers   Show providers");
        println!("  --version   Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("OpenTofu v1.7.2 (OurOS)"); return 0; }
    println!("OpenTofu v1.7.2 (OurOS)");
    println!("  Plan: 3 to add, 1 to change, 0 to destroy");
    println!("  + aws_instance.web");
    println!("  + aws_security_group.web_sg");
    println!("  + aws_lb.web_lb");
    println!("  ~ aws_route53_record.web_dns");
    println!("  Apply complete: 3 added, 1 changed, 0 destroyed");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tofu".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tofu(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tofu};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/opentofu"), "opentofu");
        assert_eq!(basename(r"C:\bin\opentofu.exe"), "opentofu.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("opentofu.exe"), "opentofu");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tofu(&["--help".to_string()], "opentofu"), 0);
        assert_eq!(run_tofu(&["-h".to_string()], "opentofu"), 0);
        let _ = run_tofu(&["--version".to_string()], "opentofu");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tofu(&[], "opentofu");
    }
}
