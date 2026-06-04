#![deny(clippy::all)]

//! woocommerce-cli — OurOS WooCommerce (the WordPress e-commerce plugin)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_woo(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: woocommerce [OPTIONS]");
        println!("WooCommerce (OurOS) — open-source e-commerce plugin for WordPress (Automattic-owned)");
        println!();
        println!("Options:");
        println!("  --core                 WooCommerce core plugin (free, GPL-licensed)");
        println!("  --extensions           Premium extensions (Subscriptions, Memberships, Bookings)");
        println!("  --payments             WooPayments (Stripe-powered, integrated processor)");
        println!("  --woocom               WooCommerce.com hosting (Automattic-managed)");
        println!("  --wp-engine            WP Engine Atlas (popular headless host)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("WooCommerce 9.x (OurOS)"); return 0; }
    println!("WooCommerce 9.x (OurOS)");
    println!("  Vendor: Automattic Inc. (San Francisco — private)");
    println!("  Original: WooThemes (Mark Forrester, Magnus Jepson, Adii Pienaar — South Africa, 2008)");
    println!("  Founded: 2011 (WooCommerce plugin), built atop Jigoshop fork");
    println!("          WooThemes acquired by Automattic May 2015 (~$30M) to integrate with WordPress.com");
    println!("          ~28% of online stores worldwide run WooCommerce (W3Techs surveys)");
    println!("          5M+ active installs of free plugin, ~3.5M live stores");
    println!("          largest e-commerce platform by store count (more than Shopify)");
    println!("  Strategic position: 'commerce for the 40% of the web running WordPress':");
    println!("                    primary competitor: Shopify (more polished, paid, hosted)");
    println!("                    self-hosted competitor: Magento (more complex, enterprise)");
    println!("                    plugin competitor (within WordPress): Easy Digital Downloads, BigCommerce-for-WordPress, Shopify-WordPress");
    println!("                    bottom-line pitch: 'free + own your store + WordPress flexibility'");
    println!("                    Automattic strategy: bundle with WordPress.com hosting + Jetpack + WooPayments");
    println!("  Pricing (the core is free — money is in extensions + hosting + payments):");
    println!("    Plugin — FREE, open-source (GPLv3)");
    println!("    WordPress hosting — varies, $5-$500+/mo (Bluehost, WP Engine, Pressable, Pantheon, Kinsta)");
    println!("    Extensions — most $79-$299/yr per extension (subscriptions, memberships, bookings, dynamic pricing)");
    println!("    WooPayments — 2.9% + 30¢ US card rates, integrated into admin");
    println!("    Themes — free + paid (StoreFront free, Astra, Flatsome, Avada popular)");
    println!("    typical 'real' WooCommerce store costs: $20-200/mo (hosting + 5-10 extensions + theme)");
    println!("  Core features (very capable, even free):");
    println!("    - Product types: simple, grouped, virtual, downloadable, variable, external");
    println!("    - Tax + shipping zones + classes");
    println!("    - Coupons + discount codes");
    println!("    - REST API (full OAuth/JWT auth)");
    println!("    - Blocks-based product/cart/checkout editor (Gutenberg integration)");
    println!("    - Block themes + Site Editor support");
    println!("    - Multi-currency via extensions");
    println!("    - Reports + analytics dashboard (improved 2020+)");
    println!("    - Customer accounts + wishlists");
    println!("  Premium extensions (the revenue engine):");
    println!("    - WooCommerce Subscriptions ($199/yr — most popular extension)");
    println!("    - WooCommerce Memberships ($199/yr — content gating)");
    println!("    - WooCommerce Bookings ($249/yr — appointment scheduling)");
    println!("    - WooCommerce Product Add-ons + Custom Fields");
    println!("    - WooCommerce Composite Products + Product Bundles");
    println!("    - Smart Coupons, Dynamic Pricing, Min/Max Quantities");
    println!("    - Stripe + Square + Authorize.net gateway extensions");
    println!("  WooPayments (Automattic's bet on integrated payments):");
    println!("    - Stripe-powered, embedded in WooCommerce admin");
    println!("    - 2.9% + 30¢ US card rates");
    println!("    - Tap-to-Pay mobile via Stripe Terminal");
    println!("    - Multi-currency + buy-now-pay-later (Klarna, Affirm) built in");
    println!("    - Trying to capture rev that previously went to Stripe + PayPal extensions");
    println!("  Third-party ecosystem (massive, but quality varies):");
    println!("    - 5,000+ extensions on WooCommerce.com marketplace");
    println!("    - Tens of thousands more from CodeCanyon, third-party shops");
    println!("    - Extension conflicts + security issues are common pain points");
    println!("    - Plugin sprawl: large stores running 30-80+ active plugins (perf + security risk)");
    println!("  Theme ecosystem:");
    println!("    - StoreFront (Automattic's free reference theme)");
    println!("    - Astra, Kadence (block themes optimized for Woo)");
    println!("    - Flatsome, Avada (legacy multi-purpose with Woo support)");
    println!("    - thousands of themes on ThemeForest specifically for WooCommerce");
    println!("    - Block themes + FSE (Full Site Editing) increasingly viable as of 2024");
    println!("  Hosting partners:");
    println!("    - WP Engine, Pressable (Automattic-owned), Kinsta, Pantheon, SiteGround");
    println!("    - WordPress.com Business + Commerce plans");
    println!("    - Cloudways, Hostinger, Bluehost (lower-end)");
    println!("    - hosting matters MORE than Shopify because YOU manage performance");
    println!("  Headless WooCommerce (growing trend):");
    println!("    - WP Engine Atlas, Frontity (acquired by Automattic), Faust.js");
    println!("    - WPGraphQL + WooGraphQL extensions for GraphQL API");
    println!("    - Use WooCommerce as backend, Next.js/Astro/Svelte as frontend");
    println!("  Customers: ~3.5M live stores, ~28% of online stores on Internet");
    println!("            Singer (sewing machines), Weber, All Blacks, Airstream, Yale, Belkin");
    println!("            Patek Philippe, Klarna corporate store");
    println!("            sweet spot: SMB content-rich stores, bloggers monetizing, agencies serving SMBs");
    println!("            very strong: blogs + content sites adding commerce, courses + memberships, services + bookings");
    println!("            weak: pure D2C (Shopify wins), enterprise (Magento/Adobe Commerce/SFCC win)");
    println!("  Critique: TCO often higher than Shopify Basic once you stack extensions + hosting + maintenance");
    println!("           security responsibility on you — keeping WordPress + plugins patched is a real job");
    println!("           performance varies wildly: cheap shared hosting → slow store; managed WP host → fast");
    println!("           checkout less polished than Shopify out-of-box (without further extensions)");
    println!("           mobile admin experience weaker than Shopify");
    println!("           Automattic's commitment to WooCommerce sometimes wavers vs other projects (Tumblr, Day One)");
    println!("           extension subscriptions: renewal sticker shock 5 years in");
    println!("  Differentiator: free + open source + on WordPress (40% of web) + content+commerce together + own your data — for merchants who already use WordPress or value content as part of commerce");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "woocommerce".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_woo(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_woo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/woocommerce"), "woocommerce");
        assert_eq!(basename(r"C:\bin\woocommerce.exe"), "woocommerce.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("woocommerce.exe"), "woocommerce");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_woo(&["--help".to_string()], "woocommerce"), 0);
        assert_eq!(run_woo(&["-h".to_string()], "woocommerce"), 0);
        let _ = run_woo(&["--version".to_string()], "woocommerce");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_woo(&[], "woocommerce");
    }
}
