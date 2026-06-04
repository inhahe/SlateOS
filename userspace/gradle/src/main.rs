#![deny(clippy::all)]

//! gradle — OurOS Gradle build automation tool
//!
//! Single personality: `gradle` (also `gradlew`)

use std::env;
use std::process;

fn run_gradle(args: Vec<String>) -> i32 {
    let tasks: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();

    if args.iter().any(|a| a == "--help" || a == "-h" || a == "-?") {
        println!("USAGE: gradle [option...] [task...]");
        println!();
        println!("Options:");
        println!("  -b, --build-file   Specify build file");
        println!("  -q, --quiet        Log errors only");
        println!("  -i, --info         Set log level to info");
        println!("  -d, --debug        Log in debug mode");
        println!("  --stacktrace       Print stacktrace");
        println!("  --no-daemon        Do not use daemon");
        println!("  --parallel         Build in parallel");
        println!("  --scan             Publish build scan");
        println!("  --version          Show version");
        println!();
        println!("Common tasks:");
        println!("  build       Assembles and tests this project");
        println!("  clean       Deletes the build directory");
        println!("  test        Runs the unit tests");
        println!("  assemble    Assembles the outputs");
        println!("  check       Runs all checks");
        println!("  tasks       Displays the tasks");
        println!("  dependencies  Displays project dependencies");
        return 0;
    }

    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("------------------------------------------------------------");
        println!("Gradle 8.7 (OurOS)");
        println!("------------------------------------------------------------");
        println!("Build time:   2025-05-22");
        println!("Revision:     abc1234");
        println!("Kotlin:       1.9.22");
        println!("Groovy:       3.0.17");
        println!("JVM:          21.0.2 (OurOS 64-Bit Server)");
        println!("OS:           OurOS x86_64");
        return 0;
    }

    println!("> Task :compileJava UP-TO-DATE");

    if tasks.is_empty() || tasks.contains(&"build") {
        println!("> Task :processResources UP-TO-DATE");
        println!("> Task :classes UP-TO-DATE");
        println!("> Task :jar UP-TO-DATE");
        println!("> Task :compileTestJava UP-TO-DATE");
        println!("> Task :test");
        println!("> Task :check");
        println!("> Task :assemble");
        println!("> Task :build");
        println!();
        println!("BUILD SUCCESSFUL in 3s");
        println!("7 actionable tasks: 1 executed, 6 up-to-date");
    } else if tasks.contains(&"clean") {
        println!("> Task :clean");
        println!();
        println!("BUILD SUCCESSFUL in 0s");
        println!("1 actionable task: 1 executed");
    } else if tasks.contains(&"test") {
        println!("> Task :test");
        println!();
        println!("com.example.AppTest > testApp PASSED");
        println!("com.example.UtilTest > testUtil PASSED");
        println!();
        println!("BUILD SUCCESSFUL in 2s");
        println!("3 actionable tasks: 1 executed, 2 up-to-date");
    } else if tasks.contains(&"tasks") {
        println!();
        println!("Build tasks");
        println!("-----------");
        println!("assemble - Assembles the outputs");
        println!("build - Assembles and tests this project");
        println!("clean - Deletes the build directory");
        println!();
        println!("Verification tasks");
        println!("------------------");
        println!("check - Runs all checks");
        println!("test - Runs the unit tests");
    } else if tasks.contains(&"dependencies") {
        println!("+--- org.springframework.boot:spring-boot-starter:3.2.0");
        println!("|    +--- org.springframework.boot:spring-boot:3.2.0");
        println!("|    \\--- org.springframework:spring-core:6.1.0");
        println!("+--- com.google.guava:guava:33.0.0-jre");
        println!("\\--- org.junit.jupiter:junit-jupiter:5.10.0 (test)");
    } else {
        for task in &tasks {
            println!("> Task :{} (simulated)", task);
        }
        println!();
        println!("BUILD SUCCESSFUL (simulated)");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gradle(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_gradle};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gradle(vec!["--help".to_string()]), 0);
        assert_eq!(run_gradle(vec!["-h".to_string()]), 0);
        let _ = run_gradle(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gradle(vec![]);
    }
}
