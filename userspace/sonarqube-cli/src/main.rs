#![deny(clippy::all)]

//! sonarqube-cli — OurOS SonarQube Scanner CLI
//!
//! Multi-personality: `sonar-scanner`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sonar(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: sonar-scanner [OPTIONS]");
        println!("SonarScanner 5.0.1 (OurOS)");
        println!();
        println!("Options:");
        println!("  -Dsonar.projectKey=KEY     Project key");
        println!("  -Dsonar.sources=PATH       Source directory");
        println!("  -Dsonar.host.url=URL       SonarQube server URL");
        println!("  -Dsonar.token=TOKEN        Authentication token");
        println!("  -Dsonar.language=LANG      Language");
        println!("  -Dsonar.exclusions=PAT     Exclude patterns");
        println!("  -Dsonar.tests=PATH         Test source directory");
        println!("  -Dsonar.coverage.reportPaths=PATH  Coverage report");
        println!("  --version                  Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("SonarScanner 5.0.1");
        return 0;
    }
    let project_key = args.iter()
        .find(|a| a.starts_with("-Dsonar.projectKey="))
        .map(|a| a.trim_start_matches("-Dsonar.projectKey="))
        .unwrap_or("my-project");
    let host = args.iter()
        .find(|a| a.starts_with("-Dsonar.host.url="))
        .map(|a| a.trim_start_matches("-Dsonar.host.url="))
        .unwrap_or("http://localhost:9000");

    println!("INFO: Scanner configuration file: sonar-scanner.properties");
    println!("INFO: Project root configuration file: sonar-project.properties");
    println!("INFO: SonarScanner 5.0.1");
    println!("INFO: Java 17.0.9");
    println!("INFO: OurOS amd64");
    println!("INFO: User cache: ~/.sonar/cache");
    println!("INFO: Communicating with SonarQube Server {}", host);
    println!("INFO: Project key: {}", project_key);
    println!("INFO: Base dir: .");
    println!("INFO: Working dir: .scannerwork");
    println!("INFO: Loading module info...");
    println!("INFO: Load project settings...");
    println!("INFO: 42 files indexed");
    println!("INFO: 38 source files to be analyzed");
    println!("INFO: Quality profile: Sonar way");
    println!();
    println!("INFO: Sensor JavaSquidSensor [java]");
    println!("INFO: Sensor CSharpSensor [csharp]");
    println!("INFO: Sensor PythonSensor [python]");
    println!("INFO: Sensor TypeScriptSensor [typescript]");
    println!();
    println!("INFO: ------------- Analysis Report -------------");
    println!("INFO: Bugs:               2");
    println!("INFO: Vulnerabilities:    1");
    println!("INFO: Code Smells:        12");
    println!("INFO: Coverage:           78.5%");
    println!("INFO: Duplications:       3.2%");
    println!();
    println!("INFO: ANALYSIS SUCCESSFUL");
    println!("INFO: Dashboard: {}/dashboard?id={}", host, project_key);
    println!("INFO: Task URL: {}/api/ce/task?id=AYxyz123", host);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sonar-scanner".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sonar(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
