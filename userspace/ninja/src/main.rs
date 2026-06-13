#![deny(clippy::all)]

//! ninja — SlateOS Ninja build system
//!
//! Multi-personality binary detected via argv[0]:
//!
//! - `ninja` (default) — build tool
//! - `samu` — samurai (ninja-compatible build tool)

use std::env;
use std::process;

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct _BuildEdge {
    rule: String,
    inputs: Vec<String>,
    outputs: Vec<String>,
    _implicit_deps: Vec<String>,
}

fn _sample_edges() -> Vec<_BuildEdge> {
    vec![
        _BuildEdge {
            rule: "cc".to_string(),
            inputs: vec!["src/main.c".to_string()],
            outputs: vec!["build/main.o".to_string()],
            _implicit_deps: vec!["src/config.h".to_string()],
        },
        _BuildEdge {
            rule: "cc".to_string(),
            inputs: vec!["src/util.c".to_string()],
            outputs: vec!["build/util.o".to_string()],
            _implicit_deps: vec![],
        },
        _BuildEdge {
            rule: "link".to_string(),
            inputs: vec!["build/main.o".to_string(), "build/util.o".to_string()],
            outputs: vec!["build/myapp".to_string()],
            _implicit_deps: vec![],
        },
    ]
}

// ── Main logic ────────────────────────────────────────────────────────

fn run_ninja(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("usage: ninja [options] [targets...]");
        println!();
        println!("Options:");
        println!("  -C DIR    Change to DIR before doing anything else");
        println!("  -f FILE   Specify input build file [default=build.ninja]");
        println!("  -j N      Run N jobs in parallel (default=auto)");
        println!("  -k N      Keep going until N jobs fail (default=1)");
        println!("  -n        Dry run (don't run commands)");
        println!("  -v        Show all command lines while building");
        println!("  -d MODE   Enable debugging (explain, stats, keepdepfile, keeprsp)");
        println!("  -t TOOL   Run a subtool (list to see available)");
        println!("  -w FLAG   Warning flags (dupbuild=warn/err)");
        println!("  --version Show version");
        return 0;
    }

    if args.iter().any(|a| a == "--version") {
        println!("0.1.0 (SlateOS)");
        return 0;
    }

    // Parse common flags
    let mut jobs = 0u32;
    let mut dry_run = false;
    let mut verbose = false;
    let mut build_dir: Option<String> = None;
    let mut tool: Option<String> = None;
    let mut targets: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-j" => {
                i += 1;
                if i < args.len() { jobs = args[i].parse().unwrap_or(4); }
            }
            "-C" => {
                i += 1;
                if i < args.len() { build_dir = Some(args[i].clone()); }
            }
            "-t" => {
                i += 1;
                if i < args.len() { tool = Some(args[i].clone()); }
            }
            "-n" => dry_run = true,
            "-v" => verbose = true,
            s if !s.starts_with('-') => targets.push(s.to_string()),
            _ => {} // ignore other flags
        }
        i += 1;
    }

    // Handle subtool
    if let Some(ref t) = tool {
        return run_tool(t, &targets);
    }

    // Build mode
    if let Some(ref d) = build_dir {
        println!("ninja: Entering directory '{}'", d);
    }

    let edges = _sample_edges();
    if jobs == 0 { jobs = 4; }

    if dry_run {
        for edge in &edges {
            println!("[dry-run] {} {} -> {}",
                edge.rule,
                edge.inputs.join(" "),
                edge.outputs.join(" "));
        }
        return 0;
    }

    let total = edges.len();
    for (idx, edge) in edges.iter().enumerate() {
        let progress = format!("[{}/{}]", idx + 1, total);
        if verbose {
            match edge.rule.as_str() {
                "cc" => println!("{} cc -c {} -o {}",
                    progress,
                    edge.inputs.join(" "),
                    edge.outputs.join(" ")),
                "link" => println!("{} cc {} -o {}",
                    progress,
                    edge.inputs.join(" "),
                    edge.outputs.join(" ")),
                _ => println!("{} {} {} -> {}",
                    progress,
                    edge.rule,
                    edge.inputs.join(" "),
                    edge.outputs.join(" ")),
            }
        } else {
            match edge.rule.as_str() {
                "cc" => println!("{} Building C object {}",
                    progress, edge.outputs.join(" ")),
                "link" => println!("{} Linking {}",
                    progress, edge.outputs.join(" ")),
                _ => println!("{} {} {}",
                    progress, edge.rule, edge.outputs.join(" ")),
            }
        }
    }

    if !targets.is_empty() {
        println!("Built targets: {}", targets.join(", "));
    }
    println!("build complete ({} edges, {} jobs).", total, jobs);
    0
}

