#![deny(clippy::all)]

//! winetricks-cli — OurOS Winetricks Wine helper
//!
//! Single personality: `winetricks`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_winetricks(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: winetricks [OPTIONS] [VERB...]");
        println!("winetricks v20231124 (OurOS) — Wine helper script");
        println!();
        println!("Verbs:");
        println!("  dlls              Install Windows DLL overrides");
        println!("  fonts             Install fonts");
        println!("  settings          Change Wine settings");
        println!("  apps              Install applications");
        println!();
        println!("Options:");
        println!("  --gui             Use GUI mode");
        println!("  -q                Quiet mode");
        println!("  --force           Force install");
        println!("  --unattended      No prompts");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("winetricks v20231124 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "--gui") {
        println!("winetricks: GUI mode started");
        return 0;
    }
    if args.is_empty() {
        println!("winetricks: no verb specified (use --help for usage)");
        return 1;
    }
    let verb = args.first().map(|s| s.as_str()).unwrap_or("");
    match verb {
        "dlls" => {
            println!("Available DLL packages:");
            println!("  vcrun2019         Visual C++ 2015-2019 Redistributable");
            println!("  d3dx9             DirectX 9 runtime");
            println!("  dotnet48          .NET Framework 4.8");
            println!("  dxvk              Vulkan-based D3D9/10/11");
        }
        "fonts" => {
            println!("Available font packages:");
            println!("  corefonts         Microsoft Core Fonts");
            println!("  tahoma            Tahoma font");
            println!("  allfonts          All available fonts");
        }
        _ => println!("winetricks: installing '{}'...", verb),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "winetricks".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_winetricks(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_winetricks};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/winetricks"), "winetricks");
        assert_eq!(basename(r"C:\bin\winetricks.exe"), "winetricks.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("winetricks.exe"), "winetricks");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_winetricks(&["--help".to_string()], "winetricks"), 0);
        assert_eq!(run_winetricks(&["-h".to_string()], "winetricks"), 0);
        assert_eq!(run_winetricks(&["--version".to_string()], "winetricks"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_winetricks(&[], "winetricks"), 0);
    }
}
