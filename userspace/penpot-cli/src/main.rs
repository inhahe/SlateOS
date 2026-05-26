#![deny(clippy::all)]
//! penpot-cli — personality CLI for Penpot, the open-source design + prototype
//! platform.
//!
//! Founded 2020 in Madrid as a flagship product of Kaleidos Open Source S.L.,
//! an open-source consultancy active since 2009. Penpot is positioned as the
//! open-source alternative to Figma — Mozilla Public Licence (MPL-2.0), with
//! SVG as the native file format instead of a proprietary binary. After the
//! 2022 Figma + Adobe acquisition announcement, Penpot benefitted from a huge
//! wave of designers + companies looking for self-hostable + non-proprietary
//! alternatives. Kaleidos shipped Penpot 1.0 in Feb 2022; Penpot raised a
//! \$8M Series A in late 2023 led by Decibel Partners + Athletica Ventures to
//! accelerate development. Picked up Mozilla, GitLab, Holaluz + several large
//! EU public-sector customers as design-system + open-source-mandate users.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Penpot open-source design + prototype personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Kaleidos 2020 Madrid; MPL-2.0; Series A 2023");
    println!("    canvas        Vector canvas + SVG-native + design tokens");
    println!("    selfhost      Docker / Helm / Kubernetes self-host first-class");
    println!("    libraries     Components + design systems + shared libraries");
    println!("    figma         Figma comparison + Figma file import");
    println!("    licence       MPL-2.0 + SVG native + transparent governance");
    println!("    customers     Open-source-mandate orgs + EU public sector + Mozilla");
    println!("    history       2020 founding -> 2022 1.0 -> 2023 Series A");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("penpot-cli 0.1.0 (open-source-design-tool personality build)"); }

fn run_about() {
    println!("Penpot (Kaleidos Open Source S.L.).");
    println!("  Founded:    2020 in Madrid as a Kaleidos flagship product.");
    println!("              Kaleidos itself has been an open-source consultancy since 2009.");
    println!("  Licence:    Mozilla Public Licence 2.0 (MPL-2.0).");
    println!("  Backers:    Decibel Partners (Series A lead), Athletica Ventures, K Fund,");
    println!("              prominent open-source ecosystem angels.");
    println!("  Funding:    ~\\$8M Series A late 2023; modest by design-tool category standards.");
    println!("  Position:   the open-source + self-hostable alternative to Figma + Sketch +");
    println!("              Adobe XD. SVG-native + design-tokens-first.");
    println!("  Major bump: huge user-growth wave following the Sept 2022 Figma + Adobe news.");
}

fn run_canvas() {
    println!("Vector canvas.");
    println!("  SVG-native: the saved file format is SVG + JSON metadata, not a proprietary");
    println!("  binary. Files can be opened + inspected + diffed with normal tools.");
    println!("  Bezier + path editing, boolean operations, masks, flexbox + grid layouts,");
    println!("  components + variants + nested instances, typography + colour styles.");
    println!("  Realtime multi-user editing + presence cursors + comment threads.");
    println!("  Design tokens (W3C draft spec): first-class in the data model, exportable");
    println!("  to JSON / CSS / Tailwind / iOS / Android consumable by build pipelines.");
    println!("  Prototype mode: clickable interactions + page-to-page transitions.");
}

fn run_selfhost() {
    println!("Self-hosting (first-class delivery option).");
    println!("  Docker Compose stack for solo + small-team self-hosts.");
    println!("  Helm chart for production Kubernetes installs.");
    println!("  Air-gapped install path supported for regulated industries + public-sector.");
    println!("  PostgreSQL + Redis backend; LDAP + OIDC + SAML SSO supported.");
    println!("  Penpot Cloud is fundamentally the same image as self-host + no feature gating");
    println!("  beyond authentication providers — full feature parity by design.");
    println!("  Common deployment: regulated EU SMBs + universities + government IT.");
}

