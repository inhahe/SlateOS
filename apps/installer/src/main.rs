//! SlateOS Installer — CLI frontend.
//!
//! Usage:
//!   installer --config <path.yaml>       Run unattended installation
//!   installer --validate <path.yaml>     Validate config without installing
//!   installer --plan <path.yaml>         Show install plan without executing
//!   installer --generate-config          Output a sample YAML config to stdout

#![allow(dead_code)]

use std::env;
use std::fs;
use std::process;

use installer::{InstallConfig, InstallPlan, generate_sample_config};

/// CLI operating mode.
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

/// Parse command-line arguments into a `Mode`.
fn parse_args() -> Mode {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        return Mode::Help;
    }

    // The first real argument determines the mode.  Arguments that take a
    // path consume args[2].
    match args[1].as_str() {
        "--help" | "-h" => Mode::Help,
        "--generate-config" => Mode::GenerateConfig,
        "--config" => {
            let path = args.get(2).unwrap_or_else(|| {
                eprintln!("error: --config requires a file path argument");
                process::exit(1);
            });
            Mode::Install(path.clone())
        }
        "--validate" => {
            let path = args.get(2).unwrap_or_else(|| {
                eprintln!("error: --validate requires a file path argument");
                process::exit(1);
            });
            Mode::Validate(path.clone())
        }
        "--plan" => {
            let path = args.get(2).unwrap_or_else(|| {
                eprintln!("error: --plan requires a file path argument");
                process::exit(1);
            });
            Mode::Plan(path.clone())
        }
        other => {
            eprintln!("error: unknown argument '{other}'");
            process::exit(1);
        }
    }
}

/// Print usage information.
fn print_usage() {
    println!("SlateOS Installer v0.1.0");
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

    println!("SlateOS Installer");
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
