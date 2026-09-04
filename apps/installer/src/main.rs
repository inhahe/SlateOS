//! Slate OS Installer — CLI frontend.
//!
//! Usage:
//!   installer --config <path.yaml>       Run unattended installation
//!   installer --validate <path.yaml>     Validate config without installing
//!   installer --plan <path.yaml>         Show install plan without executing
//!   installer --generate-config          Output a sample YAML config to stdout

use std::env;
use std::fs;
use std::process;

use installer::{InstallConfig, InstallPlan, generate_sample_config};

/// CLI operating mode.
#[derive(Debug)]
enum Mode {
    /// Run a full unattended installation.
    Install(String),
    /// Validate a config file and report errors.
    Validate(String),
    /// Show the install plan without executing.
    Plan(String),
    /// Print a sample YAML config to stdout.
    GenerateConfig,
    /// Show usage help.
    Help,
}

fn main() {
    let mode = parse_args();

    match mode {
        Mode::Help => {
            print_usage();
        }
        Mode::GenerateConfig => {
            print!("{}", generate_sample_config());
        }
        Mode::Validate(path) => {
            cmd_validate(&path);
        }
        Mode::Plan(path) => {
            cmd_plan(&path);
        }
        Mode::Install(path) => {
            cmd_install(&path);
        }
    }
}

/// Parse the real command line, reporting a usage error and exiting on one.
fn parse_args() -> Mode {
    let args: Vec<String> = env::args().collect();
    match mode_from_args(&args) {
        Ok(mode) => mode,
        Err(msg) => {
            eprintln!("error: {msg}");
            process::exit(1);
        }
    }
}

/// Decide the mode from an argument vector, `argv[0]` included.
///
/// Split out from [`parse_args`] so it can be tested: the version that reads
/// `env::args` and calls `process::exit` cannot be, and an argument parser that
/// no test has ever run is exactly the kind of code that greets a user with the
/// wrong mode.
fn mode_from_args(args: &[String]) -> Result<Mode, String> {
    // `get` rather than a length test plus an index: one expression that cannot
    // disagree with itself. No argument at all is not an error — it is help.
    let Some(first) = args.get(1) else {
        return Ok(Mode::Help);
    };

    // Modes that take a path consume the next argument.
    let path = |what: &str| -> Result<String, String> {
        args.get(2)
            .cloned()
            .ok_or_else(|| format!("{what} requires a file path argument"))
    };

    match first.as_str() {
        "--help" | "-h" => Ok(Mode::Help),
        "--generate-config" => Ok(Mode::GenerateConfig),
        "--config" => Ok(Mode::Install(path("--config")?)),
        "--validate" => Ok(Mode::Validate(path("--validate")?)),
        "--plan" => Ok(Mode::Plan(path("--plan")?)),
        other => Err(format!("unknown argument '{other}'")),
    }
}

/// Print usage information.
fn print_usage() {
    println!("Slate OS Installer v0.1.0");
    println!();
    println!("Usage:");
    println!("  installer --config <path.yaml>       Run unattended installation");
    println!("  installer --validate <path.yaml>     Validate config without installing");
    println!("  installer --plan <path.yaml>         Show install plan without executing");
    println!("  installer --generate-config          Output a sample YAML config to stdout");
    println!("  installer --help                     Show this help message");
}

/// Read a config file from disk and parse it.
fn load_config(path: &str) -> InstallConfig {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: cannot read '{path}': {e}");
            process::exit(1);
        }
    };

    match InstallConfig::from_yaml(&content) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("error: failed to parse config: {e}");
            process::exit(1);
        }
    }
}

/// Validate a config file and print results.
fn cmd_validate(path: &str) {
    let config = load_config(path);

    match config.validate() {
        Ok(()) => {
            println!("Configuration is valid.");
            println!("  Hostname:  {}", config.hostname);
            println!("  Locale:    {}", config.locale);
            println!("  Timezone:  {}", config.timezone);
            println!("  Disk:      {}", config.disk.target);
            println!("  Partitions: {}", config.disk.partitions.len());
            println!("  Users:     {}", config.users.len());
            println!("  Packages:  {}", config.packages.len());
        }
        Err(errors) => {
            eprintln!("Configuration has {} error(s):", errors.len());
            for (i, err) in errors.iter().enumerate() {
                let num = i.wrapping_add(1);
                eprintln!("  {num}. {err}");
            }
            process::exit(1);
        }
    }
}

/// Show the install plan without executing.
fn cmd_plan(path: &str) {
    let config = load_config(path);

    // Validate first.
    if let Err(errors) = config.validate() {
        eprintln!("Configuration has {} error(s):", errors.len());
        for (i, err) in errors.iter().enumerate() {
            let num = i.wrapping_add(1);
            eprintln!("  {num}. {err}");
        }
        process::exit(1);
    }

    let plan = InstallPlan::from_config(&config);
    print!("{}", plan.describe());
}

