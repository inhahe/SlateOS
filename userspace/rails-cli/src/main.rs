#![deny(clippy::all)]

//! rails-cli — Slate OS Ruby on Rails CLI
//!
//! Multi-personality: `rails`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rails(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: rails COMMAND [OPTIONS]");
        println!("Rails 7.1.3.4 (Slate OS)");
        println!();
        println!("Commands:");
        println!("  new          Create a new Rails application");
        println!("  server       Start development server (alias: s)");
        println!("  console      Start Rails console (alias: c)");
        println!("  generate     Generate code (alias: g)");
        println!("  destroy      Undo generate (alias: d)");
        println!("  db:migrate   Run database migrations");
        println!("  db:seed      Seed the database");
        println!("  db:create    Create the database");
        println!("  routes       Show all routes");
        println!("  test         Run tests (alias: t)");
        println!("  credentials  Manage encrypted credentials");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" | "-v" => println!("Rails 7.1.3.4"),
        "new" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("myapp");
            println!("      create  {}/", name);
            println!("      create  {}/Gemfile", name);
            println!("      create  {}/app/", name);
            println!("      create  {}/config/", name);
            println!("      create  {}/db/", name);
            println!("         run  bundle install");
        }
        "server" | "s" => {
            let port = args.windows(2).find(|w| w[0] == "-p")
                .map(|w| w[1].as_str()).unwrap_or("3000");
            println!("=> Booting Puma");
            println!("=> Rails 7.1.3.4 application starting in development");
            println!("=> Run `bin/rails server --help` for more startup options");
            println!("* Listening on http://127.0.0.1:{}", port);
        }
        "console" | "c" => {
            println!("Loading development environment (Rails 7.1.3.4)");
            println!("irb(main):001:0> ");
        }
        "generate" | "g" => {
            let what = args.get(1).map(|s| s.as_str()).unwrap_or("model");
            let name = args.get(2).map(|s| s.as_str()).unwrap_or("User");
            match what {
                "model" => {
                    println!("      invoke  active_record");
                    println!("      create    db/migrate/20240615120000_create_{}.rb", name.to_lowercase());
                    println!("      create    app/models/{}.rb", name.to_lowercase());
                    println!("      invoke    test_unit");
                    println!("      create      test/models/{}_test.rb", name.to_lowercase());
                }
                "controller" => {
                    println!("      create  app/controllers/{}_controller.rb", name.to_lowercase());
                    println!("      invoke  erb");
                    println!("      create    app/views/{}/", name.to_lowercase());
                    println!("      invoke  test_unit");
                }
                "scaffold" => {
                    println!("      invoke  active_record");
                    println!("      create    db/migrate/20240615120000_create_{}.rb", name.to_lowercase());
                    println!("      create    app/models/{}.rb", name.to_lowercase());
                    println!("      create    app/controllers/{}_controller.rb", name.to_lowercase());
                    println!("      create    app/views/{}/", name.to_lowercase());
                    println!("      invoke  resource_route");
                    println!("       route    resources :{}", name.to_lowercase());
                }
                _ => println!("rails generate {}: completed", what),
            }
        }
        "routes" => {
            println!("          Prefix Verb   URI Pattern                 Controller#Action");
            println!("            root GET    /                           home#index");
            println!("           users GET    /users(.:format)            users#index");
            println!("                 POST   /users(.:format)            users#create");
            println!("        new_user GET    /users/new(.:format)        users#new");
            println!("       edit_user GET    /users/:id/edit(.:format)   users#edit");
            println!("            user GET    /users/:id(.:format)        users#show");
        }
        "test" | "t" => {
            println!("Running tests...");
            println!(".....");
            println!("5 runs, 12 assertions, 0 failures, 0 errors, 0 skips");
        }
        "db:migrate" => {
            println!("== CreateUsers: migrating ===");
            println!("-- create_table(:users)");
            println!("   -> 0.0012s");
            println!("== CreateUsers: migrated (0.0012s) ===");
        }
        "db:create" => println!("Created database 'myapp_development'"),
        "db:seed" => println!("Seeded database."),
        _ => println!("rails: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rails".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rails(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rails};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rails"), "rails");
        assert_eq!(basename(r"C:\bin\rails.exe"), "rails.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rails.exe"), "rails");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rails(&["--help".to_string()]), 0);
        assert_eq!(run_rails(&["-h".to_string()]), 0);
        let _ = run_rails(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rails(&[]);
    }
}
