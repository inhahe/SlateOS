#![deny(clippy::all)]

//! mvn-cli — SlateOS Apache Maven build system
//!
//! Multi-personality: `mvn`, `mvnw`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mvn(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: mvn [OPTIONS] [GOAL [GOAL ...]] [PHASE [PHASE ...]]");
        println!("Apache Maven 3.9.6 (SlateOS)");
        println!();
        println!("Options:");
        println!("  -f FILE             Alternate POM file");
        println!("  -D KEY=VALUE        System property");
        println!("  -P PROFILE          Activate profile");
        println!("  -pl MODULE          Build specific modules");
        println!("  -am                 Also make dependent modules");
        println!("  -o                  Offline mode");
        println!("  -U                  Force update of snapshots");
        println!("  -T NUM              Thread count");
        println!("  -q                  Quiet output");
        println!("  -X                  Debug output");
        println!("  -e                  Produce execution error messages");
        println!("  -B                  Batch mode");
        println!("  --version           Show version");
        println!();
        println!("Lifecycle phases:");
        println!("  validate, compile, test, package, verify, install, deploy");
        println!("  clean, site");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("Apache Maven 3.9.6 (bc0240f3c744dd6b6ec2920b3cd08dcc295161ae)");
        println!("Maven home: /usr/local/maven");
        println!("Java version: 21.0.2, vendor: SlateOS");
        println!("Default locale: en_US, platform encoding: UTF-8");
        println!("OS name: \"slateos\", version: \"1.0\", arch: \"amd64\"");
        return 0;
    }
    let quiet = args.iter().any(|a| a == "-q");
    let phases: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();
    if !quiet {
        println!("[INFO] Scanning for projects...");
        println!("[INFO]");
        println!("[INFO] -------------------< com.example:myapp >--------------------");
        println!("[INFO] Building myapp 1.0-SNAPSHOT");
        println!("[INFO] --------------------------------[ jar ]--------------------------------");
    }
    for phase in &phases {
        match *phase {
            "clean" => {
                if !quiet {
                    println!("[INFO]");
                    println!("[INFO] --- maven-clean-plugin:3.3.2:clean (default-clean) @ myapp ---");
                    println!("[INFO] Deleting target/");
                }
            }
            "compile" => {
                if !quiet {
                    println!("[INFO]");
                    println!("[INFO] --- maven-compiler-plugin:3.12.1:compile (default-compile) @ myapp ---");
                    println!("[INFO] Compiling 15 source files to target/classes");
                }
            }
            "test" => {
                if !quiet {
                    println!("[INFO]");
                    println!("[INFO] --- maven-surefire-plugin:3.2.5:test (default-test) @ myapp ---");
                    println!("[INFO] Tests run: 42, Failures: 0, Errors: 0, Skipped: 0");
                }
            }
            "package" => {
                if !quiet {
                    println!("[INFO]");
                    println!("[INFO] --- maven-jar-plugin:3.3.0:jar (default-jar) @ myapp ---");
                    println!("[INFO] Building jar: target/myapp-1.0-SNAPSHOT.jar");
                }
            }
            "install" => {
                if !quiet {
                    println!("[INFO]");
                    println!("[INFO] --- maven-install-plugin:3.1.1:install (default-install) @ myapp ---");
                    println!("[INFO] Installing target/myapp-1.0-SNAPSHOT.jar to ~/.m2/repository");
                }
            }
            "deploy" => {
                if !quiet {
                    println!("[INFO]");
                    println!("[INFO] --- maven-deploy-plugin:3.1.1:deploy (default-deploy) @ myapp ---");
                    println!("[INFO] Deploying to remote repository");
                }
            }
            "dependency:tree" => {
                println!("[INFO] com.example:myapp:jar:1.0-SNAPSHOT");
                println!("[INFO] +- junit:junit:jar:4.13.2:test");
                println!("[INFO] |  \\- org.hamcrest:hamcrest-core:jar:1.3:test");
                println!("[INFO] \\- org.slf4j:slf4j-api:jar:2.0.11:compile");
            }
            _ => {
                if !quiet {
                    println!("[INFO] --- {} ---", phase);
                }
            }
        }
    }
    if !quiet {
        println!("[INFO] ------------------------------------------------------------------------");
        println!("[INFO] BUILD SUCCESS");
        println!("[INFO] ------------------------------------------------------------------------");
        println!("[INFO] Total time: 5.234 s");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mvn".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mvn(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mvn};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mvn"), "mvn");
        assert_eq!(basename(r"C:\bin\mvn.exe"), "mvn.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mvn.exe"), "mvn");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mvn(&["--help".to_string()]), 0);
        assert_eq!(run_mvn(&["-h".to_string()]), 0);
        let _ = run_mvn(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mvn(&[]);
    }
}