/// Run the installation (in the future, this will execute steps; for now it
/// validates, plans, and prints what it would do).
fn cmd_install(path: &str) {
    let config = load_config(path);

    // Validate.
    if let Err(errors) = config.validate() {
        eprintln!(
            "Installation aborted: configuration has {} error(s):",
            errors.len()
        );
        for (i, err) in errors.iter().enumerate() {
            let num = i.wrapping_add(1);
            eprintln!("  {num}. {err}");
        }
        process::exit(1);
    }

    let plan = InstallPlan::from_config(&config);

    println!("Slate OS Installer");
    println!("===============");
    println!();
    println!("Target disk: {}", config.disk.target);
    println!("Hostname:    {}", config.hostname);
    println!("Users:       {}", config.users.len());
    println!("Packages:    {}", config.packages.len());
    println!();
    print!("{}", plan.describe());
    println!();

    // Execute steps — currently a dry-run that logs what would happen.
    let mut progress = installer::InstallProgress::new(&plan);
    for step in &plan.steps {
        let desc = match step {
            installer::InstallStep::WipeDisk { target } => {
                format!("Wiping disk {target}")
            }
            installer::InstallStep::CreatePartition { label, size_desc } => {
                format!("Creating partition '{label}' ({size_desc})")
            }
            installer::InstallStep::FormatPartition { label, fs } => {
                format!("Formatting '{label}' as {fs}")
            }
            installer::InstallStep::MountPartition { label, mount_point } => {
                format!("Mounting '{label}' at {mount_point}")
            }
            installer::InstallStep::CopyBaseSystem => "Copying base system files".to_string(),
            installer::InstallStep::InstallPackages { packages } => {
                format!("Installing {} package(s)", packages.len())
            }
            installer::InstallStep::CreateUser { username } => {
                format!("Creating user '{username}'")
            }
            installer::InstallStep::ConfigureNetwork { mode } => {
                format!("Configuring network ({mode})")
            }
            installer::InstallStep::SetHostname { hostname } => {
                format!("Setting hostname to '{hostname}'")
            }
            installer::InstallStep::SetTimezone { timezone } => {
                format!("Setting timezone to '{timezone}'")
            }
            installer::InstallStep::SetLocale { locale } => {
                format!("Setting locale to '{locale}'")
            }
            installer::InstallStep::EnableServices { services } => {
                format!("Enabling {} service(s)", services.len())
            }
            installer::InstallStep::RunPostInstall { commands } => {
                format!("Running {} post-install command(s)", commands.len())
            }
            installer::InstallStep::InstallBootloader { target } => {
                format!("Installing bootloader to {target}")
            }
            installer::InstallStep::Unmount => "Unmounting all partitions".to_string(),
            installer::InstallStep::Reboot => "Rebooting system".to_string(),
        };
        progress.advance(&desc);
        println!("[{:>3}%] {desc}", progress.percent);
    }

    println!();
    println!("Installation complete.");
}

#[cfg(test)]
mod tests {
    // A test that unwraps a failure should fail loudly at the line that did
    // it — that is the diagnosis. The defensive lints exist to keep panics out
    // of code that runs on a user's data, which this is not.
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::{Mode, mode_from_args};

    fn argv(rest: &[&str]) -> Vec<String> {
        std::iter::once("installer".to_string())
            .chain(rest.iter().map(|s| (*s).to_string()))
            .collect()
    }

    #[test]
    fn no_arguments_is_help_and_not_an_error() {
        // Running the installer with no arguments is how a user asks what it
        // does, so it must not exit non-zero.
        assert!(matches!(mode_from_args(&argv(&[])), Ok(Mode::Help)));
    }

    #[test]
    fn both_spellings_of_help_are_accepted() {
        assert!(matches!(mode_from_args(&argv(&["--help"])), Ok(Mode::Help)));
        assert!(matches!(mode_from_args(&argv(&["-h"])), Ok(Mode::Help)));
    }

    #[test]
    fn each_path_mode_keeps_its_own_path() {
        // The three path modes differ only in the variant they build, which is
        // exactly the kind of thing a copy-paste edit gets wrong silently.
        for flag in ["--config", "--validate", "--plan"] {
            let mode = mode_from_args(&argv(&[flag, "cfg.yaml"])).unwrap();
            let got = match (flag, &mode) {
                ("--config", Mode::Install(p))
                | ("--validate", Mode::Validate(p))
                | ("--plan", Mode::Plan(p)) => Some(p.as_str()),
                _ => None,
            };
            assert_eq!(got, Some("cfg.yaml"), "{flag} built {mode:?}");
        }
    }

    #[test]
    fn a_path_mode_without_a_path_names_the_flag_that_wanted_one() {
        for flag in ["--config", "--validate", "--plan"] {
            let err = mode_from_args(&argv(&[flag])).unwrap_err();
            assert!(
                err.contains(flag),
                "the error for a missing path should name {flag}, said: {err}"
            );
        }
    }

    #[test]
    fn generate_config_takes_no_path() {
        // It writes to stdout, so a stray second argument is not consumed and
        // must not turn it into an install.
        assert!(matches!(
            mode_from_args(&argv(&["--generate-config", "ignored"])),
            Ok(Mode::GenerateConfig)
        ));
    }

    #[test]
    fn an_unknown_flag_is_rejected_and_quoted_back() {
        // Quoting matters: a mistyped flag with a trailing space reads as
        // correct in an unquoted message.
        let err = mode_from_args(&argv(&["--isntall"])).unwrap_err();
        assert!(err.contains("'--isntall'"), "said: {err}");
    }

    #[test]
    fn a_bare_path_is_rejected_rather_than_guessed_at() {
        // `installer cfg.yaml` could plausibly mean install, but guessing at an
        // unattended install of a whole disk is not a guess worth making.
        assert!(mode_from_args(&argv(&["cfg.yaml"])).is_err());
    }
}
