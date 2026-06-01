#![deny(clippy::all)]
//! miro-cli — personality CLI for Miro, the online collaborative whiteboard
//! platform.
//!
//! Founded 2011 in Perm, Russia by Andrey Khusid (CEO) and Oleg Shardin
//! as RealtimeBoard inside Khusid's studio RAAD Studio; rebranded to Miro
//! in 2019 ahead of its hypergrowth period. Headquartered in Amsterdam +
//! San Francisco; left Russia entirely after the 2022 invasion of Ukraine
//! with employees relocated to the EU. Picked up a 2022 Series C at a
//! $17.5B valuation led by ICONIQ Growth — one of the headline collab-tool
//! valuations of that cycle. Defining product: infinite-canvas whiteboard
//! plus a deep library of templates for product, design, agile, strategy,
//! and workshop facilitation. Microsoft Whiteboard + Mural + FigJam are
//! the main rivals; Miro's enterprise penetration is the differentiator.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Miro online collaborative whiteboard personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Khusid + Shardin 2011 Perm; \\$17.5B ICONIQ 2022");
    println!("    canvas        Infinite whiteboard canvas + shapes + connectors + sticky");
    println!("    templates     Workshop + agile + strategy + design template library");
    println!("    integrations  Atlassian + Microsoft + Google + Slack + Notion + Figma");
    println!("    enterprise    Enterprise tier + SSO + governance + Miro Mind Map");
    println!("    rivals        FigJam vs Mural vs Lucidspark vs MS Whiteboard");
    println!("    pricing       Free + Starter + Business + Enterprise tiers");
    println!("    customers     Product + design + agile + workshop facilitator profile");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("miro-cli 0.1.0 (collaborative-whiteboard personality build)"); }

fn run_about() {
    println!("Miro (RealtimeBoard, Inc.).");
    println!("  Founded:    2011 in Perm, Russia as RealtimeBoard inside RAAD Studio.");
    println!("  Rebranded:  2019 to Miro ahead of hypergrowth + global expansion.");
    println!("  Founders:   Andrey Khusid (CEO) + Oleg Shardin (co-founder).");
    println!("  HQ:         Amsterdam + San Francisco; left Russia entirely post-2022.");
    println!("  Backers:    ICONIQ Growth, Accel, AltaIR Capital, Atlassian, Salesforce");
    println!("              Ventures, Dragoneer, GIC.");
    println!("  Funding:    \\$400M Series C 2022 at \\$17.5B valuation; ~\\$476M total raised.");
    println!("  Headcount:  ~1,800 across EU + US + APAC post-relocations.");
    println!("  Position:   the default infinite-canvas whiteboard for enterprise teams.");
}

fn run_canvas() {
    println!("Infinite canvas.");
    println!("  Smooth pan + zoom across a logically-infinite board, multi-user cursors,");
    println!("  realtime presence indicators, follow-the-presenter mode for facilitation.");
    println!("  Primitives: sticky notes, shapes, connectors, text, images, embeds, frames,");
    println!("  tables, mindmaps, cards, kanban, voting widgets, timer + estimation widgets.");
    println!("  Frames + grouped widgets for structured layouts on top of the free canvas.");
    println!("  Voice + video chat overlay for in-board synchronous sessions.");
    println!("  AI assist: cluster + summarise sticky notes, generate diagrams from prompt.");
}

fn run_templates() {
    println!("Template library.");
    println!("  Hundreds of templates covering: product discovery, customer-journey mapping,");
    println!("  retrospectives, planning poker, OKR planning, SWOT, lean canvas, kanban,");
    println!("  service blueprinting, wireframing, mind maps, brainstorming + ideation,");
    println!("  workshop facilitation, design sprints, RACI matrices, org charts.");
    println!("  Community templates: third-party-authored, browseable in the Miroverse hub.");
    println!("  The template marketplace is Miro's content-led growth funnel + lock-in moat.");
}

fn run_integrations() {
    println!("Integration ecosystem.");
    println!("  Atlassian: deep two-way Jira + Confluence + Trello — issue cards as widgets.");
    println!("  Microsoft Teams + Office 365: embed boards in Teams meetings, SSO.");
    println!("  Google Workspace: Drive picker, Calendar, Meet, SSO.");
    println!("  Slack: share + preview boards, notifications, slash commands.");
    println!("  Notion: embed pages + databases as cards on the board.");
    println!("  Figma + FigJam: embed designs live; widely used despite Figma being a rival.");
    println!("  Public REST + Web SDK: customer + partner-built apps via Miro Apps Marketplace.");
}

fn run_enterprise() {
    println!("Enterprise tier.");
    println!("  SAML SSO + SCIM provisioning + domain control + audit logs.");
    println!("  Data classification + DLP integrations + content + access controls.");
    println!("  Customer-managed encryption keys for regulated industries.");
    println!("  Data residency: EU + US options for compliance.");
    println!("  Miro for Government tier with FedRAMP scoping (in progress).");
    println!("  Miro AI: enterprise-controlled tenancy + private model usage.");
    println!("  Miro Mind Map + diagramming flows pitched as Lucidchart replacement at scale.");
}

fn run_rivals() {
    println!("Competitive positioning.");
    println!("  FigJam (Figma):    designer-first whiteboard, bundled with Figma orgs.");
    println!("  Mural:             enterprise-first whiteboard, design-thinking heritage.");
    println!("  Lucidspark:        Lucidchart's whiteboard sibling, diagram-leaning.");
    println!("  MS Whiteboard:     bundled-with-Teams whiteboard, lightweight functionality.");
    println!("  Apple Freeform:    consumer / small-team whiteboard, Apple ecosystem only.");
    println!("  Miro's edge: largest template library + deepest enterprise penetration +");
    println!("  most integration surface area. Loses to FigJam inside design-led orgs.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  Free:         3 editable boards, unlimited team members, core widgets.");
    println!("  Starter:      ~\\$8 / user / month, unlimited boards, basic integrations.");
    println!("  Business:     ~\\$16 / user / month, advanced AI, private boards, SSO option.");
    println!("  Enterprise:   custom — SAML SSO, SCIM, audit logs, data residency, support.");
    println!("  Standard collab-tool seat-pricing shape; deep discounts at enterprise volume.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: product + design + engineering + consulting teams in companies");
    println!("  with 100-100,000+ employees. Knowledge work where async + sync visual");
    println!("  collaboration matters more than document-style writing.");
    println!("  Industries: tech, financial services, consulting, education, government,");
    println!("  healthcare, manufacturing engineering. Strong in workshop + design-thinking");
    println!("  cultures. Most Fortune 500s carry Miro as a standard tool.");
    println!("  Geographic: EU + North America strongest; growing APAC + LATAM.");
    println!("  Anti-segment: Figma-native design orgs (default to FigJam for the bundle).");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "miro-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "canvas" => run_canvas(),
        "templates" => run_templates(),
        "integrations" => run_integrations(),
        "enterprise" => run_enterprise(),
        "rivals" => run_rivals(),
        "pricing" => run_pricing(),
        "customers" => run_customers(),
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
        run_templates();
        run_integrations();
        run_enterprise();
        run_rivals();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("miro-cli");
        print_version();
    }
}
