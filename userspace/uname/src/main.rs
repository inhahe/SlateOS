//! OurOS `uname` Utility — Print System Information
//!
//! Reads system identity from `/proc/sys/kernel/` and `/proc/cpuinfo`, then
//! prints the requested fields separated by spaces. Falls back to hardcoded
//! defaults when proc files are unavailable (e.g., early boot, container
//! environments without procfs mounted).
//!
//! # Usage
//!
//! ```text
//! uname                  Print kernel name (same as -s)
//! uname -a               Print all information
//! uname -s               Print kernel name
//! uname -n               Print network node hostname
//! uname -r               Print kernel release
//! uname -v               Print kernel version string
//! uname -m               Print machine hardware name
//! uname -p               Print processor type
//! uname -i               Print hardware platform
//! uname -o               Print operating system name
//! uname --json           Print all fields as JSON
//! uname -sr              Combine flags: kernel name + release
//! ```

use std::env;
use std::fs;
use std::process;

// ============================================================================
// Constants and defaults
// ============================================================================

const VERSION: &str = "0.1.0";

/// Default kernel name when `/proc/sys/kernel/ostype` is unavailable.
const DEFAULT_KERNEL_NAME: &str = "OurOS";

/// Default kernel release when `/proc/sys/kernel/osrelease` is unavailable.
const DEFAULT_RELEASE: &str = "0.1.0";

/// Default machine architecture.
const DEFAULT_MACHINE: &str = "x86_64";

/// Default operating system name.
const DEFAULT_OS_NAME: &str = "OurOS";

// Proc filesystem paths.
const PROC_OSTYPE: &str = "/proc/sys/kernel/ostype";
const PROC_HOSTNAME: &str = "/proc/sys/kernel/hostname";
const PROC_OSRELEASE: &str = "/proc/sys/kernel/osrelease";
const PROC_VERSION: &str = "/proc/sys/kernel/version";
const PROC_CPUINFO: &str = "/proc/cpuinfo";

// ============================================================================
// Field identifiers
// ============================================================================

/// Which information fields to print. Order matches the `-a` output order
/// defined by POSIX (s, n, r, v, m) plus our extensions (p, i, o).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    KernelName,
    NodeName,
    KernelRelease,
    KernelVersion,
    Machine,
    Processor,
    HardwarePlatform,
    OperatingSystem,
}

/// The canonical output order for `-a` and for multi-flag combinations.
const ALL_FIELDS: [Field; 8] = [
    Field::KernelName,
    Field::NodeName,
    Field::KernelRelease,
    Field::KernelVersion,
    Field::Machine,
    Field::Processor,
    Field::HardwarePlatform,
    Field::OperatingSystem,
];

// ============================================================================
// Parsed command-line request
// ============================================================================

enum Request {
    /// Print the selected fields.
    PrintFields(Vec<Field>),
    /// Print all fields as JSON.
    Json,
    /// Show help text.
    Help,
    /// Show version.
    Version,
}

// ============================================================================
// Data source helpers
// ============================================================================