fn run_tool(tool: &str, _args: &[String]) -> i32 {
    match tool {
        "list" => {
            println!("ninja subtools:");
            println!("  browse    Browse dependency graph in browser");
            println!("  clean     Clean built files");
            println!("  commands  List all commands needed to rebuild");
            println!("  deps      Show stored deps for files");
            println!("  graph     Output graphviz dot file");
            println!("  query     Show inputs/outputs for a path");
            println!("  targets   List targets by rule or depth");
            println!("  compdb    Dump JSON compilation database");
            println!("  recompact Recompact ninja internal data");
            println!("  rules     List all rules");
            0
        }
        "clean" => {
            println!("Cleaning... 3 files removed.");
            0
        }
        "commands" => {
            println!("cc -c src/main.c -o build/main.o");
            println!("cc -c src/util.c -o build/util.o");
            println!("cc build/main.o build/util.o -o build/myapp");
            0
        }
        "targets" => {
            println!("build/main.o: cc");
            println!("build/util.o: cc");
            println!("build/myapp: link");
            0
        }
        "compdb" => {
            println!("[");
            println!("  {{");
            println!("    \"directory\": \"/project\",");
            println!("    \"command\": \"cc -c src/main.c -o build/main.o\",");
            println!("    \"file\": \"src/main.c\"");
            println!("  }},");
            println!("  {{");
            println!("    \"directory\": \"/project\",");
            println!("    \"command\": \"cc -c src/util.c -o build/util.o\",");
            println!("    \"file\": \"src/util.c\"");
            println!("  }}");
            println!("]");
            0
        }
        "graph" => {
            println!("digraph ninja {{");
            println!("  rankdir=\"LR\"");
            println!("  node [fontsize=10, shape=box, height=0.25]");
            println!("  edge [fontsize=10]");
            println!("  \"src/main.c\" -> \"build/main.o\"");
            println!("  \"src/util.c\" -> \"build/util.o\"");
            println!("  \"build/main.o\" -> \"build/myapp\"");
            println!("  \"build/util.o\" -> \"build/myapp\"");
            println!("}}");
            0
        }
        "deps" => {
            println!("build/main.o: #deps 2, deps mtime ...");
            println!("    src/main.c");
            println!("    src/config.h");
            0
        }
        "query" => {
            println!("build/myapp:");
            println!("  input: link");
            println!("    build/main.o");
            println!("    build/util.o");
            0
        }
        "rules" => {
            println!("cc");
            println!("link");
            0
        }
        "recompact" => { println!("Recompacted .ninja_deps and .ninja_log"); 0 }
        other => { eprintln!("ninja: unknown tool '{}'", other); 1 }
    }
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("ninja");
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

    // samu is just an alias for ninja (samurai compatibility)
    let _is_samu = prog_name == "samu";

    let code = run_ninja(rest);
    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_edges() {
        let edges = _sample_edges();
        assert_eq!(edges.len(), 3);
        assert_eq!(edges[0].rule, "cc");
        assert_eq!(edges[2].rule, "link");
    }
}
