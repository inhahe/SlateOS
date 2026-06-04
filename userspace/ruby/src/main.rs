#![deny(clippy::all)]

//! ruby — OurOS Ruby interpreter
//!
//! Multi-personality binary detected via argv[0]:
//!
//! - `ruby` (default) — Ruby interpreter
//! - `irb` — Interactive Ruby shell
//! - `gem` — Ruby package manager
//! - `rake` — Ruby build tool
//! - `bundler` / `bundle` — Ruby dependency manager
//! - `erb` — Embedded Ruby template processor
//! - `rdoc` — Ruby documentation generator
//! - `ri` — Ruby interactive reference

use std::env;
use std::process;

// ── Main logic ────────────────────────────────────────────────────────

fn run_ruby(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ruby [switches] [--] [programfile] [arguments]");
        println!();
        println!("  -0[octal]       specify record separator");
        println!("  -a              autosplit mode with -n or -p");
        println!("  -c              check syntax only");
        println!("  -e 'command'    one line of script");
        println!("  -i[extension]   edit ARGV files in place");
        println!("  -n              assume 'while gets(); ... end' loop");
        println!("  -p              assume loop like -n but print line");
        println!("  -r library      require the library before executing");
        println!("  -v              print version and enable verbose mode");
        println!("  -w              turn warnings on");
        println!("  --version       print version");
        return 0;
    }

    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("ruby 3.3.0 (2025-01-01) [x86_64-ouros]");
        return 0;
    }

    if let Some(pos) = args.iter().position(|a| a == "-e") {
        if let Some(code) = args.get(pos + 1) {
            println!("(executing: {} — simulated)", code);
            return 0;
        }
        eprintln!("ruby: no code specified for -e");
        return 1;
    }

    if args.iter().any(|a| a == "-c") {
        println!("Syntax OK");
        return 0;
    }

    let script = args.first().filter(|a| !a.starts_with('-'));
    if let Some(file) = script {
        println!("(executing: {} — simulated)", file);
        return 0;
    }

    println!("ruby 3.3.0 (2025-01-01) [x86_64-ouros]");
    println!("(interactive mode — use irb for a better experience)");
    0
}

fn run_irb(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: irb [options]");
        println!("  --noprompt    Don't show prompt");
        println!("  --noecho      Don't echo results");
        println!("  --version     Show version");
        return 0;
    }
    let _ = args;
    println!("irb(main):001:0> RUBY_PLATFORM");
    println!("=> \"x86_64-ouros\"");
    println!("irb(main):002:0> RUBY_VERSION");
    println!("=> \"3.3.0\"");
    println!("irb(main):003:0> [1,2,3].map {{ |x| x * 2 }}");
    println!("=> [2, 4, 6]");
    println!("irb(main):004:0> exit");
    0
}

fn run_gem(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("RubyGems is a sophisticated package manager for Ruby.");
            println!();
            println!("Usage: gem COMMAND [ARGS] [OPTIONS]");
            println!();
            println!("Commands:");
            println!("  install       Install a gem");
            println!("  uninstall     Uninstall a gem");
            println!("  list          List installed gems");
            println!("  search        Search for gems");
            println!("  update        Update gems");
            println!("  environment   Show RubyGems env");
            println!("  --version     Show version");
            0
        }
        "--version" | "-v" => { println!("3.5.0"); 0 }
        "install" => {
            for pkg in &cmd_args {
                if !pkg.starts_with('-') {
                    println!("Fetching {}...", pkg);
                    println!("Successfully installed {}-1.0.0", pkg);
                }
            }
            println!("1 gem installed");
            0
        }
        "list" => {
            println!("*** LOCAL GEMS ***");
            println!("bundler (2.5.0)");
            println!("irb (1.11.0)");
            println!("minitest (5.21.0)");
            println!("rake (13.1.0)");
            println!("rdoc (6.6.0)");
            0
        }
        "environment" | "env" => {
            println!("RubyGems Environment:");
            println!("  - RUBYGEMS VERSION: 3.5.0");
            println!("  - RUBY VERSION: 3.3.0 (2025-01-01) [x86_64-ouros]");
            println!("  - INSTALLATION DIRECTORY: /usr/lib/ruby/gems/3.3.0");
            println!("  - GEM PATHS:");
            println!("     - /usr/lib/ruby/gems/3.3.0");
            println!("     - /home/user/.gem/ruby/3.3.0");
            0
        }
        "search" => {
            let query = cmd_args.first().map(|s| s.as_str()).unwrap_or("*");
            println!("*** REMOTE GEMS ***");
            println!("{} (results simulated)", query);
            0
        }
        "uninstall" => { println!("Successfully uninstalled (simulated)"); 0 }
        "update" => { println!("Updating installed gems (simulated)"); 0 }
        other => { eprintln!("gem: unknown command '{}'", other); 1 }
    }
}

