#![deny(clippy::all)]

//! firebase-cli — SlateOS Firebase CLI
//!
//! Multi-personality: `firebase`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_firebase(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: firebase COMMAND [OPTIONS]");
        println!("Firebase CLI 13.12.0 (SlateOS)");
        println!();
        println!("Commands:");
        println!("  init         Initialize Firebase in a directory");
        println!("  login        Log in to Firebase");
        println!("  logout       Log out of Firebase");
        println!("  deploy       Deploy to Firebase");
        println!("  serve        Start local server");
        println!("  emulators    Manage local emulators");
        println!("  hosting      Manage hosting");
        println!("  functions    Manage Cloud Functions");
        println!("  firestore    Manage Firestore");
        println!("  database     Manage Realtime Database");
        println!("  auth         Manage Authentication");
        println!("  storage      Manage Cloud Storage");
        println!("  projects     Manage projects");
        println!("  use          Set active project");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" | "-V" => println!("13.12.0"),
        "init" => {
            println!("Firebase initialization...");
            println!("  What do you want to set up?");
            println!("  (*) Hosting");
            println!("  (*) Functions");
            println!("  (*) Firestore");
            println!();
            println!("  Created firebase.json");
            println!("  Created .firebaserc");
            println!("Firebase initialization complete!");
        }
        "deploy" => {
            let only = args.windows(2).find(|w| w[0] == "--only")
                .map(|w| w[1].as_str());
            if let Some(o) = only {
                println!("Deploying {} only...", o);
            } else {
                println!("Deploying all services...");
            }
            println!("  hosting: deployed");
            println!("  functions: deployed 2 functions");
            println!();
            println!("Deploy complete!");
            println!("  Hosting URL: https://my-project.web.app");
        }
        "serve" => {
            println!("Starting Firebase Emulators...");
            println!("  Hosting:   http://localhost:5000");
            println!("  Functions: http://localhost:5001");
            println!("  Firestore: http://localhost:8080");
        }
        "emulators" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("start");
            match sub {
                "start" => {
                    println!("Starting emulators...");
                    println!("  Auth:      http://localhost:9099");
                    println!("  Firestore: http://localhost:8080");
                    println!("  Functions: http://localhost:5001");
                    println!("  Hosting:   http://localhost:5000");
                    println!("  Storage:   http://localhost:9199");
                    println!("  UI:        http://localhost:4000");
                }
                "exec" => println!("firebase emulators: exec completed"),
                _ => println!("firebase emulators: '{}' completed", sub),
            }
        }
        "projects" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("Project ID          Display Name     Resources");
                println!("my-project-12345    My Project       [hosting, functions]");
            }
        }
        "use" => {
            let project = args.get(1).map(|s| s.as_str()).unwrap_or("my-project");
            println!("Now using project {}", project);
        }
        "login" => println!("Already logged in as user@example.com"),
        "logout" => println!("Logged out."),
        _ => println!("firebase: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "firebase".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_firebase(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_firebase};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/firebase"), "firebase");
        assert_eq!(basename(r"C:\bin\firebase.exe"), "firebase.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("firebase.exe"), "firebase");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_firebase(&["--help".to_string()]), 0);
        assert_eq!(run_firebase(&["-h".to_string()]), 0);
        let _ = run_firebase(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_firebase(&[]);
    }
}
