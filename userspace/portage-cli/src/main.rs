#![deny(clippy::all)]

//! portage-cli — OurOS Gentoo Portage package manager
//!
//! Multi-personality: `emerge`, `equery`, `eclean`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_emerge(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: emerge [OPTIONS] [ATOM ...]");
        println!("Portage 2.3.99 (OurOS)");
        println!();
        println!("Actions:");
        println!("  --install            Install packages (default)");
        println!("  --unmerge, -C        Remove packages");
        println!("  --update, -u         Update packages");
        println!("  --depclean           Remove unneeded packages");
        println!("  --sync               Sync the Portage tree");
        println!("  --search, -s TERM    Search packages");
        println!("  --info               Show system info");
        println!("  --pretend, -p        Show what would be done");
        println!("  --ask, -a            Ask before proceeding");
        println!("  --deep, -D           Consider deep dependencies");
        println!("  --newuse, -N         Rebuild for changed USE flags");
        println!("  --world              Update @world set");
        println!("  --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Portage 2.3.99 (python 3.12.2, OurOS, x86_64)");
        return 0;
    }
    if args.iter().any(|a| a == "--sync") {
        println!(">>> Syncing repository 'gentoo' ...");
        println!(">>> Starting rsync...");
        println!(">>> Sync completed.");
        println!("* IMPORTANT: news items need reading.");
        return 0;
    }
    if args.iter().any(|a| a == "-s" || a == "--search") {
        let term = args.windows(2)
            .find(|w| w[0] == "-s" || w[0] == "--search")
            .map(|w| w[1].as_str())
            .unwrap_or("vim");
        println!("*  app-editors/{}", term);
        println!("      Latest version available: 9.1.0");
        println!("      Latest version installed: [ Not Installed ]");
        println!("      Homepage: https://www.{}.org", term);
        println!("      Description: Vi IMproved");
        return 0;
    }
    if args.iter().any(|a| a == "--info") {
        println!("Portage 2.3.99 (python 3.12.2, OurOS x86_64)");
        println!("ACCEPT_KEYWORDS=\"amd64\"");
        println!("CFLAGS=\"-O2 -pipe -march=native\"");
        println!("USE=\"X alsa dbus\"");
        return 0;
    }
    let pretend = args.iter().any(|a| a == "-p" || a == "--pretend");
    let pkgs: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();
    if pretend {
        println!("These are the packages that would be merged:");
        for p in &pkgs {
            println!("[ebuild  N    ] {} 1.0.0  USE=\"-doc\"", p);
        }
    } else {
        for p in &pkgs {
            println!(">>> Emerging (1 of 1) {}-1.0.0", p);
            println!(">>> Compiling source in /var/tmp/portage/...");
            println!(">>> Installing {}-1.0.0", p);
            println!(">>> Completed installing {}-1.0.0", p);
        }
    }
    0
}

fn run_equery(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: equery COMMAND [OPTIONS]");
        println!("  belongs FILE    Find package owning a file");
        println!("  depends PKG     Show dependencies");
        println!("  files PKG       List files owned by package");
        println!("  list PATTERN    List installed packages");
        println!("  size PKG        Show package size");
        println!("  uses PKG        Show USE flags");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("list");
    match cmd {
        "list" => {
            println!("app-editors/vim-9.1.0");
            println!("dev-lang/python-3.12.2");
            println!("sys-apps/portage-2.3.99");
        }
        "size" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("vim");
            println!("{}: 42 files, 15.3 MiB", pkg);
        }
        "uses" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("vim");
            println!("USE flags for {}:", pkg);
            println!("  + X          : Enable X11 support");
            println!("  + python     : Python scripting");
            println!("  - lua        : Lua scripting");
        }
        _ => println!("equery: '{}' completed", cmd),
    }
    0
}

fn run_eclean(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: eclean [OPTIONS] ACTION");
        println!("  distfiles    Clean downloaded source files");
        println!("  packages     Clean built binary packages");
        return 0;
    }
    let action = args.first().map(|s| s.as_str()).unwrap_or("distfiles");
    println!("eclean {}: cleaning...", action);
    println!("  Deleted 15 files, freed 500 MiB");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "emerge".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "equery" => run_equery(&rest),
        "eclean" => run_eclean(&rest),
        _ => run_emerge(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_emerge};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/portage"), "portage");
        assert_eq!(basename(r"C:\bin\portage.exe"), "portage.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("portage.exe"), "portage");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_emerge(&["--help".to_string()]), 0);
        assert_eq!(run_emerge(&["-h".to_string()]), 0);
        assert_eq!(run_emerge(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_emerge(&[]), 0);
    }
}
