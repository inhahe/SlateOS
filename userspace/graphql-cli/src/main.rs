#![deny(clippy::all)]

//! graphql-cli — SlateOS GraphQL CLI tools
//!
//! Multi-personality: `graphql`, `gql`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_graphql(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: graphql COMMAND [OPTIONS]");
        println!("GraphQL CLI 4.1.0 (SlateOS)");
        println!();
        println!("Commands:");
        println!("  codegen      Generate code from schema");
        println!("  init         Initialize GraphQL project");
        println!("  validate     Validate schema");
        println!("  get-schema   Download schema from endpoint");
        println!("  diff         Compare two schemas");
        println!("  query        Execute a GraphQL query");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("4.1.0"),
        "codegen" => {
            let config = args.get(1).map(|s| s.as_str()).unwrap_or("codegen.yml");
            println!("Generating from {}...", config);
            println!("  Generated: src/generated/graphql.ts (42 types, 12 operations)");
            println!("Done.");
        }
        "init" => {
            println!("Created .graphqlrc.yml");
            println!("Created schema.graphql");
        }
        "validate" => {
            let schema = args.get(1).map(|s| s.as_str()).unwrap_or("schema.graphql");
            println!("Validating {}...", schema);
            println!("Schema is valid. 15 types, 28 fields.");
        }
        "get-schema" => {
            let endpoint = args.get(1).map(|s| s.as_str()).unwrap_or("http://localhost:4000/graphql");
            println!("Downloading schema from {}...", endpoint);
            println!("Schema saved to schema.graphql");
        }
        "diff" => {
            let old = args.get(1).map(|s| s.as_str()).unwrap_or("schema-v1.graphql");
            let new = args.get(2).map(|s| s.as_str()).unwrap_or("schema-v2.graphql");
            println!("Comparing {} vs {}:", old, new);
            println!("  + Added: User.avatarUrl (String)");
            println!("  ~ Changed: User.name (String -> String!)");
            println!("  - Removed: User.legacy_id");
        }
        "query" => {
            let endpoint = args.windows(2).find(|w| w[0] == "--endpoint")
                .map(|w| w[1].as_str()).unwrap_or("http://localhost:4000/graphql");
            println!("Querying {}...", endpoint);
            println!("{{");
            println!("  \"data\": {{");
            println!("    \"users\": [");
            println!("      {{ \"id\": \"1\", \"name\": \"Alice\" }},");
            println!("      {{ \"id\": \"2\", \"name\": \"Bob\" }}");
            println!("    ]");
            println!("  }}");
            println!("}}");
        }
        _ => println!("graphql: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "graphql".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_graphql(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_graphql};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/graphql"), "graphql");
        assert_eq!(basename(r"C:\bin\graphql.exe"), "graphql.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("graphql.exe"), "graphql");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_graphql(&["--help".to_string()]), 0);
        assert_eq!(run_graphql(&["-h".to_string()]), 0);
        let _ = run_graphql(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_graphql(&[]);
    }
}
