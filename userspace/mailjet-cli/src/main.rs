#![deny(clippy::all)]
//! mailjet-cli — Slate OS Mailjet (Sinch Email) personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}
fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — Mailjet (Sinch Email) transactional + marketing.");
    println!();
    println!("USAGE:  {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about           French origin, Pathwire, Sinch acquisition");
    println!("    products        Transactional, Marketing, MJML, Inbox preview");
    println!("    mjml            The MJML responsive-email framework");
    println!("    pricing         Volume tiers");
    println!("    customers       European enterprises and SMBs");
    println!("    differentiator  EU-headquartered, MJML, dual transactional+marketing");
    println!("    critique        Honest critique");
    println!("    help / version");
}

fn print_about() {
    println!("Mailjet — French email vendor, now part of Sinch.");
    println!();
    println!("Founded 2010 in Paris by Wilfried Durand and Julien Tartarin.");
    println!("Both founders came from MIT and engineering backgrounds. They");
    println!("started Mailjet at a moment when Europe was underserved by the");
    println!("US-dominated email-as-a-service market (SendGrid, Mailgun were");
    println!("US-based with US data residency). Mailjet positioned as EU-");
    println!("headquartered with French data centers and a GDPR-by-design");
    println!("posture from day one.");
    println!();
    println!("Funding: ~€11M Series A 2014 led by Iris Capital, ~€12M growth");
    println!("round 2015. Bootstrapped revenue growth thereafter.");
    println!();
    println!("Acquired by Mailgun's parent Pathwire (after Pathwire spun");
    println!("Mailgun out of Rackspace in 2020) in July 2019 for an undisclosed");
    println!("but reportedly ~€100M sum. Pathwire then sold the entire portfolio");
    println!("to Sinch (Swedish customer-comms conglomerate) in October 2021");
    println!("for ~$1.9B. Mailjet now sits inside Sinch alongside Mailgun,");
    println!("Inbox Tools (formerly 250ok), and Sinch's voice/SMS APIs.");
    println!();
    println!("Mailjet kept its Paris HQ and French engineering presence. The");
    println!("'Sinch Email' rebranding is partial — both Mailgun and Mailjet");
    println!("brands continue to operate, with different positioning (Mailjet");
    println!("more marketing-friendly, Mailgun more developer-tier).");
}

fn print_products() {
    println!("Mailjet product line:");
    println!();
    println!("• Transactional Email API");
    println!("    REST + SMTP for programmatic sending. Receipts, password");
    println!("    resets, magic links. Send API supports batch sends, templates,");
    println!("    variable substitution, attachments, headers.");
    println!();
    println!("• Marketing Campaigns");
    println!("    Visual email builder, segmentation, A/B testing, scheduling.");
    println!("    Templates marketplace + custom branded templates.");
    println!();
    println!("• Automation / Workflows");
    println!("    Trigger-based email sequences: welcome flows, drip campaigns,");
    println!("    abandoned cart, re-engagement.");
    println!();
    println!("• Contact Management");
    println!("    Lists, segments, custom fields, subscription preferences,");
    println!("    double opt-in, automatic unsubscribe handling.");
    println!();
    println!("• Inbox Preview (Litmus-like)");
    println!("    Render your email in 50+ inbox clients before sending —");
    println!("    Outlook, Gmail, Apple Mail, mobile, dark mode.");
    println!();
    println!("• Email Editor with MJML");
    println!("    Drag-and-drop builder + MJML-based templates. MJML compiles");
    println!("    to email-client-safe HTML automatically.");
    println!();
    println!("• Email Verification (with Inbox Tools)");
    println!("    Bulk list validation, single-address verification, list");
    println!("    hygiene at send time.");
    println!();
    println!("• APIs and SDKs");
    println!("    REST + SMTP. Official SDKs for Node, Python, Ruby, PHP, Go,");
    println!("    Java, .NET. SMTP relay for legacy applications.");
}

