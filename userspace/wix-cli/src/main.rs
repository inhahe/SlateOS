#![deny(clippy::all)]

//! wix-cli — OurOS Wix (NASDAQ:WIX, drag-and-drop website + commerce)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wix(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wix [OPTIONS]");
        println!("Wix (OurOS) — drag-and-drop website builder + commerce + business apps (NASDAQ:WIX)");
        println!();
        println!("Options:");
        println!("  --light                Light — $17/mo (basic site, custom domain)");
        println!("  --core                 Core — $29/mo (commerce, $50K/yr sales)");
        println!("  --business             Business — $36/mo");
        println!("  --business-elite       Business Elite — $159/mo (no limits)");
        println!("  --studio               Wix Studio (designer/agency tier)");
        println!("  --velo                 Velo (JS dev platform on Wix backend)");
        println!("  --bookings             Wix Bookings (appointments)");
        println!("  --restaurants          Wix Restaurants (menu + online order)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Wix 2024 (OurOS)"); return 0; }
    println!("Wix 2024 (OurOS)");
    println!("  Vendor: Wix.com Ltd. (Tel Aviv, Israel — NASDAQ:WIX)");
    println!("  Founders: Avishai Abrahami (CEO), Nadav Abrahami (his brother), Giora Kaplan, 2006");
    println!("          founded after a personal frustration trying to build a website for one of their startups");
    println!("          'why do you need to know HTML to build a basic website?' → Wix");
    println!("          all three founders still at the company in 2024");
    println!("  Founded: 2006 in Tel Aviv");
    println!("          IPO Nov 2013 NASDAQ:WIX at $16.50 (~$750M valuation)");
    println!("          peaked ~$350 in early 2021 (now ~$120-150)");
    println!("          FY2024 revenue ~$1.7B (+13% YoY), profitable as of 2023");
    println!("          ~250M registered users globally");
    println!("          ~5,000 employees in Israel + worldwide");
    println!("  Strategic position: 'website + business apps for any small business, anywhere':");
    println!("                    primary competitor: Squarespace (smaller, design-led), GoDaddy Websites + Marketing, Webflow (designer-friendlier)");
    println!("                    commerce competitor: Shopify (more commerce-deep), Squarespace");
    println!("                    booking competitor: Calendly, Acuity, Booksy");
    println!("                    pitch: '1 platform for any kind of small biz' — restaurants + retail + services + creators");
    println!("                    massively heavy advertising — Wix Super Bowl ads, YouTuber sponsorships, TikTok");
    println!("                    aggressively international: ~50% of revenue outside the Americas");
    println!("  Pricing (transparent — by tier + commerce limits):");
    println!("    Light — $17/mo (1 contributor, custom domain, basic site)");
    println!("    Core — $29/mo (commerce up to $50K/yr, basic marketing)");
    println!("    Business — $36/mo ($100K/yr, advanced commerce features)");
    println!("    Business Elite — $159/mo (unlimited, advanced reports, dev mode)");
    println!("    Wix Studio — $30+/mo (designer/agency client management)");
    println!("    transaction fees: 0% on all Wix plans (uses your payment processor)");
    println!("    + app marketplace charges + Wix Bookings + Wix Restaurants subscriptions");
    println!("  Editor (the famous drag-and-drop):");
    println!("    - Wix Editor (legacy classic drag-and-drop)");
    println!("    - Wix Studio (new, responsive-aware editor, launched 2023)");
    println!("    - Wix ADI (Artificial Design Intelligence — auto-generates a site from a few questions)");
    println!("    - 900+ templates across all industries");
    println!("    - critique: easy to make BAD sites (everything is movable, layouts can break)");
    println!("    - Wix Studio addresses this with CSS Grid + responsive constraints");
    println!("  Wix Studio (designer/agency offering, big 2023+ push):");
    println!("    - For freelance designers and agencies building client sites");
    println!("    - Reusable design systems + master pages + workspace + client billing");
    println!("    - CSS Grid + flex layout (vs Editor's absolute positioning)");
    println!("    - Direct competitor to: Webflow (designer-loved), Framer");
    println!("    - Wix's bet to capture the designer/agency market it historically lost to Webflow");
    println!("  Velo by Wix (the developer platform):");
    println!("    - JavaScript/TypeScript backend + serverless on Wix infrastructure");
    println!("    - Full-stack web dev with Wix as host + DB + CMS");
    println!("    - Custom database collections + APIs + scheduled jobs + external API calls");
    println!("    - Built on Node.js — devs can build complex apps without leaving Wix");
    println!("    - This is Wix's most-underrated technical asset");
    println!("  Business apps (Wix's depth):");
    println!("    - Wix Stores (e-commerce — multiple shipping zones, dropshipping, POS, multichannel)");
    println!("    - Wix Bookings (appointments — competes with Acuity + Calendly)");
    println!("    - Wix Restaurants (menus + online orders + tables — vs Toast lite)");
    println!("    - Wix Events + Tickets");
    println!("    - Wix Hotels (basic property mgmt + booking)");
    println!("    - Wix Music (sell tracks/albums)");
    println!("    - Wix Video (subscriptions for content creators)");
    println!("    - Wix Forms + Email Marketing + Member Areas");
    println!("    - Wix POS (Wix Retail with hardware + tap-to-pay)");
    println!("  Wix AI (heavy 2023+ push):");
    println!("    - AI site generator: describe your business → Wix generates site");
    println!("    - AI text/image/video generators for content");
    println!("    - AI Inbox (chat customer support automation)");
    println!("  Domains + email:");
    println!("    - Free domain on annual plans");
    println!("    - Wix Email (Google Workspace integration + Wix Mail)");
    println!("    - Wix Phone numbers (twilio-powered)");
    println!("  Customers: 250M+ registered, ~7M paid subs");
    println!("            heavy in: small biz worldwide, freelancers, restaurants, fitness studios");
    println!("            very international: huge in LATAM, Eastern Europe, Israel obviously, MENA, SE Asia");
    println!("            celebrity/brand: Karlie Kloss site, NICE-Network for Israel arts, many NGOs");
    println!("  Critique: SEO historically weaker than WordPress (improved significantly 2022+)");
    println!("           site speed varies — Editor sites can be heavy");
    println!("           switching off Wix is painful (proprietary platform, no easy export)");
    println!("           commerce features deep but lag Shopify on complex catalog + multi-store");
    println!("           reputation: 'cheap-looking' sites by amateurs — Wix Studio targets this perception");
    println!("           Studio adoption among Webflow loyalists is slow");
    println!("           heavy ad spend dampens GAAP profitability vs revenue growth");
    println!("  Differentiator: most all-in-one platform (site + commerce + bookings + restaurants + POS + dev platform) at SMB pricing, with Velo for developers — for any small biz wanting one vendor for everything");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wix".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wix(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
