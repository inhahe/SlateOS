#![deny(clippy::all)]
//! kayako-cli — personality CLI for Kayako, the long-running PHP-era help
//! desk that survived two decades.
//!
//! Founded 2001 in Jalandhar, India by Varun Shoor at age 17, on PHP +
//! MySQL — one of the first commercially-distributed help-desk products
//! and the standard self-hosted choice in the early 2000s alongside
//! osTicket. Moved HQ to London. Pivoted from self-hosted licensing to a
//! Kayako-cloud SaaS model in the 2010s rebuild ("The new Kayako"),
//! reorganising the product around a unified customer-journey timeline.
//! Acquired by ESW Capital / Crossover in late 2017, after which it
//! continued operating but with reduced public footprint compared to
//! the Zendesk era — typical ESW playbook of running a mature product
//! profitably without aggressive growth marketing.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Kayako veteran help-desk personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Varun Shoor 2001 Jalandhar India; PHP-era pioneer");
    println!("    classic       Kayako Classic self-hosted PHP product (legacy)");
    println!("    cloud         Kayako Cloud SaaS rebuild + unified-journey UX");
    println!("    timeline      SingleView customer-journey timeline differentiator");
    println!("    selfhost      OnSite self-hosted continued option");
    println!("    eswcapital    Crossover / ESW Capital acquisition (2017+)");
    println!("    pricing       Per-agent-per-month tiered pricing");
    println!("    customers     Long-tenure SMB + mid-market customer profile");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("kayako-cli 0.1.0 (veteran-help-desk personality build)"); }

fn run_about() {
    println!("Kayako (Kayako, Inc., now ESW Capital portfolio).");
    println!("  Founded:    2001 — Varun Shoor, age 17, Jalandhar, India.");
    println!("  Languages:  PHP + MySQL originally — among the first commercially-");
    println!("              successful help desks distributed as on-premise software.");
    println!("  HQ:         London (mid-2000s relocation), with India engineering.");
    println!("  Backers:    Bootstrapped originally; took growth investment ~2014-2015");
    println!("              from Mayfield Fund + Index Ventures pre-pivot.");
    println!("  Acquired:   ESW Capital / Crossover acquired Kayako in late 2017.");
    println!("  Position:   profitable, low-marketing-spend ESW portfolio company,");
    println!("              long-tenured SMB + mid-market user base.");
}

fn run_classic() {
    println!("Kayako Classic (legacy self-hosted PHP product).");
    println!("  The pre-2016 product line that the company was originally built around.");
    println!("  Sold as a one-time licence + support contract — install on your own LAMP");
    println!("  stack, ticketing + live chat + knowledge base.");
    println!("  Modular: SupportSuite, ResolutionSuite, FusionSuite (different bundles).");
    println!("  Still maintained as a 'Classic' product line for customers who didn't");
    println!("  migrate to the new cloud — long EOL tail, common ESW pattern.");
}

fn run_cloud() {
    println!("Kayako Cloud (SaaS rebuild).");
    println!("  Launched 2016 as 'The new Kayako' — full rewrite from the PHP monolith,");
    println!("  with a modern UI + SaaS deployment model.");
    println!("  Email-to-ticket, live chat widget, Facebook + Twitter capture,");
    println!("  knowledge base, automation rules, SLA management.");
    println!("  Multi-brand support: one tenant can host multiple branded help portals.");
    println!("  REST API + webhooks for integration with CRM / billing systems.");
}

fn run_timeline() {
    println!("SingleView customer-journey timeline.");
    println!("  Kayako Cloud's defining UX differentiator at relaunch:");
    println!("  every customer interaction across channels rendered as a single");
    println!("  vertical timeline — emails, chats, social mentions, knowledge-base");
    println!("  searches, page views.");
    println!("  Agent answering a new ticket sees the full prior journey at a glance.");
    println!("  Was years ahead of the 'unified customer view' trend that Intercom +");
    println!("  Front + Kustomer later monetised — Kayako shipped it first but didn't");
    println!("  capture the upside.");
}

fn run_selfhost() {
    println!("OnSite (self-hosted) continued option.");
    println!("  Even after the SaaS pivot, Kayako preserved an OnSite option:");
    println!("  install Kayako Cloud's codebase on customer hardware behind their firewall.");
    println!("  Use cases: regulated industries (healthcare, government), customers in");
    println!("  jurisdictions with data-residency rules, customers with existing on-prem");
    println!("  Kayako Classic investments that prefer to stay self-hosted.");
    println!("  Reduces churn from regulated buyers that SaaS-only competitors lose.");
}

fn run_eswcapital() {
    println!("Crossover / ESW Capital era (2017+).");
    println!("  ESW Capital (Austin, Joe Liemandt) acquires mature B2B software companies");
    println!("  and runs them on a high-cash-yield, low-marketing-spend playbook.");
    println!("  Kayako fits the pattern: established customer base, technical leadership");
    println!("  no longer needed at full strength, opportunity to extract long-tail recurring");
    println!("  revenue without aggressive growth investments.");
    println!("  Engineering reorganised onto Crossover's remote-contractor model.");
    println!("  Reduced product velocity post-acquisition; stable feature set with");
    println!("  maintenance + selective improvements.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  Inbox:    ~$15 per agent per month, basic shared inbox + ticketing.");
    println!("  Growth:   ~$30 per agent per month, adds automation + chat + analytics.");
    println!("  Scale:    ~$60 per agent per month, adds multi-brand + advanced reporting.");
    println!("  Enterprise: custom pricing with SLA + dedicated support.");
    println!("  OnSite: separate licence pricing for self-hosted deployments.");
    println!("  Annual contracts standard; volume discounts for larger agent counts.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Long-tenure SMB + mid-market customers, often on Kayako 5-15+ years.");
    println!("  Geographic spread: heavy UK + India + commonwealth-country presence");
    println!("  reflecting the founder's roots; large US + EU long tail.");
    println!("  Industries: software vendors, hosting / managed-service providers,");
    println!("  universities + education tech, professional services, MSPs.");
    println!("  Named historical customers: NASA (selective), Toshiba, Peugeot,");
    println!("  Texas A&M University, MTV (selective), various large hosting + ISP firms.");
    println!("  Modern position: respected veteran option, more 'still using it' than");
    println!("  'evaluating it' for new buyers — typical of a mature ESW portfolio brand.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "kayako-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "classic" => run_classic(),
        "cloud" => run_cloud(),
        "timeline" => run_timeline(),
        "selfhost" => run_selfhost(),
        "eswcapital" => run_eswcapital(),
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
        run_classic();
        run_cloud();
        run_timeline();
        run_selfhost();
        run_eswcapital();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("kayako-cli");
        print_version();
    }
}
