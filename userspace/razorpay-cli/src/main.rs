#![deny(clippy::all)]
//! razorpay-cli — Slate OS Razorpay India payments personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — Razorpay India payments + banking suite (personality)");
    println!();
    println!("USAGE:");
    println!("    {prog} <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about         Harshil + Shashank, YC W15, Bangalore");
    println!("    upi           UPI deep dive (the India payments singularity)");
    println!("    api           Payments + Orders + Refunds API");
    println!("    suite         RazorpayX (banking), Capital (lending), Payroll");
    println!("    methods       UPI, cards, netbanking, wallets, EMI, BNPL");
    println!("    rbi           RBI regulatory context (PA-PG, tokenization)");
    println!("    redomicile    The 2024 redomicile-to-India story");
    println!("    help / version");
}

fn print_version() {
    println!("razorpay-cli 0.1.0 — Slate OS personality binary");
    println!("Razorpay Software Pvt Ltd — Bengaluru (Bangalore), India");
}

fn cmd_about() {
    println!("Razorpay — The full-stack financial solutions company.");
    println!();
    println!("Founded:  2014 in Jaipur, then moved to Bangalore");
    println!("Founders: Harshil Mathur (CEO, IIT Roorkee) +");
    println!("          Shashank Kumar (CTO, IIT Roorkee)");
    println!("Backers:  Y Combinator W15 batch — early Indian YC graduate");
    println!();
    println!("Funding milestones:");
    println!("  2015 (YC):  Initial USD 120k YC investment");
    println!("  2018 Ser B: USD 20M (Matrix, Tiger Global, MasterCard, et al.)");
    println!("  2020 Ser C: USD 75M");
    println!("  2020 Ser D: USD 100M at USD 1B+ — first unicorn round");
    println!("  2021 Ser E: USD 160M at USD 3B");
    println!("  Dec 2021:   USD 375M Series F at USD 7.5B valuation");
    println!("              Lead: Lone Pine + Alkeon + TCV + GIC + Tiger + Sequoia");
    println!();
    println!("Positioning:");
    println!("  • India-first, India-only (does not target outside-India volumes)");
    println!("  • Full financial OS for SMBs: payments + banking + payroll + credit");
    println!("  • Famous developer experience (Indian Stripe analog)");
    println!("  • Strong play in subscription / recurring (eMandate, eNACH, UPI AutoPay)");
}

fn cmd_upi() {
    println!("UPI — Unified Payments Interface");
    println!("(The most important payment rail in India, and arguably the world's");
    println!(" most successful instant payment system.)");
    println!();
    println!("What it is:");
    println!("  Real-time, account-to-account, 24/7/365 instant payment rails");
    println!("  operated by NPCI (National Payments Corporation of India).");
    println!("  Launched April 2016. Free for consumers. Effectively free for");
    println!("  merchants below INR 2000 (P2M MDR is zero by RBI mandate).");
    println!();
    println!("Volumes (monthly, India):");
    println!("  • Apr 2016 (launch):   ~100,000 transactions/month");
    println!("  • Dec 2020:            ~2 billion txns / month");
    println!("  • Dec 2023:            ~12 billion txns / month, USD 200B+ value");
    println!("  • Late 2024:           ~16-18 billion txns / month");
    println!("  This is more transactions than Visa + Mastercard globally combined.");
    println!();
    println!("How it works (collect flow):");
    println!("  1. Merchant calls Razorpay API to create a 'collect request'");
    println!("  2. Customer enters or selects their VPA (e.g. user@upi)");
    println!("  3. Customer's PSP app (PhonePe / GPay / Paytm / BHIM) pops a prompt");
    println!("  4. Customer enters UPI PIN to authorize");
    println!("  5. Money moves bank-to-bank instantly via NPCI rails");
    println!("  6. Razorpay webhooks merchant with status (~5-30 seconds typical)");
    println!();
    println!("Intent flow (preferred, lower friction):");
    println!("  Customer scans QR or taps deep-link -> PSP opens with prefilled");
    println!("  payment -> one-tap authorize. Most successful conversion path.");
    println!();
    println!("UPI AutoPay (recurring):");
    println!("  RBI-approved mandate flow for subscription billing. Caps:");
    println!("  INR 15,000 per transaction without re-authentication, higher with");
    println!("  additional factor. Razorpay was an early integrator.");
}

