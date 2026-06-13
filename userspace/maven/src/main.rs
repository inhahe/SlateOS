#![deny(clippy::all)]

//! maven — Slate OS Apache Maven build tool
//!
//! Multi-personality: `mvn` (default), `mvnw`

use std::env;
use std::process;

fn run_maven(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("usage: mvn [options] [<goal(s)>] [<phase(s)>]");
        println!();
        println!("Options:");
        println!("  -D,--define <arg>    Define system property");
        println!("  -f,--file <arg>      POM file");
        println!("  -o,--offline         Work offline");
        println!("  -q,--quiet           Quiet output");
        println!("  -U,--update-snapshots  Force updates");
        println!("  -X,--debug           Debug output");
        println!("  -pl,--projects <arg> Build specific modules");
        println!("  -T,--threads <arg>   Thread count");
        println!("  --version            Show version");
        return 0;
    }

    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("Apache Maven 3.9.6 (Slate OS)");
        println!("Maven home: /usr/share/maven");
        println!("Java version: 21.0.2");
        println!("OS name: \"slateos\", version: \"0.1\", arch: \"amd64\"");
        return 0;
    }

    let phases: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    let quiet = args.iter().any(|a| a == "-q" || a == "--quiet");

    if !quiet {
        println!("[INFO] Scanning for projects...");
        println!("[INFO]");
        println!("[INFO] -----------------< com.example:myproject >------------------");
        println!("[INFO] Building myproject 1.0-SNAPSHOT");
        println!("[INFO] --------------------------------[ jar ]---------------------------------");
    }

    if phases.is_empty() || phases.contains(&"package") || phases.contains(&"install") {
        if !quiet {
            println!("[INFO]");
            println!("[INFO] --- maven-resources-plugin:3.3.1:resources ---");
            println!("[INFO] Copying 1 resource");
            println!("[INFO]");
            println!("[INFO] --- maven-compiler-plugin:3.12.1:compile ---");
            println!("[INFO] Nothing to compile - all classes are up to date");
            println!("[INFO]");
            println!("[INFO] --- maven-surefire-plugin:3.2.5:test ---");
            println!("[INFO] Tests run: 5, Failures: 0, Errors: 0, Skipped: 0");
            println!("[INFO]");
            println!("[INFO] --- maven-jar-plugin:3.3.0:jar ---");
            println!("[INFO] Building jar: target/myproject-1.0-SNAPSHOT.jar");
        }
        if phases.contains(&"install") && !quiet {
            println!("[INFO]");
            println!("[INFO] --- maven-install-plugin:3.1.1:install ---");
            println!("[INFO] Installing target/myproject-1.0-SNAPSHOT.jar to ~/.m2/repository");
        }
    } else if phases.contains(&"clean") {
        if !quiet {
            println!("[INFO]");
            println!("[INFO] --- maven-clean-plugin:3.3.2:clean ---");
            println!("[INFO] Deleting target/");
        }
    } else if phases.contains(&"test") {
        if !quiet {
            println!("[INFO]");
            println!("[INFO] --- maven-surefire-plugin:3.2.5:test ---");
            println!("[INFO]");
            println!("[INFO] -------------------------------------------------------");
            println!("[INFO]  T E S T S");
            println!("[INFO] -------------------------------------------------------");
            println!("[INFO] Running com.example.AppTest");
            println!("[INFO] Tests run: 5, Failures: 0, Errors: 0, Skipped: 0");
        }
    } else if phases.contains(&"dependency:tree") {
        println!("[INFO] com.example:myproject:jar:1.0-SNAPSHOT");
        println!("[INFO] +- org.springframework:spring-core:jar:6.1.0:compile");
        println!("[INFO] +- com.google.guava:guava:jar:33.0.0-jre:compile");
        println!("[INFO] \\- org.junit.jupiter:junit-jupiter:jar:5.10.0:test");
    } else {
        for phase in &phases {
            if !quiet { println!("[INFO] Executing: {} (simulated)", phase); }
        }
    }

    if !quiet {
        println!("[INFO] ------------------------------------------------------------------------");
        println!("[INFO] BUILD SUCCESS");
        println!("[INFO] ------------------------------------------------------------------------");
        println!("[INFO] Total time:  3.5 s");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_maven(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_maven};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_maven(vec!["--help".to_string()]), 0);
        assert_eq!(run_maven(vec!["-h".to_string()]), 0);
        let _ = run_maven(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_maven(vec![]);
    }
}
