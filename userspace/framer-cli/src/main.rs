#![deny(clippy::all)]
//! framer-cli — personality CLI for Framer, the design-to-published-website
//! platform.
//!
//! Founded 2014 in Amsterdam by Koen Bok (CEO, ex-Sofa / ex-Facebook) and
//! Jorn van Dijk (ex-Sofa / ex-Facebook). Sofa was acquired by Facebook
//! in 2011; both founders then left to start Framer. The original product
//! was Framer Classic — a code-based interaction-prototyping tool used by
//! senior product designers at Apple, Google, Airbnb to build advanced
//! interaction prototypes via CoffeeScript / TypeScript. Around 2020-2022
//! Framer pivoted to a Webflow-style visual + responsive site builder
//! that publishes real production websites. Picked up Series C in 2023
//! led by Atomico + AlleyCorp at a multi-hundred-million valuation. Today
//! Framer competes with Webflow + Squarespace + Wix on the no-code-website
//! axis, with a designer-first reputation as the differentiator.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Framer design + publish site builder personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Bok + van Dijk 2014 Amsterdam ex-Sofa + ex-Facebook");
    println!("    canvas        Designer-first canvas + breakpoints + responsive layout");
    println!("    publish       Publish-to-real-website + custom domain + edge hosting");
    println!("    cms           Built-in CMS + collections + dynamic pages");
    println!("    code          Framer code components + React + Motion library");
    println!("    history       2014 Framer Classic -> 2022 visual builder pivot");
    println!("    pricing       Free + Mini + Basic + Pro + Startup + Enterprise tiers");
    println!("    customers     Designer-led indie + agency + small-team marketing sites");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("framer-cli 0.1.0 (designer-first-site-builder personality build)"); }

fn run_about() {
    println!("Framer (Motif Tech B.V.).");
    println!("  Founded:    2014, Amsterdam, Netherlands.");
    println!("  Founders:   Koen Bok (CEO; ex-Sofa, ex-Facebook product) +");
    println!("              Jorn van Dijk (ex-Sofa, ex-Facebook product).");
    println!("              Sofa was acquired by Facebook in 2011 — both left to start Framer.");
    println!("  Backers:    Atomico, AlleyCorp, GV (Google Ventures), Accel, Atomico.");
    println!("  Funding:    \\$27M Series C 2023; multi-hundred-million valuation.");
    println!("  Position:   designer-first website builder with publish-to-real-web output.");
    println!("  Heritage:   Framer Classic (2014) was the go-to interaction-prototype tool");
    println!("              for Apple + Google + Airbnb senior designers.");
}

fn run_canvas() {
    println!("Designer-first canvas.");
    println!("  Vector + image + text on an infinite canvas like Figma, but the canvas is");
    println!("  the page layout — what you draw is what publishes.");
    println!("  Breakpoint system: desktop / tablet / phone breakpoints with per-breakpoint");
    println!("  layout overrides. Modern flexbox + grid + stack layout primitives.");
    println!("  Smart components: variants + props + states like Figma components, but live.");
    println!("  Animation + scroll-linked effects + on-hover micro-interactions inline.");
    println!("  Designer ergonomics that Webflow's classic UI was always knocked for.");
}

fn run_publish() {
    println!("Publish + hosting.");
    println!("  One-click publish: Framer hosts the rendered site on its global edge network.");
    println!("  Custom domain: bring your own domain, automatic SSL, custom DNS records.");
    println!("  SSR + static rendering: SEO-friendly pre-rendered HTML output.");
    println!("  Image optimisation + responsive image serving baked in.");
    println!("  Lighthouse / Core-Web-Vitals scoring competitive with hand-built React sites.");
    println!("  No need to deploy elsewhere — Framer is both the editor + the host.");
}

fn run_cms() {
    println!("Built-in CMS.");
    println!("  Collections: structured content types with fields like text, image, date,");
    println!("  number, link, reference, multi-reference, rich text, file.");
    println!("  Dynamic pages: bind one page template to a collection, get N pages out.");
    println!("  Markdown + MDX import for blog migrations.");
    println!("  Localisation: multi-locale CMS entries for translated sites.");
    println!("  External CMS sync via Framer API for Sanity / Contentful / Notion sources.");
    println!("  Sufficient for blogs, docs, marketing-site portfolios, small e-commerce.");
}

fn run_code() {
    println!("Code components (the Framer Classic heritage).");
    println!("  Framer code components: hand-written React + TypeScript components that");
    println!("  drop onto the canvas alongside visual elements.");
    println!("  Property controls: declare component props that show up in the inspector.");
    println!("  Framer Motion: animation library spun out of Framer, now its own ecosystem,");
    println!("  the de facto React animation library outside Framer itself.");
    println!("  This bridges no-code customers who hit a wall + developer-led customers");
    println!("  who want a designer-friendly canvas with React escape-hatches.");
}

fn run_history() {
    println!("History (compressed).");
    println!("  2011:  Sofa acquired by Facebook; Bok + van Dijk join + later leave.");
    println!("  2014:  Framer Classic launches — CoffeeScript-based prototype tool for senior");
    println!("         product designers at Apple + Google + Airbnb.");
    println!("  2018:  TypeScript + React components added; closer to a real design tool.");
    println!("  2020:  pivot toward visual layout + responsive design begins.");
    println!("  2022:  Framer Sites launches — publish-to-real-web becomes the primary product.");
    println!("  2023:  Series C from Atomico + AlleyCorp + Accel; team expands.");
    println!("  2024+: AI features for site + copy + content generation alongside the canvas.");
}

fn run_pricing() {
    println!("Pricing model.");
    println!("  Free:       Framer-branded subdomain, basic site, watermark.");
    println!("  Mini:       ~\\$5/month for personal landing pages.");
    println!("  Basic:      ~\\$15/month for small business sites + custom domain.");
    println!("  Pro:        ~\\$30/month for marketing teams + advanced CMS + integrations.");
    println!("  Startup:    ~\\$75/month for high-traffic startup sites + analytics + AB tests.");
    println!("  Enterprise: custom — SSO, audit, dedicated support, SLAs.");
    println!("  Per-site pricing instead of per-builder-seat — same shape as Webflow.");
}

fn run_customers() {
    println!("Customer profile:");
    println!("  Sweet spot: design-led startups + agencies + indie product makers.");
    println!("  Industries: SaaS marketing sites, design + creative agencies, portfolio sites");
    println!("  for designers + photographers + studios, indie product landing pages,");
    println!("  developer-tool startup marketing sites where the marketing site has to");
    println!("  *look* indistinguishable from a hand-built Next.js site.");
    println!("  Geographic: heavy EU + US + LATAM + APAC creative communities.");
    println!("  Common journey: 'we want a Webflow site but our designer hates the Webflow UI'.");
    println!("  Anti-segment: large enterprise CMS (those go to Webflow Enterprise or AEM).");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "framer-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "canvas" => run_canvas(),
        "publish" => run_publish(),
        "cms" => run_cms(),
        "code" => run_code(),
        "history" => run_history(),
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
        run_publish();
        run_cms();
        run_code();
        run_history();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("framer-cli");
        print_version();
    }
}
