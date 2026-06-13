#![deny(clippy::all)]

//! collibra-cli — Slate OS Collibra (data intelligence + governance, Brussels + NYC)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_collibra(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: collibra [OPTIONS]");
        println!("Collibra (Slate OS) — data intelligence platform (catalog + governance leader)");
        println!();
        println!("Options:");
        println!("  --catalog              Browse cataloged assets");
        println!("  --governance           Data Governance Center (the flagship)");
        println!("  --quality              Data Quality + Observability (acquired OwlDQ)");
        println!("  --privacy              Privacy + Risk module (CCPA/GDPR mapping)");
        println!("  --ai-governance        AI Governance (model registry + risk, 2024)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Collibra 2024.07 (Slate OS)"); return 0; }
    println!("Collibra 2024.07 (Slate OS) — Data Intelligence Platform");
    println!("  Vendor: Collibra NV (Brussels, Belgium + New York City)");
    println!("  Founders: Felix Van de Maele (CEO) + Stijn Christiaens + Pieter De Leenheer, 2008");
    println!("          Felix: Belgian data scientist, also founded later companies");
    println!("          spun out of STARLab at Vrije Universiteit Brussel (semantic web research)");
    println!("          16+ years building — one of the oldest still-private 'data intelligence' vendors");
    println!("          dual HQ: Brussels (engineering) + NYC (sales)");
    println!("  Funding: ~$600M total — Decacorn ($5.25B valuation 2021)");
    println!("         Series G Nov 2021: $250M led by Sequoia + Tiger + Battery + ICONIQ");
    println!("         peak $5.25B valuation 2021");
    println!("         valuation reportedly cut to ~$2B in 2023 secondary sales (down round)");
    println!("         layoffs 2023 (~15% headcount) + 2024 — typical post-2021 zirp adjustment");
    println!("  ARR: $250M+ (largest pure-play catalog/governance vendor)");
    println!("  Strategic position: 'governance-first' data intelligence (vs Alation's catalog-first):");
    println!("                    pitch: 'enterprise data governance + catalog + quality + privacy in one'");
    println!("                    contrast: Alation more catalog UX-focused; Collibra more governance + workflow depth");
    println!("                    primary competitor: Alation (head-to-head), Informatica, Atlan, IBM, Microsoft Purview");
    println!("                    moat: 16-year head start on enterprise governance feature breadth");
    println!("                    sales motion: 6-18 month sales cycles, Fortune 500 dominant");
    println!("                    Gartner Magic Quadrant Leader for data governance year after year");
    println!("  Pricing (enterprise, no free tier):");
    println!("    Standard — $100K-300K/yr");
    println!("    Premium — $300K-1M/yr (full modules)");
    println!("    Enterprise — $500K-5M+/yr (Fortune 500, multi-module, on-prem option)");
    println!("    per-module pricing — quality / privacy / lineage often add-ons");
    println!("    on-prem deployments still significant (banks, regulated industries)");
    println!("  Platform modules:");
    println!("    1. Data Catalog — discover + document data assets");
    println!("    2. Data Governance Center — policies, workflows, stewardship (the flagship)");
    println!("    3. Data Quality + Observability — acquired OwlDQ (2021)");
    println!("    4. Data Privacy — PII mapping + regulatory reporting (GDPR/CCPA/HIPAA)");
    println!("    5. Data Lineage — automated cross-stack lineage");
    println!("    6. AI Governance — model registry + risk (2024 push for GenAI enterprise)");
    println!("    7. Protect — column/row-level access controls (2023 acquisition)");
    println!("  Data Governance Center (DGC — the historical flagship):");
    println!("    - Policy lifecycle management (draft → approve → enforce)");
    println!("    - Workflow engine for stewardship + access requests + change approvals");
    println!("    - Business glossary with multi-language support");
    println!("    - Hierarchical taxonomies + ontologies (semantic web heritage)");
    println!("    - Regulatory mapping: SOX, BCBS 239, GDPR, CCPA, HIPAA, PCI-DSS, FATCA");
    println!("    - Council/committee modeling for data governance committees");
    println!("  Acquisitions (the M&A history):");
    println!("    - OwlDQ (2021) — data quality + observability");
    println!("    - SQLdep (2018) — automated lineage parsing");
    println!("    - Privitar discussion (2022) — privacy/de-identification, deal fell through");
    println!("    - Husprey (2022) — data notebooks");
    println!("    - Decube partnership (catalog augmentation)");
    println!("  AI Governance (2024 push):");
    println!("    - ML/AI model registry + lineage to training data");
    println!("    - Risk + compliance tracking for EU AI Act, NIST AI RMF");
    println!("    - Model card generation + bias monitoring");
    println!("    - Compete with: Credo AI, Fiddler, Holistic AI, Arthur, Domino");
    println!("    - bet: AI Governance becomes mandatory like data governance was 2010s");
    println!("  Collibra AI (2023+):");
    println!("    - Natural language search across catalog");
    println!("    - Auto-generated descriptions + business context");
    println!("    - Chat with your catalog (GPT-4 + Anthropic)");
    println!("    - Compete with Alation Anywhere + Atlan AI");
    println!("  Connectors (200+):");
    println!("    - Warehouses: Snowflake, BigQuery, Databricks, Redshift, Synapse, Teradata, Netezza, Oracle");
    println!("    - Legacy: SAP, mainframe, Cobol/DB2 (still relevant for bank customers)");
    println!("    - BI: Tableau, Power BI, Looker, Qlik, Cognos, MicroStrategy, SAP BO");
    println!("    - ETL: Informatica, Talend, Ab Initio, IBM DataStage, dbt, Airflow, Fivetran");
    println!("    - Cloud: AWS Glue, Azure Purview ingestion, GCP Dataplex");
    println!("  Collibra CLI usage:");
    println!("    collibra login --instance company.collibra.com");
    println!("    collibra asset list --type Table --domain Finance");
    println!("    collibra policy assign --asset 'customer_pii' --policy 'GDPR-restricted'");
    println!("    collibra steward assign --domain customer --user alice");
    println!("    collibra workflow start --type access-request --asset orders");
    println!("  Customers (~700+ paying):");
    println!("    - Bank of America, ABN AMRO, Deutsche Bank, BNP Paribas, ING, Goldman Sachs");
    println!("    - Pfizer, GSK, Novartis, AstraZeneca");
    println!("    - Walmart, Target, UPS, FedEx, Heineken, Adobe");
    println!("    - Federal Reserve, US Treasury, multiple central banks");
    println!("    - Fortune 500 dominant — 60%+ of largest banks globally use Collibra");
    println!("    - 7+ year average contract length (high stickiness)");
    println!("  Critique: heavyweight + complex to deploy (typical 6-12 month implementations)");
    println!("           UX dated vs Atlan/modern catalogs — admins love it, analysts less so");
    println!("           expensive even by enterprise standards ($500K+ floor common)");
    println!("           OwlDQ integration still incomplete after 3+ years");
    println!("           Snowflake Horizon + Databricks Unity threaten from warehouse layer");
    println!("           IPO repeatedly delayed — 2021 funding round was last major raise");
    println!("           valuation cut 2023 + layoffs = pressure on growth narrative");
    println!("           Atlan winning modern-data-stack greenfield in tech sector");
    println!("           limited self-service — typically requires consultants to deploy");
    println!("  Differentiator: 16-year head start + Fortune 500 governance dominance (especially banking + pharma + government) + deepest workflow engine + AI Governance early mover + 200+ connectors including legacy SAP/mainframe — the enterprise governance choice for regulated industries");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "collibra".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_collibra(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_collibra};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/collibra"), "collibra");
        assert_eq!(basename(r"C:\bin\collibra.exe"), "collibra.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("collibra.exe"), "collibra");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_collibra(&["--help".to_string()], "collibra"), 0);
        assert_eq!(run_collibra(&["-h".to_string()], "collibra"), 0);
        let _ = run_collibra(&["--version".to_string()], "collibra");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_collibra(&[], "collibra");
    }
}
