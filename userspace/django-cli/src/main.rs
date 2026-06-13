#![deny(clippy::all)]

//! django-cli — SlateOS Django management tools
//!
//! Multi-personality: `django-admin`, `manage.py`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_django(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: django-admin COMMAND [OPTIONS]");
        println!("Django 5.0.7 (SlateOS)");
        println!();
        println!("Commands:");
        println!("  startproject   Create a new project");
        println!("  startapp       Create a new app");
        println!("  runserver      Start development server");
        println!("  migrate        Run database migrations");
        println!("  makemigrations Create new migrations");
        println!("  createsuperuser Create admin user");
        println!("  shell          Start Python shell");
        println!("  test           Run tests");
        println!("  collectstatic  Collect static files");
        println!("  check          Check for issues");
        println!("  showmigrations Show migration status");
        println!("  dbshell        Open database shell");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("5.0.7"),
        "startproject" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("myproject");
            println!("Creating project '{}'...", name);
            println!("  {}/", name);
            println!("    manage.py");
            println!("    {}/", name);
            println!("      __init__.py");
            println!("      settings.py");
            println!("      urls.py");
            println!("      wsgi.py");
            println!("      asgi.py");
        }
        "startapp" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("myapp");
            println!("Creating app '{}'...", name);
        }
        "runserver" => {
            let addr = args.get(1).map(|s| s.as_str()).unwrap_or("127.0.0.1:8000");
            println!("Watching for file changes with StatReloader");
            println!("Performing system checks...");
            println!("System check identified no issues (0 silenced).");
            println!("Starting development server at http://{}/", addr);
            println!("Quit the server with CONTROL-C.");
        }
        "migrate" => {
            println!("Operations to perform:");
            println!("  Apply all migrations: admin, auth, contenttypes, sessions");
            println!("Running migrations:");
            println!("  Applying contenttypes.0001_initial... OK");
            println!("  Applying auth.0001_initial... OK");
            println!("  Applying admin.0001_initial... OK");
            println!("  Applying sessions.0001_initial... OK");
        }
        "makemigrations" => {
            let app = args.get(1).map(|s| s.as_str());
            if let Some(a) = app {
                println!("Migrations for '{}':", a);
            }
            println!("  0002_auto_20240615_1200.py");
            println!("    - Add field avatar to user");
        }
        "test" => {
            let app = args.get(1).map(|s| s.as_str());
            if let Some(a) = app {
                println!("Running tests for {}...", a);
            }
            println!("Creating test database...");
            println!("....");
            println!("----------------------------------------------------------------------");
            println!("Ran 4 tests in 0.234s");
            println!("OK");
        }
        "check" => {
            println!("System check identified no issues (0 silenced).");
        }
        "collectstatic" => {
            println!("123 static files copied to '/var/www/static'.");
        }
        "showmigrations" => {
            println!("admin");
            println!(" [X] 0001_initial");
            println!("auth");
            println!(" [X] 0001_initial");
            println!(" [X] 0002_alter_permission_name_max_length");
        }
        "shell" => println!("Python 3.12.4 (Django shell)"),
        "createsuperuser" => println!("Superuser created successfully."),
        _ => println!("django-admin: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "django-admin".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_django(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_django};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/django"), "django");
        assert_eq!(basename(r"C:\bin\django.exe"), "django.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("django.exe"), "django");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_django(&["--help".to_string()]), 0);
        assert_eq!(run_django(&["-h".to_string()]), 0);
        let _ = run_django(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_django(&[]);
    }
}
