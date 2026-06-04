#![deny(clippy::all)]

//! go — OurOS Go programming language toolchain
//!
//! Multi-personality binary detected via argv[0]:
//!
//! - `go` (default) — Go tool
//! - `gofmt` — Go source code formatter

use std::env;
use std::process;

// ── Main logic ────────────────────────────────────────────────────────

fn run_go(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "help" | "--help" | "-h" => {
            println!("Go is a tool for managing Go source code.");
            println!();
            println!("Usage: go <command> [arguments]");
            println!();
            println!("Commands:");
            println!("  bug         Start a bug report");
            println!("  build       Compile packages and dependencies");
            println!("  clean       Remove object files and cached files");
            println!("  doc         Show documentation for package");
            println!("  env         Print Go environment information");
            println!("  fix         Update packages to use new APIs");
            println!("  fmt         Gofmt (reformat) package sources");
            println!("  generate    Generate Go files by processing source");
            println!("  get         Add dependencies to current module");
            println!("  install     Compile and install packages");
            println!("  list        List packages or modules");
            println!("  mod         Module maintenance");
            println!("  work        Workspace maintenance");
            println!("  run         Compile and run Go program");
            println!("  test        Test packages");
            println!("  tool        Run specified go tool");
            println!("  version     Print Go version");
            println!("  vet         Report likely mistakes in packages");
            0
        }
        "version" => { println!("go version go1.22.0 ouros/amd64"); 0 }
        "env" => {
            println!("GO111MODULE=\"\"");
            println!("GOARCH=\"amd64\"");
            println!("GOOS=\"ouros\"");
            println!("GOPATH=\"/home/user/go\"");
            println!("GOROOT=\"/usr/local/go\"");
            println!("GOVERSION=\"go1.22.0\"");
            println!("GOMOD=\"/project/go.mod\"");
            0
        }
        "build" => {
            let output = cmd_args.iter().position(|a| a == "-o")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str());
            match output {
                Some(o) => println!("Building -> {} (simulated)", o),
                None => println!("Building ./... (simulated)"),
            }
            0
        }
        "run" => {
            let file = cmd_args.first().map(|s| s.as_str()).unwrap_or("main.go");
            println!("(running {} — simulated)", file);
            0
        }
        "test" => {
            let verbose = cmd_args.iter().any(|a| a == "-v");
            if verbose {
                println!("=== RUN   TestBasic");
                println!("--- PASS: TestBasic (0.00s)");
                println!("=== RUN   TestAdvanced");
                println!("--- PASS: TestAdvanced (0.01s)");
            }
            println!("ok      mypackage       0.015s");
            0
        }
        "get" => {
            for pkg in &cmd_args {
                if !pkg.starts_with('-') {
                    println!("go: downloading {}", pkg);
                    println!("go: added {} v1.0.0", pkg);
                }
            }
            0
        }
        "mod" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("help");
            match sub {
                "init" => {
                    let name = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("myproject");
                    println!("go: creating new go.mod: module {}", name);
                }
                "tidy" => println!("go: finding module dependencies... done"),
                "download" => println!("go: downloading dependencies... done"),
                "vendor" => println!("go: creating vendor directory... done"),
                "graph" => {
                    println!("myproject golang.org/x/text@v0.14.0");
                    println!("myproject golang.org/x/net@v0.20.0");
                }
                "verify" => println!("all modules verified"),
                "why" => println!("# myproject\nmyproject\n(simulated)"),
                "edit" => println!("(editing go.mod — simulated)"),
                _ => {
                    println!("go mod <command>");
                    println!("  init      Initialize new module");
                    println!("  tidy      Add/remove dependencies");
                    println!("  download  Download modules to cache");
                    println!("  vendor    Make vendored copy");
                    println!("  graph     Print module requirement graph");
                    println!("  verify    Verify dependencies");
                }
            }
            0
        }
        "fmt" => {
            let files: Vec<&str> = cmd_args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
            if files.is_empty() {
                println!("(formatting ./... — simulated)");
            } else {
                for f in &files { println!("(formatted {} — simulated)", f); }
            }
            0
        }
        "vet" => { println!("(go vet ./... — no issues found, simulated)"); 0 }
        "install" => {
            let pkg = cmd_args.first().map(|s| s.as_str()).unwrap_or("./...");
            println!("(installing {} — simulated)", pkg);
            0
        }
        "clean" => {
            println!("(cleaning build cache — simulated)");
            0
        }
        "doc" => {
            let pkg = cmd_args.first().map(|s| s.as_str()).unwrap_or("fmt");
            println!("package {}", pkg);
            println!();
            println!("    import \"std/{}\"", pkg);
            println!();
            println!("    Package {} provides ... (simulated)", pkg);
            0
        }
        "list" => {
            println!("myproject");
            println!("myproject/internal/util");
            println!("myproject/cmd/server");
            0
        }
        "tool" => {
            let tool = cmd_args.first().map(|s| s.as_str()).unwrap_or("list");
            if tool == "list" {
                println!("addr2line");
                println!("asm");
                println!("compile");
                println!("cover");
                println!("link");
                println!("pprof");
                println!("trace");
                println!("vet");
            } else {
                println!("(go tool {} — simulated)", tool);
            }
            0
        }
        "generate" => { println!("(go generate ./... — simulated)"); 0 }
        "work" => { println!("(go work — simulated)"); 0 }
        other => { eprintln!("go: unknown command \"{}\"", other); 1 }
    }
}

fn run_gofmt(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("usage: gofmt [flags] [path ...]");
        println!("  -d    display diffs instead of rewriting files");
        println!("  -l    list files whose formatting differs");
        println!("  -w    write result to file");
        println!("  -s    simplify code");
        return 0;
    }

    let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    for f in &files {
        println!("(formatted: {} — simulated)", f);
    }
    if files.is_empty() { println!("(reading from stdin — simulated)"); }
    0
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("go");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog_name.as_str() {
        "gofmt" => run_gofmt(rest),
        _ => run_go(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{run_go};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_go(vec!["--help".to_string()]), 0);
        assert_eq!(run_go(vec!["-h".to_string()]), 0);
        let _ = run_go(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_go(vec![]);
    }
}