fn print_mjml() {
    println!("MJML — the Mailjet Markup Language.");
    println!();
    println!("MJML is Mailjet's open-source email markup framework, released");
    println!("2016 under MIT. The core problem MJML solves: HTML email is");
    println!("table-soup-based, with notoriously inconsistent rendering across");
    println!("clients. Building responsive, accessible HTML email by hand is");
    println!("a specialized art. MJML lets you write semantic-ish markup that");
    println!("compiles down to email-client-safe HTML.");
    println!();
    println!("Example MJML:");
    println!();
    println!("  <mjml>");
    println!("    <mj-body>");
    println!("      <mj-section>");
    println!("        <mj-column>");
    println!("          <mj-text>Hello world!</mj-text>");
    println!("          <mj-button href=\"https://example.com\">");
    println!("            Click me");
    println!("          </mj-button>");
    println!("        </mj-column>");
    println!("      </mj-section>");
    println!("    </mj-body>");
    println!("  </mjml>");
    println!();
    println!("Compiles to ~50 lines of nested tables, VML for Outlook, inline");
    println!("CSS, mobile-responsive media queries. The output works in Gmail,");
    println!("Outlook (including the dreaded Outlook 2007-2019 Word renderer),");
    println!("Apple Mail, Yahoo, AOL, etc.");
    println!();
    println!("MJML is one of the most starred open-source email frameworks");
    println!("(~16K GitHub stars). Used independently of Mailjet — many");
    println!("Postmark, SendGrid, SES, and Resend customers use MJML for");
    println!("template authoring. Direct competitor to React Email in 2024+,");
    println!("though MJML is markup-based and React Email is component-based.");
    println!();
    println!("Tooling: VS Code extension, mjml-cli, online playground, ~30");
    println!("pre-built templates in the MJML gallery.");
}

fn print_pricing() {
    println!("Mailjet pricing (USD/EUR, 2025):");
    println!();
    println!("• Free");
    println!("    6,000 emails/month (200/day cap), all features, Mailjet");
    println!("    branding in footer, unlimited contacts, 1 user.");
    println!();
    println!("• Essential (transactional-focused):");
    println!("    $15/mo — 15K emails/mo, no daily cap, removes branding,");
    println!("    online + email support, basic statistics.");
    println!();
    println!("• Premium:");
    println!("    $25/mo — 15K emails + segmentation, automation, A/B testing,");
    println!("    multi-user accounts (3 users), priority support.");
    println!();
    println!("• Custom (enterprise):");
    println!("    Volume pricing for 100K+ emails/month, dedicated IPs, SLA,");
    println!("    SSO, audit logs, dedicated support manager.");
    println!();
    println!("• Pay-as-you-go credits also available for sporadic senders.");
    println!();
    println!("Pricing is competitive with SendGrid for marketing tiers and");
    println!("with Mailgun for transactional tiers. EU regional sending");
    println!("(dedicated EU infrastructure) available on higher plans for");
    println!("GDPR-strict customers.");
}

fn print_customers() {
    println!("Mailjet customer references:");
    println!();
    println!("  • Microsoft — uses Mailjet for some EU transactional flows");
    println!("  • LeBonCoin (French Craigslist) — high-volume transactional");
    println!("  • Décathlon — retail email");
    println!("  • Société Générale — banking notifications");
    println!("  • Carrefour — retail loyalty");
    println!("  • Cdiscount — French e-commerce");
    println!("  • Renault, Volvo — automotive customer comms");
    println!("  • TF1 (French TV) — broadcast notifications");
    println!("  • Many European SMBs and mid-market");
    println!("  • EU government agencies");
    println!();
    println!("Pattern: heavy European enterprise + SMB presence, retail and");
    println!("e-commerce, French + EU-wide brands that prefer EU-headquartered");
    println!("vendors for GDPR comfort. Some US adoption but North American");
    println!("market share trails SendGrid + Mailgun (Sinch sister product).");
}