fn cmd_api() {
    println!("Razorpay API");
    println!();
    println!("Base URL: https://api.razorpay.com/v1");
    println!("Auth:     HTTP Basic — username=key_id, password=key_secret");
    println!("          Test keys prefixed rzp_test_, live keys prefixed rzp_live_");
    println!();
    println!("The two-phase flow (Orders + Payments):");
    println!();
    println!("  1. Server: POST /orders");
    println!("     {{ amount: 50000, currency: 'INR', receipt: 'ord_42' }}");
    println!("     -> returns order_id (e.g. order_K9Z...)");
    println!();
    println!("  2. Client: render Razorpay Checkout (JS SDK) with order_id");
    println!("     Customer picks UPI / card / netbanking / wallet inside the modal");
    println!("     On success, callback delivers razorpay_payment_id + signature");
    println!();
    println!("  3. Server: verify signature with HMAC-SHA256(order_id|payment_id, key)");
    println!("     Then POST /payments/{{id}}/capture (if auto-capture is off)");
    println!();
    println!("Subscriptions:");
    println!("  POST /plans               — create a billing plan (interval+amount)");
    println!("  POST /subscriptions       — create a subscription tied to a customer");
    println!("  POST /customers           — create a customer");
    println!("  POST /tokens              — create a recurring token (eMandate/UPI AutoPay)");
    println!();
    println!("Smart Collect:");
    println!("  Virtual account numbers + UPI VPAs for receiving bulk NEFT/RTGS/IMPS");
    println!("  payments — Razorpay auto-reconciles incoming credits to invoices.");
}

fn cmd_suite() {
    println!("The Razorpay product suite (beyond payments)");
    println!();
    println!("Payments (core):");
    println!("  Standard checkout, UPI, cards, netbanking, wallets, BNPL,");
    println!("  recurring, payment links, payment pages, route (split payments)");
    println!();
    println!("RazorpayX (business banking):");
    println!("  Current accounts opened in partnership with RBL Bank / ICICI.");
    println!("  Bulk payouts (INR -> bank/UPI/card), tax payments, vendor payments,");
    println!("  multi-user approval workflows, expense cards, current account APIs.");
    println!();
    println!("Razorpay Capital (lending):");
    println!("  Working capital loans for merchants — underwritten using the");
    println!("  merchant's own Razorpay transaction history as risk signal.");
    println!("  Same-day disbursement, repayment as % of daily settlement.");
    println!();
    println!("Razorpay Payroll (Opfin, acquired 2019):");
    println!("  Cloud payroll + compliance (PF, ESI, TDS, professional tax).");
    println!("  Direct salary deposits via RazorpayX rails.");
    println!();
    println!("RazorpayPOS / Magic Checkout / Thirdwatch (fraud):");
    println!("  In-store, one-click web, and AI risk scoring respectively.");
    println!();
    println!("Curlec (Malaysia, acquired Nov 2022):");
    println!("  Direct debit + payments PSP for Malaysia — Razorpay's first");
    println!("  cross-border expansion (still essentially India-adjacent SEA).");
    println!();
    println!("The thesis: own every financial workflow for an Indian SMB —");
    println!("not just the payment, but the bank account, the loan, the payroll.");
}

fn cmd_methods() {
    println!("Payment methods supported by Razorpay");
    println!();
    println!("UPI (dominant, ~70%+ of consumer transactions):");
    println!("  Collect requests, intent (QR / deep-link), UPI AutoPay (recurring)");
    println!();
    println!("Cards:");
    println!("  Visa, Mastercard, RuPay, American Express, Diners");
    println!("  EMI on cards (3 / 6 / 9 / 12 / 18 / 24 month plans, bank-specific)");
    println!("  No-cost EMI (merchant absorbs interest)");
    println!();
    println!("Netbanking:");
    println!("  60+ Indian banks — SBI, HDFC, ICICI, Axis, Kotak, Yes, IDFC...");
    println!();
    println!("Wallets:");
    println!("  Paytm Wallet, Amazon Pay, PhonePe Wallet, Mobikwik, FreeCharge,");
    println!("  Airtel Money, Ola Money, JioMoney");
    println!();
    println!("BNPL / pay-later:");
    println!("  Simpl, LazyPay, ICICI Pay Later, Flipkart Pay Later, ZestMoney");
    println!("  (note: ZestMoney shut down Dec 2023 — fintech consolidation)");
    println!();
    println!("Bank transfers (Smart Collect):");
    println!("  NEFT, RTGS, IMPS to virtual account numbers");
    println!();
    println!("International cards: accepted but subject to RBI cross-border rules");
    println!("                     and merchant onboarding eligibility");
}

