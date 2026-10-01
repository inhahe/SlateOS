//! Slate OS Installer — CLI frontend.
//!
//! Usage:
//!   installer --config <path.yaml>       Run unattended installation
//!   installer --validate <path.yaml>     Validate config without installing
//!   installer --plan <path.yaml>         Show install plan without executing
//!   installer --generate-config          Output a sample YAML config to stdout
//!   installer --grub-detect              Report the GRUB found, and Slate OS's entry in it
//!   installer --grub-add --uuid <esp>    Add Slate OS to GRUB's menu
//!   installer --grub-update --uuid <esp> Rewrite that entry
//!   installer --grub-remove              Remove it
//!
//! The GRUB commands are `grubcmd`'s; their options are in [`print_usage`].
//!
//! Arguments are read as `OsString`s: a path may be any bytes, and
//! `env::args` panics on one that is not text.

mod grubcmd;

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use installer::{InstallConfig, InstallPlan, generate_sample_config};
use pathtext::ShowPath;

/// CLI operating mode.
#[derive(Debug)]
enum Mode {
    /// Run a full unattended installation.
    Install(PathBuf),
    /// Validate a config file and report errors.
    Validate(PathBuf),
    /// Show the install plan without executing.
    Plan(PathBuf),
    /// Print a sample YAML config to stdout.
    GenerateConfig,
    /// A GRUB command.
    Grub(grubcmd::Command),
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
        Mode::Grub(cmd) => match grubcmd::run(&cmd) {
            Ok(report) => print!("{report}"),
            Err(msg) => {
                eprintln!("error: {msg}");
                process::exit(1);
            }
        },
    }
}