fn print_differentiator() {
    println!("Why teams pick Mailjet:");
    println!();
    println!("• EU-headquartered and GDPR-native. Data centers in France (and");
    println!("  Frankfurt). DPA terms friendly to EU enterprises. Important");
    println!("  for procurement at French/German/Italian companies.");
    println!();
    println!("• Both transactional AND marketing in one platform. Many");
    println!("  vendors specialize in one — Mailjet does both with a unified");
    println!("  contact model, deliverability infrastructure, and analytics.");
    println!();
    println!("• MJML built in. The leading open-source email markup framework");
    println!("  is integrated into Mailjet's template editor.");
    println!();
    println!("• Inbox Preview built in (formerly separate Litmus subscription).");
    println!();
    println!("• Multilingual support and EU regional infrastructure. Strong");
    println!("  fit for European multilingual brands.");
    println!();
    println!("• Sinch parent gives access to omnichannel (SMS, voice, push,");
    println!("  RCS) under common contracts for enterprises that need");
    println!("  cross-channel customer-comms.");
    println!();
    println!("vs. Mailgun: same parent (Sinch). Mailgun is more developer-");
    println!("  tier transactional; Mailjet has stronger marketing UI and EU");
    println!("  positioning. Some overlap intentionally — different segments.");
    println!();
    println!("vs. SendGrid: Mailjet has cleaner UX and EU positioning;");
    println!("  SendGrid has more North American brand and higher-volume");
    println!("  enterprise feature set.");
    println!();
    println!("vs. Mailchimp: Mailjet is more developer-friendly and");
    println!("  transactional-capable; Mailchimp is more marketing-funnel-");
    println!("  focused with a deeper UI footprint.");
}

fn print_critique() {
    println!("Honest critique of Mailjet:");
    println!();
    println!("• Brand confusion post-acquisition. Mailjet / Mailgun / Sinch");
    println!("  Email — multiple sibling products under one parent has");
    println!("  led to overlapping positioning and customer uncertainty");
    println!("  about long-term roadmap consolidation.");
    println!();
    println!("• Roadmap velocity post-Sinch has been slower than indie days.");
    println!("  Big-company consolidation effects. Customers report that");
    println!("  feature requests move slowly through the larger Sinch");
    println!("  product portfolio.");
    println!();
    println!("• Marketing-tier UX is functional but less polished than");
    println!("  Mailchimp or Klaviyo. Coming from those products you'll");
    println!("  notice rough edges in Mailjet's marketing automation builder.");
    println!();
    println!("• North American adoption is lower than EU. Documentation,");
    println!("  customer success, and timezone-aligned support are better");
    println!("  for EU customers.");
    println!();
    println!("• Deliverability is reasonable but not industry-leading. Some");
    println!("  shared-IP pools have had reputation issues during certain");
    println!("  periods. Dedicated IPs available on higher tiers but at cost.");
    println!();
    println!("• MJML is open-source and not Mailjet-exclusive — competitive");
    println!("  vendors can integrate MJML support too, slightly diluting");
    println!("  the Mailjet differentiator over time.");
    println!();
    println!("• Pricing tier between Free and Essential ($0 to $15) has a");
    println!("  gap where light senders find no good fit.");
}

fn run_mailjet(args: &[String], prog: &str) -> i32 {
    if args.is_empty() { print_help(prog); return 0; }
    match args[0].as_str() {
        "help" | "--help" | "-h" => { print_help(prog); 0 }
        "version" | "--version" | "-V" => {
            println!("{prog} 0.1.0 (Slate OS personality CLI)"); 0
        }
        "about" => { print_about(); 0 }
        "products" => { print_products(); 0 }
        "mjml" => { print_mjml(); 0 }
        "pricing" => { print_pricing(); 0 }
        "customers" => { print_customers(); 0 }
        "differentiator" | "diff" => { print_differentiator(); 0 }
        "critique" => { print_critique(); 0 }
        other => {
            eprintln!("{prog}: unknown subcommand '{other}'");
            eprintln!("Try '{prog} help' for usage.");
            2
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "mailjet".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_mailjet(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn t_basename() { assert_eq!(basename("/usr/bin/mailjet"), "mailjet"); }
    #[test] fn t_strip() { assert_eq!(strip_ext("mailjet.exe"), "mailjet"); }
    #[test] fn t_help() { assert_eq!(run_mailjet(&[], "mailjet"), 0); }
    #[test] fn t_unknown() { assert_eq!(run_mailjet(&["xx".to_string()], "mailjet"), 2); }
}
