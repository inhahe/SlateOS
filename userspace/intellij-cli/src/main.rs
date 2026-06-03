#![deny(clippy::all)]

//! intellij-cli — OurOS IntelliJ IDEA (JetBrains flagship IDE)
//!
//! Single personality: `intellij`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ij(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: intellij [OPTIONS]");
        println!("IntelliJ IDEA 2024.3 (OurOS) — JetBrains flagship Java/Kotlin IDE");
        println!();
        println!("Options:");
        println!("  --community            IntelliJ IDEA Community Edition (free, OSS)");
        println!("  --ultimate             IntelliJ IDEA Ultimate Edition (paid)");
        println!("  --new                  New project");
        println!("  --ai-assistant         JetBrains AI Assistant ($10/mo)");
        println!("  --toolbox              JetBrains Toolbox App (manages installs)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("IntelliJ IDEA 2024.3 (build 243.21565.193) (OurOS)"); return 0; }
    println!("IntelliJ IDEA 2024.3 (build 243.21565.193) (OurOS)");
    println!("  Vendor: JetBrains s.r.o. (HQ Prague, CZ — founded 2000 in St. Petersburg, RU)");
    println!("  Founders: Sergey Dmitriev, Valentin Kipiatkov, Eugene Belyaev");
    println!("  HQ relocation: 2022 moved EU + sold Russia operations after Ukraine invasion");
    println!("  Pricing:");
    println!("    Community Edition: FREE (Apache 2.0 license, OSS)");
    println!("    Ultimate Edition: $599/yr (individual), $169/yr after 3 years (loyalty discount)");
    println!("    All Products Pack: $289/yr — every JetBrains IDE + plugins");
    println!("  Built on: IntelliJ Platform (open-source) — also powers PyCharm, WebStorm, etc.");
    println!("  Language: written in Java + Kotlin (JetBrains' own language — also they invented Kotlin!)");
    println!("  JetBrains IDE family (all built on IntelliJ Platform):");
    println!("    - PyCharm (Python), WebStorm (JS/TS), PhpStorm (PHP), RubyMine (Ruby)");
    println!("    - CLion (C++), Rider (.NET / Unity / Unreal), GoLand (Go), DataGrip (SQL)");
    println!("    - RustRover (Rust — released 2024), Android Studio (Google fork)");
    println!("    - Aqua (test automation), Fleet (their newer multi-lang lightweight editor)");
    println!("  Killer features:");
    println!("    - World-class refactoring (rename, extract, inline — across the whole project)");
    println!("    - Deep code analysis: warnings, intentions, quick-fixes, dataflow analysis");
    println!("    - First-class debugger with smart step-into, eval expression, hot-swap");
    println!("    - Built-in tools: Git/HG/SVN, DB, HTTP client, terminal, profiler, decompiler");
    println!("    - 'Bookmarks' + 'Recent Files' + 'Search Everywhere' (double Shift) navigation");
    println!("  AI Assistant ($10/mo extra): code completion, refactoring suggestions, commit messages");
    println!("  Spinoff: Kotlin (now official Android language), Hectic, Compose Multiplatform");
    println!("  Differentiator: best refactoring + Java/Kotlin / multi-language depth in industry");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "intellij".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ij(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ij};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/intellij"), "intellij");
        assert_eq!(basename(r"C:\bin\intellij.exe"), "intellij.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("intellij.exe"), "intellij");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_ij(&["--help".to_string()], "intellij"), 0);
        assert_eq!(run_ij(&["-h".to_string()], "intellij"), 0);
        assert_eq!(run_ij(&["--version".to_string()], "intellij"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_ij(&[], "intellij"), 0);
    }
}
