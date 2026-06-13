#![deny(clippy::all)]

//! convertkit-cli — SlateOS ConvertKit / Kit (creator-focused email marketing)
//!
//! Single personality: `convertkit` (also responds to `kit`)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ck(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: convertkit [OPTIONS]");
        println!("ConvertKit / Kit (Slate OS) — email marketing for creators (newsletters, courses, podcasts)");
        println!();
        println!("Options:");
        println!("  --newsletter           Newsletter Free tier (up to 10K subscribers)");
        println!("  --creator              Creator plan (automations + integrations)");
        println!("  --creator-pro          Creator Pro (advanced reporting + newsletter referral)");
        println!("  --commerce             Commerce — sell digital products directly");
        println!("  --sponsor-network      Sponsor Network (newsletter sponsorship marketplace)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ConvertKit / Kit 2024 (Slate OS)"); return 0; }
    println!("ConvertKit / Kit 2024 (Slate OS)");
    println!("  Vendor: ConvertKit, Inc. (Boise, ID — private, rebranded to 'Kit' in 2024)");
    println!("  Founders: Nathan Barry (CEO), 2013");
    println!("          Barry was a designer + indie author who needed email for his book launches");
    println!("          built ConvertKit himself initially, scaled it as a one-person shop until ~2014");
    println!("          legendary 'public failure' story: nearly shut down 2014, pivoted to creators, became hit");
    println!("          Barry blogs openly about ConvertKit's monthly revenue + lessons");
    println!("  Founded: 2013 in Boise, Idaho — fully remote since founding");
    println!("          bootstrapped to ~$30M+ ARR with no outside funding");
    println!("          considered the bootstrapped success story of SaaS email");
    println!("          rebranded to 'Kit' Aug 2024 (Barry: 'we're not just convert + kit anymore')");
    println!("  Strategic position: 'email + audience for creators, not e-commerce or marketers':");
    println!("                    laser-focused on bloggers, YouTubers, podcasters, course creators, authors, newsletter writers");
    println!("                    NOT for e-commerce (would use Klaviyo)");
    println!("                    NOT for B2B (would use HubSpot or ActiveCampaign)");
    println!("                    competes mostly with Substack (newsletter platform), Beehiiv (next-gen creator), MailerLite");
    println!("                    legacy competitors: Mailchimp, AWeber, ConstantContact");
    println!("  Pricing (subscriber-count based, very transparent):");
    println!("    Newsletter (Free) — up to 10K subscribers, unlimited broadcasts");
    println!("       no automations, no integrations, includes 'Powered by Kit' branding");
    println!("    Creator — starts $29/mo (1K subs), $79/mo (10K), $379/mo (50K), $679/mo (100K)");
    println!("       automations + integrations + visual editor + sequences");
    println!("    Creator Pro — starts $59/mo (1K subs), $149/mo (10K), $679/mo (50K)");
    println!("       Newsletter Referral System + advanced reporting + Facebook custom audiences + priority support");
    println!("    scales smoothly to 500K+ subscribers");
    println!("  Creator-native features:");
    println!("    - Subscriber-centric (not list-centric): one subscriber, many tags");
    println!("    - Tag-based segmentation (no need to dupe subscribers across lists)");
    println!("    - Landing pages + forms (looks like a creator's website, not a marketer's funnel)");
    println!("    - Embedded forms, modal popups, inline forms, slide-ins, sticky bars");
    println!("    - Sign-up form trigger: tag, sequence, broadcast send, custom field, automation");
    println!("    - Visual automation builder ('Visual Automations' — added 2018)");
    println!("    - Sequences (linear drip series — the original ConvertKit feature)");
    println!("    - Broadcasts (one-off newsletter sends)");
    println!("  Kit Commerce (digital product sales):");
    println!("    - Sell digital products: ebooks, music, presets, templates, courses");
    println!("    - Stripe + PayPal integration");
    println!("    - Optional tipping ('pay what you want')");
    println!("    - Built-in checkout — no Gumroad/Stripe Checkout needed");
    println!("    - Auto-deliver via email + download link");
    println!("    - Bundles + coupons + tax handling");
    println!("    - 3.5% transaction fee on Free, 0% on Creator+");
    println!("  Sponsor Network (Apr 2023 launch):");
    println!("    - Marketplace connecting newsletter creators with brand sponsors");
    println!("    - Brands like Notion, Webflow, Beehiiv, Convertkit-listed-creators");
    println!("    - Creators self-pitch newsletter ad slots — Kit handles invoicing");
    println!("    - 10% take rate");
    println!("    - Directly competes with Beehiiv's 'Beehiiv Boosts' newsletter sponsorship");
    println!("  Creator Network + Recommendations:");
    println!("    - Creators recommend other creators' newsletters during sign-up confirmation");
    println!("    - Both parties grow audience via swap referrals");
    println!("    - Newsletter Referral System (Creator Pro): readers refer friends to unlock rewards (like Morning Brew)");
    println!("  Integrations: 130+");
    println!("              WordPress, Squarespace, Webflow, Shopify");
    println!("              YouTube, Patreon, Teachable, Thinkific, Podia");
    println!("              Zapier + Make + Pabbly");
    println!("              Stripe (Commerce + tip), PayPal");
    println!("              REST API + JS embed SDK");
    println!("  Customers: 600,000+ creators using Kit (free + paid)");
    println!("            Tim Ferriss, James Clear, Pat Flynn, Casey Neistat, Brené Brown, Seth Godin");
    println!("            Heath Brothers, Maria Popova (Marginalian), and tens of thousands of newsletter writers");
    println!("  Critique: weaker for e-commerce (no real cart/product feed/abandoned cart vs Klaviyo)");
    println!("           reporting is basic vs HubSpot/Klaviyo/Braze");
    println!("           no SMS channel (creators don't need it as much)");
    println!("           landing page templates are limited vs Squarespace/Carrd");
    println!("           Substack threat: free + built-in audience + paid subscriptions out of the box");
    println!("           Beehiiv threat: better creator UX + Beehiiv Boosts grew faster");
    println!("           rebrand to 'Kit' caused confusion + SEO loss");
    println!("  Differentiator: best email tool for non-techy creators + first-class commerce + sponsor network — 'newsletter business in a box'");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "convertkit".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ck(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ck};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/convertkit"), "convertkit");
        assert_eq!(basename(r"C:\bin\convertkit.exe"), "convertkit.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("convertkit.exe"), "convertkit");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ck(&["--help".to_string()], "convertkit"), 0);
        assert_eq!(run_ck(&["-h".to_string()], "convertkit"), 0);
        let _ = run_ck(&["--version".to_string()], "convertkit");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ck(&[], "convertkit");
    }
}
