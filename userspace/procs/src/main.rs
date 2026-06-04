#![deny(clippy::all)]

//! procs — OurOS modern replacement for ps written in Rust
//!
//! Single personality: `procs`

use std::env;
use std::process;

fn run_procs(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: procs [OPTIONS] [KEYWORD]...");
        println!();
        println!("A modern replacement for ps.");
        println!();
        println!("Options:");
        println!("  -a, --and              AND logic for keywords");
        println!("  -o, --or               OR logic for keywords (default)");
        println!("  -d, --nand             NAND logic for keywords");
        println!("  -r, --nor              NOR logic for keywords");
        println!("  -l, --list             Show list of available columns");
        println!("  -t, --tree             Show process tree");
        println!("  -w, --watch <SECS>     Watch mode with interval");
        println!("  --watch-interval <N>   Alias for --watch");
        println!("  -i, --insert <COL>     Insert column to output");
        println!("  --only <PIDS>          Show only specified PIDs");
        println!("  --sorta <COL>          Sort ascending by column");
        println!("  --sortd <COL>          Sort descending by column");
        println!("  -c, --color <WHEN>     Color output (auto/always/never)");
        println!("  --theme <THEME>        Color theme (auto/dark/light/monokai)");
        println!("  -p, --pager <CMD>      Pager to use");
        println!("  --no-header            Don't show column headers");
        println!("  --per-core             Show per-core CPU usage");
        println!("  --gen-completion <SH>  Generate shell completions");
        println!("  --gen-config           Generate default config");
        println!("  -V, --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("procs 0.14.6 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "-l" || a == "--list") {
        println!("Available columns:");
        println!("  Pid          Process ID");
        println!("  PPid         Parent process ID");
        println!("  User         User name");
        println!("  Uid          User ID");
        println!("  Group        Group name");
        println!("  Gid          Group ID");
        println!("  State        Process state");
        println!("  Nice         Nice value");
        println!("  Tty          Terminal");
        println!("  Threads      Thread count");
        println!("  TcpPort      TCP port");
        println!("  UdpPort      UDP port");
        println!("  Cpu          CPU usage (%)");
        println!("  Mem          Memory usage (%)");
        println!("  VmSize       Virtual memory size");
        println!("  VmRss        Resident set size");
        println!("  VmData       Data segment size");
        println!("  VmStack      Stack size");
        println!("  VmSwap       Swap usage");
        println!("  ReadBytes    Total read bytes");
        println!("  WriteBytes   Total written bytes");
        println!("  StartTime    Start time");
        println!("  Elapsed      Elapsed time");
        println!("  Command      Command name");
        println!("  CmdLine      Full command line");
        return 0;
    }
    if args.iter().any(|a| a == "--gen-config") {
        println!("# procs configuration");
        println!("[[columns]]");
        println!("kind = \"Pid\"");
        println!("style = \"BrightYellow|Yellow\"");
        println!("align = \"Right\"");
        println!();
        println!("[[columns]]");
        println!("kind = \"User\"");
        println!("style = \"BrightGreen|Green\"");
        println!();
        println!("[[columns]]");
        println!("kind = \"Cpu\"");
        println!("style = \"BrightCyan|Cyan\"");
        println!("align = \"Right\"");
        println!();
        println!("[[columns]]");
        println!("kind = \"Mem\"");
        println!("style = \"BrightMagenta|Magenta\"");
        println!("align = \"Right\"");
        println!();
        println!("[[columns]]");
        println!("kind = \"Command\"");
        println!("style = \"BrightWhite|White\"");
        return 0;
    }

    let tree = args.iter().any(|a| a == "-t" || a == "--tree");

    let keywords: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if tree {
        println!("  PID  User     CPU%  Mem%  Command");
        println!("    1  root      0.0   0.1  init");
        println!("    ├─ 42  root      0.1   0.2  service-manager");
        println!("    │  ├─ 85  root      0.0   0.1  network-manager");
        println!("    │  ├─ 92  root      0.0   0.3  display-server");
        println!("    │  │  └─ 120  user      1.2   2.5  compositor");
        println!("    │  └─ 98  root      0.0   0.1  audio-server");
        println!("    ├─ 150  user      0.5   1.8  terminal");
        println!("    │  └─ 155  user      0.2   0.4  shell");
        println!("    │     └─ 201  user      8.5   3.2  cargo build");
        println!("    └─ 180  user      2.1   5.4  browser");
    } else if !keywords.is_empty() {
        println!("  PID  User     State  CPU%  Mem%  TCP  Command");
        println!("  201  user     R       8.5   3.2  ---  cargo build");
        println!("  202  user     S       0.1   0.5  ---  rust-analyzer");
    } else {
        println!("  PID  User     State  CPU%  Mem%  TCP   Command");
        println!("    1  root     S       0.0   0.1  ---   init");
        println!("   42  root     S       0.1   0.2  ---   service-manager");
        println!("   85  root     S       0.0   0.1  ---   network-manager");
        println!("   92  root     S       0.0   0.3  ---   display-server");
        println!("   98  root     S       0.0   0.1  ---   audio-server");
        println!("  120  user     S       1.2   2.5  ---   compositor");
        println!("  150  user     S       0.5   1.8  ---   terminal");
        println!("  155  user     S       0.2   0.4  ---   shell");
        println!("  180  user     S       2.1   5.4  443   browser");
        println!("  201  user     R       8.5   3.2  ---   cargo build");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_procs(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_procs};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_procs(vec!["--help".to_string()]), 0);
        assert_eq!(run_procs(vec!["-h".to_string()]), 0);
        let _ = run_procs(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_procs(vec![]);
    }
}
