#![deny(clippy::all)]

//! gog-cli — OurOS GOG Galaxy 2.0 (DRM-free game store by CD Projekt)
//!
//! Single personality: `gog`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gog(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gog [OPTIONS]");
        println!("GOG Galaxy 2.0.78 (OurOS) — DRM-FREE game store + universal launcher");
        println!();
        println!("Options:");
        println!("  --library              Unified library (GOG + Steam + Epic + Xbox + PSN + Origin etc.)");
        println!("  --store                GOG Store (DRM-free)");
        println!("  --integrations         Manage external store integrations");
        println!("  --offline-installer    Download offline installer (DRM-free .exe + .bin)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("GOG Galaxy 2.0.78.10 (OurOS)"); return 0; }
    println!("GOG Galaxy 2.0.78.10 (OurOS)");
    println!("  Vendor: GOG Sp. z o.o. — subsidiary of CD Projekt SA (Warsaw, Poland)");
    println!("  CD Projekt: founded 1994 — also made The Witcher 1/2/3 + Cyberpunk 2077");
    println!("  Launched: GOG.com 2008 (originally 'Good Old Games', dropped name for broader scope)");
    println!("  Philosophy: DRM-FREE. Every game runs offline forever, no online activation");
    println!("             Offline installers downloadable: keep .exe/.bin files, install anywhere");
    println!("  Pricing: regional, often discounted for low-income regions (PPP pricing)");
    println!("  Catalog: classics (Heroes of Might & Magic, Fallout 1/2, Baldur's Gate I/II, etc.)");
    println!("          + indies + AAA where studios agree to DRM-free (CDPR's own games, etc.)");
    println!("  Famous absent: most major AAA publishers refuse DRM-free → narrower catalog vs Steam");
    println!("  GOG Galaxy 2.0: launched 2019 — universal launcher that AGGREGATES all your stores");
    println!("                  into one library (Steam, Epic, Xbox, PSN, Switch, Battle.net, Origin)");
    println!("  Money-back: 30-day refund, no questions asked (more lenient than Steam)");
    println!("  Cloud saves: yes, friends/chat/achievements (added later)");
    println!("  Mascot: Big Boy / dual-headed eagle silhouette");
    println!("  Differentiator: DRM-free preservation + universal launcher for all your other stores");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gog".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gog(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gog};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gog"), "gog");
        assert_eq!(basename(r"C:\bin\gog.exe"), "gog.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gog.exe"), "gog");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_gog(&["--help".to_string()], "gog"), 0);
        assert_eq!(run_gog(&["-h".to_string()], "gog"), 0);
        assert_eq!(run_gog(&["--version".to_string()], "gog"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_gog(&[], "gog"), 0);
    }
}