fn run_libraries() {
    println!("Components + libraries.");
    println!("  Component primitives: variants, overrides, swap, nested components.");
    println!("  Shared libraries: publish a design system across files + teams + workspaces.");
    println!("  Design tokens flow through components into export pipelines automatically.");
    println!("  Auto-layout (flex + grid) for responsive component design.");
    println!("  Export specifications: per-asset multi-format (SVG, PNG, JPEG, PDF, WebP).");
    println!("  Code inspect tab generates CSS + tokens + measurements for developer handoff.");
}

fn run_figma() {
    println!("Comparison with Figma.");
    println!("  Penpot:   MPL-2.0 OSS,    SVG-native,         self-hostable, open governance.");
    println!("  Figma:    proprietary,    .fig binary format, SaaS-only,     Adobe (paused).");
    println!("  Penpot can import Figma files (.fig) and export back — migration is realistic.");
    println!("  Penpot loses to Figma on: plug-in ecosystem maturity, raw editor feature breadth,");
    println!("  AI / generative features, ProtoPie-level micro-interaction prototyping.");
    println!("  Penpot wins on: licence, self-host, design-tokens, file portability, governance,");
    println!("  cost at scale, vendor-lock-in resistance, transparency, EU public-sector fit.");
}

fn run_licence() {
    println!("Licence + governance.");
    println!("  Mozilla Public Licence 2.0: file-level copyleft, plays well with proprietary");
    println!("  customer assets while keeping Penpot itself permanently open.");
    println!("  Transparent roadmap on GitHub + community proposals.");
    println!("  No CLA — contributions stay under the MPL-2.0 directly.");
    println!("  SVG-native file format means files outlive the platform.");
    println!("  Hosted by Kaleidos (Madrid) but governed in the open with community input.");
    println!("  Design Tokens W3C draft contributor — pushes the standard from inside Penpot.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: open-source-mandate orgs, EU public sector, mission-driven NGOs,");
    println!("  large enterprises with strict data-sovereignty + IP-portability requirements.");
    println!("  Named customers: Mozilla, GitLab, Holaluz, multiple EU government agencies.");
    println!("  Industries: developer tooling, regulated finance, government, education,");
    println!("  research labs, foundations + non-profits, open-source projects themselves.");
    println!("  Geographic: very heavy EU presence (Madrid origin + GDPR + sovereignty fit),");
    println!("  growing LATAM + APAC + selective US.");
    println!("  Anti-segment: design-led startups in the Figma + Notion + Linear stack.");
}

fn run_history() {
    println!("History.");
    println!("  2009:  Kaleidos founded in Madrid as an open-source consultancy.");
    println!("  2020:  Penpot project starts as a Kaleidos flagship + alpha public release.");
    println!("  Feb 2022: Penpot 1.0 ships — first stable, production-ready release.");
    println!("  Sept 2022: Figma + Adobe deal announced; Penpot signups surge.");
    println!("  2023:  Penpot 2.0 with major performance + feature overhaul + design tokens.");
    println!("  Dec 2023: ~\\$8M Series A from Decibel + Athletica + K Fund.");
    println!("  Dec 2023: Figma + Adobe deal abandoned over EU/UK competition concerns.");
    println!("  2024+: continued growth in EU public sector + open-source-mandate orgs.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "penpot-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "canvas" => run_canvas(),
        "selfhost" => run_selfhost(),
        "libraries" => run_libraries(),
        "figma" => run_figma(),
        "licence" | "license" => run_licence(),
        "customers" => run_customers(),
        "history" => run_history(),
        "help" | "--help" | "-h" => print_help(&prog),
        "version" | "--version" | "-V" => print_version(),
        other => {
            println!("unknown command: {other}");
            print_help(&prog);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_handles_separators() {
        assert_eq!(basename("/a/b/c"), "c");
        assert_eq!(basename("a\\b\\c"), "c");
        assert_eq!(basename("only"), "only");
    }

    #[test]
    fn strip_ext_drops_exe() {
        assert_eq!(strip_ext("foo.exe"), "foo");
        assert_eq!(strip_ext("foo"), "foo");
    }

    #[test]
    fn smoke_runs() {
        run_about();
        run_canvas();
        run_selfhost();
        run_libraries();
        run_figma();
        run_licence();
        run_customers();
        run_history();
    }

    #[test]
    fn help_and_version() {
        print_help("penpot-cli");
        print_version();
    }
}
