#![deny(clippy::all)]
//! mercury-cli — personality CLI for Mercury, the YC-founder-favoured
//! US business banking platform.
//!
//! Founded 2017 in San Francisco by Immad Akhund (CEO, Heyzap co-founder
//! sold to Fyber, previously YC partner), Jason Zhang, and Max Tagher.
//! Mercury targets the very specific bottleneck that startup founders
//! complained about loudest: traditional US business banks (Wells, BoA,
//! Chase) are slow, paperwork-heavy, and unfriendly to early-stage
//! companies without revenue. Mercury sits in front of FDIC-insured partner
//! banks (Choice, Evolve, Column) and provides the actually-good UI +
//! API on top. Reached $5B+ valuation in a 2024 secondary; profitable.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Mercury YC-founder business banking personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Akhund 2017 SF, ex-Heyzap/YC partner");
    println!("    accounts      Checking + savings, FDIC-insured via partners");
    println!("    treasury      Yield via money-market sweep");
    println!("    iocredit      IO Credit Card 1.5% cashback, no PG");
    println!("    api           Programmatic banking + AP automation");
    println!("    ventureapps   Venture Debt + Mercury Raise demo day");
    println!("    bankingpanic  Mar 2023 SVB run + sweep-product reaction");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("mercury-cli 0.1.0 (startup-banking personality build)"); }

fn run_about() {
    println!("Mercury Technologies, Inc.");
    println!("  Founded:    2017, San Francisco.");
    println!("  Founders:   Immad Akhund (CEO; previously sold Heyzap to Fyber,");
    println!("              YC partner ~2014-2016), Jason Zhang, Max Tagher.");
    println!("  Model:      Not a bank itself; the user-facing experience on top");
    println!("              of partner banks Choice, Evolve, Column.");
    println!("  Valuation:  Secondary at ~$3.5B-5B 2024 (variously reported);");
    println!("              profitable.");
    println!("  Customers:  200K+ startups + SMBs banking on Mercury.");
}

fn run_accounts() {
    println!("Accounts — FDIC-insured via partner banks.");
    println!("  Checking + savings with full ACH, wire, check deposit, debit card.");
    println!("  FDIC insurance up to $5M via sweep across partner banks.");
    println!("  Virtual + physical debit cards with per-card limits + freeze.");
    println!("  Free domestic + international USD wires up to a per-month cap.");
    println!("  No monthly fees, no minimum balances, no overdraft.");
}

fn run_treasury() {
    println!("Mercury Treasury — yield product.");
    println!("  Sweeps idle balance into money-market funds (BlackRock + Vanguard).");
    println!("  Daily liquidity; treated as on-balance-sheet for the customer.");
    println!("  Yields track short-term US Treasury rates (e.g. ~4-5% recently).");
    println!("  No lockup, no minimum, no withdrawal penalties.");
    println!("  Designed to replace 'park it in a brokerage' workflow for startups.");
}

fn run_iocredit() {
    println!("IO Credit Card.");
    println!("  Charge card paid in full from the Mercury checking account.");
    println!("  1.5% cashback on all purchases.");
    println!("  No personal guarantee required; underwritten on company cashflow.");
    println!("  Per-card spend limits, employee cards, real-time merchant lock.");
    println!("  Receipt + memo capture for accounting export.");
}

fn run_api() {
    println!("Mercury API.");
    println!("  REST API for balances, transactions, transfers, recipients.");
    println!("  OAuth2 auth, sandbox + production environments.");
    println!("  Webhooks for incoming/outgoing transactions, balance thresholds.");
    println!("  AP automation: scheduled payments, bulk uploads, multi-approver");
    println!("  workflow with role-based controls.");
    println!("  Bookkeeping integrations: QuickBooks Online, Xero, NetSuite.");
}

fn run_ventureapps() {
    println!("Founder-network features.");
    println!("  Mercury Raise: programmatic intro service connecting Mercury");
    println!("  startups to ~250 vetted investors — a built-in demo day.");
    println!("  Mercury Venture Debt: revenue-linked debt instruments for");
    println!("  startups with traction (alternative to dilutive equity).");
    println!("  Founders Edition: extra perks for early Mercury customers.");
}

fn run_bankingpanic() {
    println!("March 2023 SVB collapse — Mercury's reaction.");
    println!("  When SVB went into FDIC receivership, Mercury was a prime");
    println!("  beneficiary of deposit flight: thousands of YC + venture-backed");
    println!("  startups opened Mercury accounts in days.");
    println!("  Mercury shipped 'Mercury Vault' (sweep to $5M FDIC) within weeks");
    println!("  to address concentration-risk concerns the panic exposed.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  Substack, Linear, Ramp (cross-fintech, both YC-era), Notion (early),");
    println!("  Cursor, Anthropic (early), a16z portfolio long tail.");
    println!("  Heavy adoption in YC Winter/Summer batches; default banking for");
    println!("  many recent startup cohorts.");
    println!("  Also long tail of indie + bootstrapped SaaS founders.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "mercury-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "accounts" => run_accounts(),
        "treasury" => run_treasury(),
        "iocredit" => run_iocredit(),
        "api" => run_api(),
        "ventureapps" => run_ventureapps(),
        "bankingpanic" => run_bankingpanic(),
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
        run_accounts();
        run_treasury();
        run_iocredit();
        run_api();
        run_ventureapps();
        run_bankingpanic();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("mercury-cli");
        print_version();
    }
}