fn cmd_rbi() {
    println!("RBI regulatory context — why Indian payments are unique");
    println!();
    println!("RBI = Reserve Bank of India. Central bank. Aggressive regulator");
    println!("of fintech. Sets the rules every Indian PSP lives or dies by.");
    println!();
    println!("PA-PG license (Payment Aggregator / Payment Gateway):");
    println!("  Introduced 2020. Required by ANY entity aggregating merchant funds.");
    println!("  Razorpay received in-principle approval July 2022, then was");
    println!("  granted final PA license July 2023 (after a temporary pause on");
    println!("  new merchant onboarding from Dec 2022 to Jun 2023 while RBI");
    println!("  reviewed). This affected ALL major PSPs including Cashfree, PayU.");
    println!();
    println!("Card tokenization mandate (Sep 2022 enforcement):");
    println!("  Merchants and PSPs may no longer store raw card numbers — only");
    println!("  network-issued tokens. Massive industry retooling required.");
    println!("  Razorpay was first to implement tokenization at scale.");
    println!();
    println!("Recurring payments rules (Oct 2021):");
    println!("  Standing instruction on cards capped at INR 15,000 without");
    println!("  per-transaction additional factor of authentication. Disrupted");
    println!("  card-based subscription models industry-wide. Pushed everyone");
    println!("  toward UPI AutoPay + eMandate.");
    println!();
    println!("Cross-border rules:");
    println!("  PA-CB (Payment Aggregator - Cross Border) license required for");
    println!("  outbound INR remittance flows. Separate regime, in flux 2024-2025.");
    println!();
    println!("MDR caps:");
    println!("  P2M UPI: zero MDR (subsidized by Govt of India)");
    println!("  RuPay debit (low value): zero MDR");
    println!("  Other cards: market-rate (typically 1.5-2.5% all-in)");
}

fn cmd_redomicile() {
    println!("Razorpay redomicile — the 2024 move back to India");
    println!();
    println!("Background:");
    println!("  Like many Indian unicorns, Razorpay's holding company was");
    println!("  incorporated in Delaware (USA) during its YC + global VC era.");
    println!("  The operating subsidiary was always in India, but cap-table");
    println!("  and IPO-eligible entity sat overseas.");
    println!();
    println!("The move:");
    println!("  Announced 2023, executed through 2024. Razorpay 'flipped' the");
    println!("  holding structure: Indian entity becomes the parent, Delaware");
    println!("  entity becomes a subsidiary (or is unwound).");
    println!();
    println!("Why:");
    println!("  • SEBI requires Indian incorporation for an Indian IPO");
    println!("  • Razorpay public statements: IPO planned for 2025-2026 on NSE/BSE");
    println!("  • RBI / GoI political pressure for Indian-domiciled fintech");
    println!("  • Better access to Indian retail investor capital");
    println!();
    println!("Tax cost:");
    println!("  Redomicile triggered ~USD 300M+ tax outflow (reported press");
    println!("  estimate) to Indian and US authorities. PhonePe paid even more");
    println!("  (~USD 1B) for its earlier flip.");
    println!();
    println!("Other Indian fintechs that flipped:");
    println!("  PhonePe (2022), Pine Labs (2024), Groww (2024), Zepto (2024)");
    println!();
    println!("Razorpay's flip is widely viewed as the largest in Indian fintech");
    println!("by net asset value at time of move. Setup for an eventual IPO.");
}

fn run_razorpay(args: &[String], prog: &str) -> i32 {
    let Some(sub) = args.first() else {
        print_help(prog);
        return 0;
    };
    match sub.as_str() {
        "about" => cmd_about(),
        "upi" => cmd_upi(),
        "api" => cmd_api(),
        "suite" => cmd_suite(),
        "methods" => cmd_methods(),
        "rbi" => cmd_rbi(),
        "redomicile" => cmd_redomicile(),
        "help" | "--help" | "-h" => print_help(prog),
        "version" | "--version" | "-V" => print_version(),
        other => {
            eprintln!("{prog}: unknown subcommand '{other}'");
            eprintln!("Try '{prog} help' for the list of subcommands.");
            return 2;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "razorpay-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_razorpay(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/razorpay-cli"), "razorpay-cli");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("razorpay-cli.exe"), "razorpay-cli");
    }

    #[test]
    fn help_returns_zero() {
        let _ = run_razorpay(&[], "razorpay-cli");
    }

    #[test]
    fn unknown_subcommand_returns_two() {
        assert_eq!(run_razorpay(&["bogus".into()], "razorpay-cli"), 2);
    }
}