/// Read a proc file, returning its trimmed contents or `None` on any error.
fn read_proc(path: &str) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Extract the processor model name from `/proc/cpuinfo`.
///
/// Scans for the first line matching `model name\t: <value>` and returns the
/// value portion. Returns `None` if the file is missing or contains no model
/// name entry.
fn read_processor_from_cpuinfo() -> Option<String> {
    let content = fs::read_to_string(PROC_CPUINFO).ok()?;

    for line in content.lines() {
        let line = line.trim();
        // cpuinfo format: "model name\t: Intel Core i7-..."
        // After trim, look for lines starting with "model name".
        if let Some(rest) = line.strip_prefix("model name") {
            // Skip optional whitespace and the colon separator.
            let rest = rest.trim_start();
            if let Some(value) = rest.strip_prefix(':') {
                let value = value.trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }

    None
}

// ============================================================================
// System info collection
// ============================================================================

/// All system information fields, resolved from proc or defaults.
struct SystemInfo {
    kernel_name: String,
    node_name: String,
    kernel_release: String,
    kernel_version: String,
    machine: String,
    processor: String,
    hardware_platform: String,
    operating_system: String,
}

impl SystemInfo {
    /// Gather system information from proc files, falling back to defaults.
    fn gather() -> Self {
        let kernel_name = read_proc(PROC_OSTYPE)
            .unwrap_or_else(|| DEFAULT_KERNEL_NAME.to_string());

        let node_name = read_proc(PROC_HOSTNAME)
            .unwrap_or_else(|| "unknown".to_string());

        let kernel_release = read_proc(PROC_OSRELEASE)
            .unwrap_or_else(|| DEFAULT_RELEASE.to_string());

        let kernel_version = read_proc(PROC_VERSION)
            .unwrap_or_else(|| "unknown".to_string());

        let machine = DEFAULT_MACHINE.to_string();

        let processor = read_processor_from_cpuinfo()
            .unwrap_or_else(|| DEFAULT_MACHINE.to_string());

        // Hardware platform: same as machine on x86_64.
        let hardware_platform = DEFAULT_MACHINE.to_string();

        let operating_system = DEFAULT_OS_NAME.to_string();

        Self {
            kernel_name,
            node_name,
            kernel_release,
            kernel_version,
            machine,
            processor,
            hardware_platform,
            operating_system,
        }
    }

    /// Get the string value for a given field.
    fn get(&self, field: Field) -> &str {
        match field {
            Field::KernelName => &self.kernel_name,
            Field::NodeName => &self.node_name,
            Field::KernelRelease => &self.kernel_release,
            Field::KernelVersion => &self.kernel_version,
            Field::Machine => &self.machine,
            Field::Processor => &self.processor,
            Field::HardwarePlatform => &self.hardware_platform,
            Field::OperatingSystem => &self.operating_system,
        }
    }

    /// Render all fields as a JSON object string.
    ///
    /// Hand-built to avoid pulling in serde/serde_json as dependencies for a
    /// tiny utility. Values are escaped for JSON safety.
    fn to_json(&self) -> String {
        let mut out = String::with_capacity(512);
        out.push_str("{\n");
        out.push_str(&format!("  \"kernel_name\": {},\n", json_escape(&self.kernel_name)));
        out.push_str(&format!("  \"node_name\": {},\n", json_escape(&self.node_name)));
        out.push_str(&format!("  \"kernel_release\": {},\n", json_escape(&self.kernel_release)));
        out.push_str(&format!("  \"kernel_version\": {},\n", json_escape(&self.kernel_version)));
        out.push_str(&format!("  \"machine\": {},\n", json_escape(&self.machine)));
        out.push_str(&format!("  \"processor\": {},\n", json_escape(&self.processor)));
        out.push_str(&format!("  \"hardware_platform\": {},\n", json_escape(&self.hardware_platform)));
        out.push_str(&format!("  \"operating_system\": {}\n", json_escape(&self.operating_system)));
        out.push('}');
        out
    }
}

/// Escape a string for JSON output. Wraps the result in double quotes.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                // Unicode escape for other control characters.
                for unit in c.encode_utf16(&mut [0u16; 2]) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ============================================================================
// Argument parsing
// ============================================================================

/// Parse command-line arguments into a `Request`.
///
/// Supports both separate flags (`uname -s -r`) and combined short flags
/// (`uname -sr`). Long options use `--` prefix.
fn parse_args(args: &[String]) -> Request {
    if args.len() <= 1 {
        // No arguments: default to printing kernel name (same as -s).
        return Request::PrintFields(vec![Field::KernelName]);
    }

    let mut fields: Vec<Field> = Vec::new();
    let mut seen_json = false;

    for arg in &args[1..] {
        let arg_str = arg.as_str();

        // Long options.
        if arg_str.starts_with("--") {
            match arg_str {
                "--all" => {
                    return Request::PrintFields(ALL_FIELDS.to_vec());
                }
                "--kernel-name" => push_unique(&mut fields, Field::KernelName),
                "--nodename" => push_unique(&mut fields, Field::NodeName),
                "--kernel-release" => push_unique(&mut fields, Field::KernelRelease),
                "--kernel-version" => push_unique(&mut fields, Field::KernelVersion),
                "--machine" => push_unique(&mut fields, Field::Machine),
                "--processor" => push_unique(&mut fields, Field::Processor),
                "--hardware-platform" => push_unique(&mut fields, Field::HardwarePlatform),
                "--operating-system" => push_unique(&mut fields, Field::OperatingSystem),
                "--json" => {
                    seen_json = true;
                }
                "--help" => return Request::Help,
                "--version" => return Request::Version,
                _ => {
                    eprintln!("uname: unrecognized option '{arg_str}'");
                    eprintln!("Try 'uname --help' for more information.");
                    process::exit(1);
                }
            }
            continue;
        }

        // Short options: may be combined (e.g., "-sr" = -s -r).
        if arg_str.starts_with('-') && arg_str.len() > 1 {
            for ch in arg_str[1..].chars() {
                match ch {
                    'a' => {
                        return Request::PrintFields(ALL_FIELDS.to_vec());
                    }
                    's' => push_unique(&mut fields, Field::KernelName),
                    'n' => push_unique(&mut fields, Field::NodeName),
                    'r' => push_unique(&mut fields, Field::KernelRelease),
                    'v' => push_unique(&mut fields, Field::KernelVersion),
                    'm' => push_unique(&mut fields, Field::Machine),
                    'p' => push_unique(&mut fields, Field::Processor),
                    'i' => push_unique(&mut fields, Field::HardwarePlatform),
                    'o' => push_unique(&mut fields, Field::OperatingSystem),
                    'h' => return Request::Help,
                    _ => {
                        eprintln!("uname: invalid option -- '{ch}'");
                        eprintln!("Try 'uname --help' for more information.");
                        process::exit(1);
                    }
                }
            }
            continue;
        }

        // Bare argument — not expected for uname.
        eprintln!("uname: extra operand '{arg_str}'");
        eprintln!("Try 'uname --help' for more information.");
        process::exit(1);
    }

    if seen_json {
        return Request::Json;
    }

    if fields.is_empty() {
        // Only unrecognized modifiers, no field flags — default to kernel name.
        Request::PrintFields(vec![Field::KernelName])
    } else {
        Request::PrintFields(fields)
    }
}

/// Push a field into the list only if it is not already present, preserving
/// the canonical output order by always inserting at the position matching
/// `ALL_FIELDS` order.
fn push_unique(fields: &mut Vec<Field>, field: Field) {
    if fields.contains(&field) {
        return;
    }

    // Find where this field belongs in the canonical order.
    let target_rank = ALL_FIELDS.iter().position(|f| *f == field);
    match target_rank {
        Some(rank) => {
            // Find the insertion point: after all existing fields whose rank
            // is less than ours.
            let pos = fields.iter().position(|f| {
                ALL_FIELDS.iter().position(|af| af == f).unwrap_or(0) > rank
            });
            match pos {
                Some(p) => fields.insert(p, field),
                None => fields.push(field),
            }
        }
        None => fields.push(field),
    }
}

// ============================================================================
// Help text
// ============================================================================

fn print_help() {
    println!("OurOS uname v{VERSION}");
    println!();
    println!("Print system information.");
    println!();
    println!("USAGE:");
    println!("  uname [OPTION]...");
    println!();
    println!("With no OPTION, same as -s.");
    println!();
    println!("OPTIONS:");
    println!("  -a, --all                Print all information (same as -snrvmpio)");
    println!("  -s, --kernel-name        Print the kernel name");
    println!("  -n, --nodename           Print the network node hostname");
    println!("  -r, --kernel-release     Print the kernel release");
    println!("  -v, --kernel-version     Print the kernel version");
    println!("  -m, --machine            Print the machine hardware name");
    println!("  -p, --processor          Print the processor type");
    println!("  -i, --hardware-platform  Print the hardware platform");
    println!("  -o, --operating-system   Print the operating system");
    println!("      --json               Print all fields as JSON");
    println!("  -h, --help               Display this help and exit");
    println!("      --version            Display version and exit");
    println!();
    println!("DATA SOURCES:");
    println!("  {PROC_OSTYPE:<36} Kernel name");
    println!("  {PROC_HOSTNAME:<36} Node hostname");
    println!("  {PROC_OSRELEASE:<36} Kernel release");
    println!("  {PROC_VERSION:<36} Kernel version string");
    println!("  {PROC_CPUINFO:<36} Processor model name");
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();
    let request = parse_args(&args);

    let exit_code = run(request);
    process::exit(exit_code);
}

/// Execute the parsed request. Returns exit code (0 = success, 1 = error).
fn run(request: Request) -> i32 {
    match request {
        Request::Help => {
            print_help();
            0
        }

        Request::Version => {
            println!("uname (OurOS) {VERSION}");
            0
        }

        Request::PrintFields(fields) => {
            let info = SystemInfo::gather();

            let values: Vec<&str> = fields.iter().map(|f| info.get(*f)).collect();
            println!("{}", values.join(" "));
            0
        }

        Request::Json => {
            let info = SystemInfo::gather();
            println!("{}", info.to_json());
            0
        }
    }
}