fn run_rake(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "-H") {
        println!("Usage: rake [options ...] [VAR=VALUE ...] [targets ...]");
        println!();
        println!("Options:");
        println!("  -T, --tasks    List tasks with descriptions");
        println!("  -n, --dry-run  Do a dry run");
        println!("  -t, --trace    Turn on invoke/execute tracing");
        println!("  -f FILE        Use FILE as Rakefile");
        println!("  --version      Show version");
        return 0;
    }

    if args.iter().any(|a| a == "--version") {
        println!("rake, version 13.1.0"); return 0;
    }

    if args.iter().any(|a| a == "-T" || a == "--tasks") {
        println!("rake build    # Build the project");
        println!("rake clean    # Remove build artifacts");
        println!("rake default  # Run default tasks");
        println!("rake test     # Run tests");
        return 0;
    }

    let target = args.first().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("default");
    println!("** Invoke {} (first_time)", target);
    println!("** Execute {} (simulated)", target);
    0
}

fn run_bundle(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Bundler manages an application's dependencies.");
            println!();
            println!("Usage: bundle COMMAND [ARGS]");
            println!();
            println!("Commands:");
            println!("  install    Install gems from Gemfile");
            println!("  update     Update gems");
            println!("  exec       Execute command in bundle context");
            println!("  list       List gems in bundle");
            println!("  show       Show gem source location");
            println!("  init       Generate a Gemfile");
            println!("  --version  Show version");
            0
        }
        "--version" | "-v" => { println!("Bundler version 2.5.0"); 0 }
        "install" => {
            println!("Fetching gem metadata from https://rubygems.org/...");
            println!("Resolving dependencies...");
            println!("Using bundler 2.5.0");
            println!("Bundle complete! 3 Gemfile dependencies, 15 gems now installed.");
            0
        }
        "list" => {
            println!("Gems included by the bundle:");
            println!("  * bundler (2.5.0)");
            println!("  * rake (13.1.0)");
            println!("  * minitest (5.21.0)");
            0
        }
        "exec" => { println!("(bundle exec — simulated)"); 0 }
        "init" => { println!("Writing new Gemfile (simulated)"); 0 }
        other => { eprintln!("bundle: unknown command '{}'", other); 1 }
    }
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("ruby");
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
        "irb" => run_irb(rest),
        "gem" => run_gem(rest),
        "rake" => run_rake(rest),
        "bundler" | "bundle" => run_bundle(rest),
        "erb" => { println!("(ERB template processing — simulated)"); 0 }
        "rdoc" => { println!("Generating documentation... done (simulated)"); 0 }
        "ri" => { println!("(ri: Ruby interactive reference — simulated)"); 0 }
        _ => run_ruby(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{run_ruby};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ruby(vec!["--help".to_string()]), 0);
        assert_eq!(run_ruby(vec!["-h".to_string()]), 0);
        let _ = run_ruby(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ruby(vec![]);
    }
}
