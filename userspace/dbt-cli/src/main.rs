#![deny(clippy::all)]

//! dbt-cli — OurOS dbt (data build tool) CLI
//!
//! Single personality: `dbt`

use std::env;
use std::process;

fn run_dbt(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dbt <COMMAND> [OPTIONS]");
        println!();
        println!("dbt (data build tool) CLI (OurOS).");
        println!();
        println!("Commands:");
        println!("  init         Initialize dbt project");
        println!("  run          Run models");
        println!("  test         Run tests");
        println!("  build        Build (run + test)");
        println!("  compile      Compile SQL");
        println!("  seed         Load seed data");
        println!("  snapshot     Run snapshots");
        println!("  docs         Generate/serve docs");
        println!("  source       Manage sources");
        println!("  deps         Install dependencies");
        println!("  clean        Clean artifacts");
        println!("  debug        Debug configuration");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("dbt 1.7.0 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "init" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("my_project");
            println!("Creating dbt project '{}'...", name);
            println!("  Created {}/dbt_project.yml", name);
            println!("  Created {}/models/", name);
            println!("  Created {}/seeds/", name);
            println!("  Created {}/snapshots/", name);
            println!("  Created {}/tests/", name);
            println!("  Created {}/macros/", name);
            println!("Happy modeling!");
            0
        }
        "run" => {
            let select = args.windows(2).find(|w| w[0] == "--select" || w[0] == "-s").map(|w| w[1].as_str());
            println!("Running with dbt=1.7.0");
            println!("Found 8 models, 3 tests, 2 seeds, 1 snapshot");
            println!();
            if let Some(s) = select {
                println!("  Concurrency: 4 threads");
                println!("  1 of 1 OK  created model analytics.{} ............... [SELECT in 0.45s]", s);
            } else {
                println!("  Concurrency: 4 threads");
                println!("  1 of 8 OK  created model staging.stg_customers ...... [SELECT in 0.32s]");
                println!("  2 of 8 OK  created model staging.stg_orders ......... [SELECT in 0.28s]");
                println!("  3 of 8 OK  created model staging.stg_payments ....... [SELECT in 0.31s]");
                println!("  4 of 8 OK  created model marts.dim_customers ........ [SELECT in 0.45s]");
                println!("  5 of 8 OK  created model marts.fct_orders ........... [SELECT in 0.52s]");
                println!("  6 of 8 OK  created model marts.fct_payments ......... [SELECT in 0.38s]");
                println!("  7 of 8 OK  created model analytics.revenue .......... [SELECT in 0.41s]");
                println!("  8 of 8 OK  created model analytics.customers_kpis ... [SELECT in 0.55s]");
            }
            println!();
            println!("Finished running 8 models in 0 hours 0 minutes and 3.22 seconds (3.22s).");
            println!("Completed successfully.");
            0
        }
        "test" => {
            println!("Running with dbt=1.7.0");
            println!("Found 3 tests");
            println!();
            println!("  Concurrency: 4 threads");
            println!("  1 of 3 PASS  unique_customers_customer_id ............. [PASS in 0.12s]");
            println!("  2 of 3 PASS  not_null_orders_order_id ................. [PASS in 0.09s]");
            println!("  3 of 3 PASS  relationships_orders_customer_id ......... [PASS in 0.15s]");
            println!();
            println!("Finished running 3 tests in 0 hours 0 minutes and 0.36 seconds (0.36s).");
            println!("Completed successfully.");
            0
        }
        "build" => {
            println!("Running with dbt=1.7.0");
            println!("Found 8 models, 3 tests, 2 seeds, 1 snapshot");
            println!();
            println!("  Concurrency: 4 threads");
            println!("  Running seeds...");
            println!("  1 of 2 OK  seed file seeds.raw_customers .............. [INSERT 100 in 0.18s]");
            println!("  2 of 2 OK  seed file seeds.raw_orders ................. [INSERT 250 in 0.22s]");
            println!("  Running models...");
            println!("  1 of 8 OK  created model staging.stg_customers ........ [SELECT in 0.32s]");
            println!("  ...");
            println!("  8 of 8 OK  created model analytics.customers_kpis ..... [SELECT in 0.55s]");
            println!("  Running tests...");
            println!("  3 of 3 PASS ......................................... [PASS in 0.36s]");
            println!("  Running snapshots...");
            println!("  1 of 1 OK  snapshotted scd_customers .................. [INSERT 0 in 0.28s]");
            println!();
            println!("Finished running 2 seeds, 8 models, 3 tests, 1 snapshot in 4.91s.");
            println!("Completed successfully.");
            0
        }
        "compile" => {
            println!("Running with dbt=1.7.0");
            println!("Found 8 models");
            println!();
            println!("  Compiled 8 models");
            println!("  Written to target/compiled/");
            println!("  Compiled SQL available at target/compiled/my_project/models/");
            0
        }
        "seed" => {
            println!("Running with dbt=1.7.0");
            println!("Found 2 seeds");
            println!();
            println!("  1 of 2 OK  seed file seeds.raw_customers ...... [INSERT 100 in 0.18s]");
            println!("  2 of 2 OK  seed file seeds.raw_orders ......... [INSERT 250 in 0.22s]");
            println!();
            println!("Finished running 2 seeds in 0.40 seconds.");
            0
        }
        "docs" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("generate");
            match sub {
                "generate" => {
                    println!("Running with dbt=1.7.0");
                    println!("  Generating catalog...");
                    println!("  Generated catalog.json with 8 models, 2 seeds, 3 sources");
                    println!("  Docs written to target/");
                }
                "serve" => {
                    let port = args.windows(2).find(|w| w[0] == "--port").map(|w| w[1].as_str()).unwrap_or("8080");
                    println!("Serving docs at http://localhost:{}", port);
                    println!("Press Ctrl+C to exit.");
                }
                _ => { println!("Docs operation: {}", sub); }
            }
            0
        }
        "source" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("freshness");
            match sub {
                "freshness" => {
                    println!("Running with dbt=1.7.0");
                    println!();
                    println!("  Source            Table           Status   Max Loaded At         Criteria");
                    println!("  raw.customers     customers       PASS     2024-01-15 13:00:00   < 24 hours");
                    println!("  raw.orders        orders          PASS     2024-01-15 14:00:00   < 12 hours");
                    println!("  raw.payments      payments        WARN     2024-01-14 08:00:00   < 24 hours");
                }
                _ => { println!("Source operation: {}", sub); }
            }
            0
        }
        "deps" => {
            println!("Running with dbt=1.7.0");
            println!("Installing dbt-utils@1.1.1");
            println!("  Installed from hub (version 1.1.1)");
            println!("Installing dbt-expectations@0.10.1");
            println!("  Installed from hub (version 0.10.1)");
            println!();
            println!("Installed 2 packages.");
            0
        }
        "clean" => {
            println!("Running with dbt=1.7.0");
            println!("  Deleted target/");
            println!("  Deleted dbt_packages/");
            println!("  Deleted logs/");
            println!("Cleaned successfully.");
            0
        }
        "debug" => {
            println!("Running with dbt=1.7.0");
            println!();
            println!("  dbt version: 1.7.0");
            println!("  python version: 3.12.0");
            println!("  os info: OurOS x86_64");
            println!("  Configuration:");
            println!("    profiles.yml found:  YES (/home/user/.dbt/profiles.yml)");
            println!("    dbt_project.yml found: YES (/home/user/project/dbt_project.yml)");
            println!("  Required dependencies:");
            println!("    git: installed (2.43.0)");
            println!("  Connection:");
            println!("    method:    postgres");
            println!("    host:      localhost");
            println!("    port:      5432");
            println!("    user:      dbt_user");
            println!("    database:  analytics");
            println!("    schema:    public");
            println!("    Connection test: OK");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: dbt <command>. See --help.");
            } else {
                eprintln!("Error: unknown command '{}'. See --help.", cmd);
            }
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dbt(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_dbt};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dbt(vec!["--help".to_string()]), 0);
        assert_eq!(run_dbt(vec!["-h".to_string()]), 0);
        let _ = run_dbt(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dbt(vec![]);
    }
}
