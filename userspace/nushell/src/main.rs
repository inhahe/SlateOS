#![deny(clippy::all)]

//! nushell — OurOS modern shell with structured data
//!
//! Single personality: `nu`

use std::env;
use std::process;

fn run_nu(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nu [OPTIONS] [FILE] [-- ARGS...]");
        println!();
        println!("A new type of shell that works with structured data.");
        println!();
        println!("Options:");
        println!("  -c, --commands <COMMANDS>   Run commands and then exit");
        println!("  --config <FILE>             Config file to use");
        println!("  --env-config <FILE>         Environment config file");
        println!("  -e, --execute <STRING>      Run a command string");
        println!("  -I, --include-path <DIR>    Add directory to module search");
        println!("  -l, --login                 Start as a login shell");
        println!("  -m, --table-mode <MODE>     Table display mode");
        println!("  -n, --no-config-file        Don't load config files");
        println!("  --no-history                Don't save command history");
        println!("  --no-std-lib                Don't load standard library");
        println!("  -t, --threads <NUM>         Number of threads");
        println!("  --stdin                     Read from stdin");
        println!("  --ide-ast                   IDE: return AST");
        println!("  --ide-check <SPAN>          IDE: check syntax at span");
        println!("  --ide-complete <POS>        IDE: get completions at position");
        println!("  --ide-goto-def <POS>        IDE: go to definition");
        println!("  --ide-hover <POS>           IDE: get hover info");
        println!("  --lsp                       Start LSP server");
        println!("  -V, --version               Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("nu 0.94.0 (OurOS)");
        return 0;
    }

    // Check for -c / --commands or -e / --execute
    let mut cmd_idx = None;
    for (i, a) in args.iter().enumerate() {
        if (a == "-c" || a == "--commands" || a == "-e" || a == "--execute")
            && i + 1 < args.len()
        {
            cmd_idx = Some(i + 1);
            break;
        }
    }

    if let Some(idx) = cmd_idx {
        let command = &args[idx];
        // Simulate some nushell structured data commands
        if command.contains("ls") {
            println!("╭───┬────────────┬──────┬──────────┬──────────────╮");
            println!("│ # │    name    │ type │   size   │   modified   │");
            println!("├───┼────────────┼──────┼──────────┼──────────────┤");
            println!("│ 0 │ Cargo.toml │ file │    456 B │ 2 hours ago  │");
            println!("│ 1 │ src        │ dir  │    4 KiB │ 1 hour ago   │");
            println!("│ 2 │ tests      │ dir  │    4 KiB │ 3 hours ago  │");
            println!("│ 3 │ README.md  │ file │  2.1 KiB │ 1 day ago    │");
            println!("╰───┴────────────┴──────┴──────────┴──────────────╯");
        } else if command.contains("sys") {
            println!("╭─────────┬──────────────────────────╮");
            println!("│ host    │ {{record 6 fields}}        │");
            println!("│ cpu     │ [table 8 rows]           │");
            println!("│ disks   │ [table 2 rows]           │");
            println!("│ mem     │ {{record 4 fields}}        │");
            println!("│ net     │ [table 3 rows]           │");
            println!("│ temp    │ [table 4 rows]           │");
            println!("╰─────────┴──────────────────────────╯");
        } else if command.contains("ps") {
            println!("╭────┬──────┬──────────────┬───────┬──────────╮");
            println!("│  # │  pid │     name     │  cpu  │  memory  │");
            println!("├────┼──────┼──────────────┼───────┼──────────┤");
            println!("│  0 │    1 │ init         │  0.00 │   12 MiB │");
            println!("│  1 │   42 │ service-mgr  │  0.10 │   24 MiB │");
            println!("│  2 │  180 │ browser      │  8.20 │  512 MiB │");
            println!("│  3 │  201 │ cargo        │ 28.50 │  256 MiB │");
            println!("╰────┴──────┴──────────────┴───────┴──────────╯");
        } else {
            println!("(nu: executed '{}' — structured output simulated)", command);
        }
        return 0;
    }

    // Check for a script file
    let script: Option<&str> = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str());

    if let Some(file) = script {
        println!("(nu: executing script '{}' — simulated)", file);
        return 0;
    }

    // Interactive mode
    println!("Welcome to Nushell 0.94.0 (OurOS)");
    println!("Type 'help' for help, 'help commands' for command list");
    println!();
    println!("〉");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nu(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_nu};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_nu(vec!["--help".to_string()]), 0);
        assert_eq!(run_nu(vec!["-h".to_string()]), 0);
        let _ = run_nu(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_nu(vec![]);
    }
}
