#![deny(clippy::all)]

//! squarespace-cli — OurOS Squarespace (NYSE:SQSP, design-led website + commerce builder)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sqsp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: squarespace [OPTIONS]");
        println!("Squarespace (OurOS) — beautifully-designed website + commerce + scheduling + domains in one");
        println!();
        println!("Options:");
        println!("  --personal             Personal — $16/mo");
        println!("  --business             Business — $23/mo (adds commerce, custom CSS)");
        println!("  --commerce-basic       Commerce Basic — $28/mo");
        println!("  --commerce-advanced    Commerce Advanced — $52/mo (abandoned cart, subscriptions)");
        println!("  --scheduling           Acuity Scheduling (acquired 2019)");
        println!("  --domains              Squarespace Domains");
        println!("  --bio-sites            Bio Sites (link-in-bio)");
        println!("  --unfold               Unfold (story templates, acquired 2019)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Squarespace 2024 (OurOS)"); return 0; }
    println!("Squarespace 2024 (OurOS)");
    println!("  Vendor: Squarespace Inc. (New York City — was NYSE:SQSP, taken private 2024)");
    println!("  Founders: Anthony Casalena, 2003");
    println!("          Casalena built first version in his University of Maryland dorm");
    println!("          self-funded for ~7 years before any outside money");
    println!("          stayed CEO from age 19 through IPO and now ongoing private");
    println!("          one of the most consistent founder-CEOs in SaaS (20+ years running it)");
    println!("  Founded: 2003 in College Park MD, moved to NYC ~2010");
    println!("          self-funded until 2010 ($38M from Index/Accel/General Atlantic)");
    println!("          IPO May 2021 via direct listing NYSE:SQSP, opened ~$48 (~$7B valuation)");
    println!("          dropped to $20-30 range 2022-2024");
    println!("          Permira buyout announced May 2024 at $46.50/share (~$7.2B), closed Q4 2024");
    println!("          taken PRIVATE — Casalena rolled equity + remains CEO");
    println!("          FY2023 revenue ~$1B, ~4M paying subs");
    println!("  Strategic position: 'design-first website + commerce + appointments + bio for creators':");
    println!("                    primary competitor: Wix (slightly cheaper, more features, less polished)");
    println!("                    competitor in commerce: Shopify (more commerce-deep), Wix Stores");
    println!("                    competitor in scheduling: Calendly, SimplyBook.me");
    println!("                    competitor in bio-link: Linktree, Beacons");
    println!("                    pitch: 'beautiful brand site that also sells' for creatives + small biz");
    println!("                    famous for super-polished podcast ads (every podcast circa 2014-2023)");
    println!("  Pricing (transparent — by tier, annual prepay typical):");
    println!("    Personal — $16/mo (annual) — 1 contributor, basic website, no commerce");
    println!("    Business — $23/mo — unlimited contributors, custom CSS/JS, basic commerce");
    println!("    Commerce Basic — $28/mo — full commerce, gift cards, no transaction fees");
    println!("    Commerce Advanced — $52/mo — abandoned cart recovery, subscriptions, advanced shipping");
    println!("    Acuity Scheduling — $20-$61/mo separate (acquired 2019, integrated)");
    println!("    Squarespace Domains — separate, $20-$50/yr typical");
    println!("    transaction fees: 3% (Business), 0% (Commerce tiers) — plus card processor fees");
    println!("  Design system (the real product):");
    println!("    - Fluid Engine (drag-and-drop visual editor, replaced classic editor 2022)");
    println!("    - ~100 award-winning templates, all responsive + design-coherent");
    println!("    - Built-in galleries, parallax scrolling, video backgrounds, typography");
    println!("    - Each template includes professional fonts + image layouts");
    println!("    - 'designer-grade' vs Wix's 'kitchen-sink' aesthetic — Squarespace prized for visual restraint");
    println!("  Commerce features:");
    println!("    - Sell physical, digital, services, gift cards, subscriptions");
    println!("    - POS via Square integration (not Squarespace POS — different companies)");
    println!("    - Inventory + variants + product reviews + related products");
    println!("    - Abandoned cart email (Commerce Advanced only)");
    println!("    - Member areas (paid content gating)");
    println!("    - Donations + fundraising blocks");
    println!("    - Print-on-demand via Printful/Printify integrations");
    println!("    - Apple Pay + Stripe + PayPal + Afterpay/Klarna");
    println!("    - Multi-currency NOT natively supported (vs Shopify/Wix) — perennial customer complaint");
    println!("  Acuity Scheduling (acquired Apr 2019 ~$65M):");
    println!("    - Appointment booking + calendar + automated reminders");
    println!("    - Integrates into Squarespace sites as embedded scheduler");
    println!("    - Direct competitor to: Calendly + SimplyBook.me");
    println!("    - Massive in: coaches, therapists, salons, tutors, fitness studios");
    println!("  Unfold (acquired Sep 2019):");
    println!("    - Mobile app for Instagram Story templates");
    println!("    - 100M+ downloads, popular with creators");
    println!("    - Doesn't directly tie to website biz but locks in creator brand mindshare");
    println!("  Bio Sites (Squarespace's Linktree competitor):");
    println!("    - Free link-in-bio mini-pages");
    println!("    - Funnels users into upgrading to full Squarespace site");
    println!("  Squarespace AI (added 2023):");
    println!("    - AI site generator (Squarespace Blueprint AI)");
    println!("    - AI content generation for product descriptions, about pages");
    println!("    - Designed for the 'first 5 minutes' onboarding experience");
    println!("  Domains:");
    println!("    - Bought Google Domains business (Jun 2023) for ~$180M");
    println!("    - 10M+ domains under management overnight after acquisition");
    println!("    - Now a major domain registrar (top 5 globally)");
    println!("  Marketing add-ons:");
    println!("    - Email Campaigns (Mailchimp-lite, built-in)");
    println!("    - Member Areas + paid newsletters (Substack-lite)");
    println!("    - Forms + popups");
    println!("    - SEO basics built in (no plugins needed)");
    println!("  Customers: 4M+ paying subscribers globally");
    println!("            heavy in: photographers, restaurants, freelancers, B&Bs, wedding planners, podcasters");
    println!("            often the 'site' part of 'IG + Squarespace' creator stack");
    println!("            celebrity sites: Idris Elba, Jay-Z, Lana Del Rey, Reese Witherspoon (via Hello Sunshine)");
    println!("            sweet spot: $0-$1M/yr boutique businesses + creators wanting one polished brand");
    println!("  Critique: less customizable than Wix or WordPress — by design");
    println!("           apps marketplace is tiny vs Shopify (limits extensibility for commerce-heavy use)");
    println!("           multi-currency missing + international tax handling weak");
    println!("           commerce reporting basic vs Shopify Analytics");
    println!("           Fluid Engine editor transition (2022) caused complaints from long-time customers");
    println!("           Permira buyout: uncertainty about product investment under PE ownership");
    println!("           pricier than Wix for similar features");
    println!("  Differentiator: best-designed templates + design-coherent end-result + Acuity scheduling + creator-friendly + integrated domains — for people who care more about how their site looks than what it does");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "squarespace".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sqsp(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sqsp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/squarespace"), "squarespace");
        assert_eq!(basename(r"C:\bin\squarespace.exe"), "squarespace.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("squarespace.exe"), "squarespace");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_sqsp(&["--help".to_string()], "squarespace"), 0);
        assert_eq!(run_sqsp(&["-h".to_string()], "squarespace"), 0);
        assert_eq!(run_sqsp(&["--version".to_string()], "squarespace"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_sqsp(&[], "squarespace"), 0);
    }
}