/// Parse the real command line, reporting a usage error and exiting on one.
fn parse_args() -> Mode {
    let args: Vec<OsString> = env::args_os().collect();
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
fn mode_from_args(args: &[OsString]) -> Result<Mode, String> {
    // `get` rather than a length test plus an index: one expression that cannot
    // disagree with itself. No argument at all is not an error — it is help.
    let Some(first) = args.get(1) else {
        return Ok(Mode::Help);
    };

    // Modes that take a path consume the next argument.
    let path = |what: &str| -> Result<PathBuf, String> {
        args.get(2)
            .map(PathBuf::from)
            .ok_or_else(|| format!("{what} requires a file path argument"))
    };
    // The GRUB commands read the rest as options.
    let grub = |verb| grubcmd::parse(verb, args.get(2..).unwrap_or_default()).map(Mode::Grub);

    match first.to_str() {
        Some("--help" | "-h") => Ok(Mode::Help),
        Some("--generate-config") => Ok(Mode::GenerateConfig),
        Some("--config") => Ok(Mode::Install(path("--config")?)),
        Some("--validate") => Ok(Mode::Validate(path("--validate")?)),
        Some("--plan") => Ok(Mode::Plan(path("--plan")?)),
        Some("--grub-detect") => grub(grubcmd::Verb::Detect),
        Some("--grub-add") => grub(grubcmd::Verb::Add),
        Some("--grub-update") => grub(grubcmd::Verb::Update),
        Some("--grub-remove") => grub(grubcmd::Verb::Remove),
        _ => Err(format!("unknown argument '{}'", first.as_os_str().shown())),
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
    println!();
    println!("GRUB, to start Slate OS from another system's boot menu:");
    println!(
        "  installer --grub-detect              Report the GRUB found, and Slate OS's entry in it"
    );
    println!("  installer --grub-add --uuid <esp>    Add Slate OS to GRUB's menu");
    println!("  installer --grub-update --uuid <esp> Rewrite that entry");
    println!("  installer --grub-remove              Remove it");
    println!();
    println!("  --uuid <esp>    Filesystem UUID of the EFI system partition Limine is on");
    println!(
        "  --path <path>   Limine on that partition (default {})",
        installer::LIMINE_EFI_PATH
    );
    println!(
        "  --title <text>  The menu entry's title (default \"{}\")",
        installer::GRUB_TITLE
    );
    println!("  --root <dir>    The system whose GRUB it is, mounted there (default /)");
    println!("  --no-update     Leave GRUB's menu to be rebuilt later");
    println!();
    println!("GRUB chainloads Limine, which starts Slate OS: the kernel cannot be loaded");
    println!("by GRUB itself. For the running system the menu is rebuilt with its own");
    println!("update-grub or grub-mkconfig; another system's is left for it to rebuild.");
}

/// Read a config file from disk and parse it.
fn load_config(path: &Path) -> InstallConfig {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: cannot read '{}': {e}", path.shown());
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
fn cmd_validate(path: &Path) {
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
fn cmd_plan(path: &Path) {
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
fn cmd_install(path: &Path) {
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
            installer::InstallStep::AddGrubEntry { title } => {
                format!("Adding '{title}' to GRUB's menu")
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

    use super::{Mode, grubcmd, mode_from_args};
    use std::ffi::OsString;

    fn argv(rest: &[&str]) -> Vec<OsString> {
        std::iter::once("installer")
            .chain(rest.iter().copied())
            .map(OsString::from)
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
                | ("--plan", Mode::Plan(p)) => p.to_str(),
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
    fn each_grub_flag_asks_for_its_own_command() {
        use grubcmd::Action;
        for (flag, rest) in [
            ("--grub-detect", &[][..]),
            ("--grub-add", &["--uuid", "1A2B-3C4D"][..]),
            ("--grub-update", &["--uuid", "1A2B-3C4D"][..]),
            ("--grub-remove", &[][..]),
        ] {
            let args: Vec<&str> = std::iter::once(flag).chain(rest.iter().copied()).collect();
            let Ok(Mode::Grub(cmd)) = mode_from_args(&argv(&args)) else {
                panic!("{flag} did not build a GRUB command");
            };
            let got = match cmd.action {
                Action::Detect => "--grub-detect",
                Action::Add(_) => "--grub-add",
                Action::Update(_) => "--grub-update",
                Action::Remove => "--grub-remove",
            };
            assert_eq!(got, flag);
        }
    }

    #[test]
    fn a_grub_commands_options_reach_it() {
        let Ok(Mode::Grub(cmd)) = mode_from_args(&argv(&["--grub-remove", "--root", "/mnt"]))
        else {
            panic!("--grub-remove did not build a GRUB command");
        };
        assert_eq!(cmd.root, std::path::PathBuf::from("/mnt"));
        let err = mode_from_args(&argv(&["--grub-add"])).unwrap_err();
        assert!(err.contains("--uuid"), "said: {err}");
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_is_not_text_is_kept_as_it_is() {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::OsStr::from_bytes(b"caf\xe9.yaml").to_os_string();
        let args = vec![
            OsString::from("installer"),
            OsString::from("--plan"),
            path.clone(),
        ];
        let Ok(Mode::Plan(p)) = mode_from_args(&args) else {
            panic!("--plan did not keep its path");
        };
        assert_eq!(p.as_os_str(), path);
    }

    #[cfg(windows)]
    #[test]
    fn a_path_that_is_not_text_is_kept_as_it_is() {
        use std::os::windows::ffi::OsStringExt;
        let mut wide: Vec<u16> = "caf".encode_utf16().collect();
        wide.push(0xD800);
        let path = OsString::from_wide(&wide);
        let args = vec![
            OsString::from("installer"),
            OsString::from("--plan"),
            path.clone(),
        ];
        let Ok(Mode::Plan(p)) = mode_from_args(&args) else {
            panic!("--plan did not keep its path");
        };
        assert_eq!(p.as_os_str(), path);
    }

    #[test]
    fn a_bare_path_is_rejected_rather_than_guessed_at() {
        // `installer cfg.yaml` could plausibly mean install, but guessing at an
        // unattended install of a whole disk is not a guess worth making.
        assert!(mode_from_args(&argv(&["cfg.yaml"])).is_err());
    }
}
