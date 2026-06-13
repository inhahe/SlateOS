// SlateOS lp — printing utilities
//
// Multi-personality binary:
//   lp      — submit print job
//   lpstat  — show printer and job status
//   lprm    — remove print jobs
//   cancel  — cancel print jobs (alias for lprm)
//   lpr     — submit print job (BSD style)
//   lpq     — show queue (BSD style)
//
// Usage:
//   lp [OPTIONS] [file...]
//   lpstat [OPTIONS]
//   lprm [OPTIONS] [job-id...]
//   cancel [job-id...] [-a] [-x]
//   lpr [OPTIONS] [file...]
//   lpq [OPTIONS]

#![cfg_attr(not(test), no_main)]

#[cfg(not(test))]
use std::env;
use std::io::{self, Write};
#[cfg(not(test))]
use std::io::Read;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Personality detection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Personality {
    Lp,
    Lpstat,
    Lprm,
    Cancel,
    Lpr,
    Lpq,
}

fn detect_personality(argv0: &str) -> Personality {
    let base = argv0.rsplit('/').next().unwrap_or(argv0);
    let base = base.rsplit('\\').next().unwrap_or(base);
    let lower = base.to_ascii_lowercase();
    let lower = lower.strip_suffix(".exe").unwrap_or(&lower);
    match lower {
        "lpstat" => Personality::Lpstat,
        "lprm" => Personality::Lprm,
        "cancel" => Personality::Cancel,
        "lpr" => Personality::Lpr,
        "lpq" => Personality::Lpq,
        _ => Personality::Lp,
    }
}

// ---------------------------------------------------------------------------
// Print job and queue
// ---------------------------------------------------------------------------

// Several fields are parsed from the CUPS queue state file and only
// consumed by the still-pending verbose `lpstat -l` listing path.
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct PrintJob {
    job_id: u32,
    username: String,
    title: String,
    printer: String,
    size_bytes: u64,
    copies: u32,
    status: JobStatus,
    submitted: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JobStatus {
    Pending,
    Processing,
    Held,
    Completed,
    Cancelled,
}

impl JobStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Processing => "processing",
            Self::Held => "held",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }
}

// Same rationale as PrintJob — enabled/jobs_queued feed the verbose
// `lpstat -l` listing path.
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct PrinterInfo {
    name: String,
    description: String,
    location: String,
    uri: String,
    is_default: bool,
    accepting: bool,
    enabled: bool,
    state: PrinterState,
    jobs_queued: u32,
}

// Processing/Stopped are produced by the CUPS-style state machine that
// hasn't been wired up yet; the sample data only emits Idle.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrinterState {
    Idle,
    Processing,
    Stopped,
}

impl PrinterState {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Processing => "processing",
            Self::Stopped => "stopped",
        }
    }
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Config {
    personality: Personality,
    files: Vec<PathBuf>,
    printer: Option<String>,
    copies: u32,
    title: Option<String>,
    priority: u32,
    job_ids: Vec<u32>,
    show_all: bool,
    show_devices: bool,
    show_printers: bool,
    show_jobs: bool,
    show_long: bool,
    show_accepting: bool,
    cancel_all: bool,
    cancel_purge: bool,
    show_help: bool,
    show_version: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            personality: Personality::Lp,
            files: Vec::new(),
            printer: None,
            copies: 1,
            title: None,
            priority: 50,
            job_ids: Vec::new(),
            show_all: false,
            show_devices: false,
            show_printers: false,
            show_jobs: false,
            show_long: false,
            show_accepting: false,
            cancel_all: false,
            cancel_purge: false,
            show_help: false,
            show_version: false,
        }
    }
}

fn parse_args(args: &[String]) -> Result<Config, String> {
    let personality = args
        .first()
        .map(|a| detect_personality(a))
        .unwrap_or(Personality::Lp);

    let mut cfg = Config {
        personality,
        ..Default::default()
    };

    let mut i = 1;

    while i < args.len() {
        let arg = &args[i];
        match personality {
            Personality::Lp | Personality::Lpr => match arg.as_str() {
                "-d" | "-P" => {
                    i += 1;
                    cfg.printer = Some(
                        args.get(i)
                            .ok_or("-d requires a printer name")?
                            .clone(),
                    );
                }
                "-n" | "--copies" => {
                    i += 1;
                    cfg.copies = args
                        .get(i)
                        .ok_or("-n requires a number")?
                        .parse::<u32>()
                        .map_err(|e| format!("-n: {e}"))?;
                }
                "-t" | "-T" | "-J" => {
                    i += 1;
                    cfg.title = Some(
                        args.get(i)
                            .ok_or("-t requires a title")?
                            .clone(),
                    );
                }
                "-q" => {
                    i += 1;
                    cfg.priority = args
                        .get(i)
                        .ok_or("-q requires a number")?
                        .parse::<u32>()
                        .map_err(|e| format!("-q: {e}"))?;
                }
                "-h" | "--help" => cfg.show_help = true,
                "-V" | "--version" => cfg.show_version = true,
                "-" => cfg.files.push(PathBuf::from("-")), // stdin
                other if other.starts_with('-') => {
                    return Err(format!("lp: unknown option: {other}"));
                }
                _ => cfg.files.push(PathBuf::from(arg)),
            },
            Personality::Lpstat | Personality::Lpq => match arg.as_str() {
                "-a" => cfg.show_accepting = true,
                "-d" => cfg.show_printers = true,
                "-p" => cfg.show_printers = true,
                "-o" => cfg.show_jobs = true,
                "-v" => cfg.show_devices = true,
                "-l" => cfg.show_long = true,
                "-t" => cfg.show_all = true,
                "-h" | "--help" => cfg.show_help = true,
                "-V" | "--version" => cfg.show_version = true,
                other if other.starts_with('-') => {
                    return Err(format!("lpstat: unknown option: {other}"));
                }
                _ => {} // printer names, ignored for now
            },
            Personality::Lprm | Personality::Cancel => match arg.as_str() {
                "-a" => cfg.cancel_all = true,
                "-x" => cfg.cancel_purge = true,
                "-P" => {
                    i += 1;
                    cfg.printer = Some(
                        args.get(i)
                            .ok_or("-P requires a printer name")?
                            .clone(),
                    );
                }
                "-h" | "--help" => cfg.show_help = true,
                "-V" | "--version" => cfg.show_version = true,
                other if other.starts_with('-') => {
                    return Err(format!("lprm: unknown option: {other}"));
                }
                _ => {
                    // Try to parse as job ID
                    if let Ok(id) = arg.parse::<u32>() {
                        cfg.job_ids.push(id);
                    }
                }
            },
        }
        i += 1;
    }

    // If no files specified for lp/lpr, read from stdin
    if matches!(personality, Personality::Lp | Personality::Lpr) && cfg.files.is_empty() {
        cfg.files.push(PathBuf::from("-"));
    }

    Ok(cfg)
}

// ---------------------------------------------------------------------------
// Print spool directory
// ---------------------------------------------------------------------------

const SPOOL_DIR: &str = "/var/spool/lpd";
const PRINTERS_FILE: &str = "/etc/printcap";

#[cfg(not(test))]
fn get_next_job_id() -> u32 {
    let counter_file = format!("{SPOOL_DIR}/.next_id");
    let current = std::fs::read_to_string(&counter_file)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(1);
    let next = current.saturating_add(1);
    let _ = std::fs::create_dir_all(SPOOL_DIR);
    let _ = std::fs::write(&counter_file, format!("{next}\n"));
    current
}

#[cfg(not(test))]
fn get_default_printer() -> String {
    // Check PRINTER env var, then LPDEST, then first in printcap
    if let Ok(p) = env::var("PRINTER") {
        return p;
    }
    if let Ok(p) = env::var("LPDEST") {
        return p;
    }
    "default".to_string()
}

fn read_printers() -> Vec<PrinterInfo> {
    let mut printers = Vec::new();

    if let Ok(content) = std::fs::read_to_string(PRINTERS_FILE) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Simple format: name|description:options
            let parts: Vec<&str> = line.split('|').collect();
            let name = parts[0].trim_end_matches(':').to_string();
            let desc = parts.get(1).unwrap_or(&"").trim_end_matches(':').to_string();

            printers.push(PrinterInfo {
                name: name.clone(),
                description: desc,
                location: String::new(),
                uri: "file:///dev/null".to_string(),
                is_default: printers.is_empty(),
                accepting: true,
                enabled: true,
                state: PrinterState::Idle,
                jobs_queued: 0,
            });
        }
    }

    // Always have at least a default printer
    if printers.is_empty() {
        printers.push(PrinterInfo {
            name: "default".to_string(),
            description: "Default Printer".to_string(),
            location: String::new(),
            uri: "file:///dev/null".to_string(),
            is_default: true,
            accepting: true,
            enabled: true,
            state: PrinterState::Idle,
            jobs_queued: 0,
        });
    }

    printers
}

fn read_jobs() -> Vec<PrintJob> {
    let mut jobs = Vec::new();

    let spool_path = Path::new(SPOOL_DIR);
    if let Ok(entries) = std::fs::read_dir(spool_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("job")
                && let Ok(content) = std::fs::read_to_string(&path)
                    && let Some(job) = parse_job_file(&content) {
                        jobs.push(job);
                    }
        }
    }

    jobs
}

fn parse_job_file(content: &str) -> Option<PrintJob> {
    let mut job_id = 0u32;
    let mut username = String::new();
    let mut title = String::new();
    let mut printer = String::new();
    let mut size_bytes = 0u64;
    let mut copies = 1u32;
    let mut status = JobStatus::Pending;

    for line in content.lines() {
        let (key, val) = line.split_once('=')?;
        match key.trim() {
            "id" => job_id = val.trim().parse().ok()?,
            "user" => username = val.trim().to_string(),
            "title" => title = val.trim().to_string(),
            "printer" => printer = val.trim().to_string(),
            "size" => size_bytes = val.trim().parse().unwrap_or(0),
            "copies" => copies = val.trim().parse().unwrap_or(1),
            "status" => {
                status = match val.trim() {
                    "processing" => JobStatus::Processing,
                    "held" => JobStatus::Held,
                    "completed" => JobStatus::Completed,
                    "cancelled" => JobStatus::Cancelled,
                    _ => JobStatus::Pending,
                }
            }
            _ => {}
        }
    }

    if job_id == 0 {
        return None;
    }

    Some(PrintJob {
        job_id,
        username,
        title,
        printer,
        size_bytes,
        copies,
        status,
        submitted: String::new(),
    })
}

// ---------------------------------------------------------------------------
// Subcommand implementations
// ---------------------------------------------------------------------------

#[cfg(not(test))]
fn run_lp(cfg: &Config, writer: &mut dyn Write) -> io::Result<i32> {
    let username = env::var("USER").unwrap_or_else(|_| "root".to_string());
    let printer = cfg
        .printer
        .clone()
        .unwrap_or_else(get_default_printer);

    for file_path in &cfg.files {
        let (title, size) = if file_path.as_os_str() == "-" {
            // Read stdin
            let mut data = Vec::new();
            io::stdin().read_to_end(&mut data)?;
            let size = data.len() as u64;
            ("(stdin)".to_string(), size)
        } else {
            let meta = std::fs::metadata(file_path)?;
            let title = cfg
                .title
                .clone()
                .unwrap_or_else(|| file_path.display().to_string());
            (title, meta.len())
        };

        let job_id = get_next_job_id();

        // Write job file
        let job_content = format!(
            "id={job_id}\nuser={username}\ntitle={title}\nprinter={printer}\nsize={size}\ncopies={}\nstatus=pending\n",
            cfg.copies
        );

        let _ = std::fs::create_dir_all(SPOOL_DIR);
        let _ = std::fs::write(
            format!("{SPOOL_DIR}/{job_id}.job"),
            job_content,
        );

        writeln!(
            writer,
            "request id is {printer}-{job_id} ({size} bytes)"
        )?;
    }

    Ok(0)
}

fn run_lpstat(cfg: &Config, writer: &mut dyn Write) -> io::Result<i32> {
    let printers = read_printers();
    let jobs = read_jobs();

    if cfg.show_all {
        // Show everything
        writeln!(writer, "scheduler is running")?;
        writeln!(writer)?;

        // Default destination
        if let Some(p) = printers.iter().find(|p| p.is_default) {
            writeln!(writer, "system default destination: {}", p.name)?;
        }

        // Accepting
        for p in &printers {
            writeln!(
                writer,
                "{} accepting requests since startup",
                p.name
            )?;
        }

        // Printers
        writeln!(writer)?;
        for p in &printers {
            writeln!(
                writer,
                "printer {} is {}. enabled since startup",
                p.name,
                p.state.as_str()
            )?;
        }

        // Jobs
        if jobs.is_empty() {
            writeln!(writer, "no entries")?;
        } else {
            for job in &jobs {
                writeln!(
                    writer,
                    "{}-{:<5} {:<10} {:<6} {}",
                    job.printer,
                    job.job_id,
                    job.username,
                    format_size(job.size_bytes),
                    job.status.as_str()
                )?;
            }
        }
        return Ok(0);
    }

    if cfg.show_printers {
        for p in &printers {
            if cfg.show_long {
                writeln!(writer, "printer {} is {}. enabled since startup", p.name, p.state.as_str())?;
                if !p.description.is_empty() {
                    writeln!(writer, "\tDescription: {}", p.description)?;
                }
                if !p.location.is_empty() {
                    writeln!(writer, "\tLocation: {}", p.location)?;
                }
                writeln!(writer, "\tConnection: {}", p.uri)?;
            } else {
                let default = if p.is_default { " (default)" } else { "" };
                writeln!(
                    writer,
                    "printer {} is {}. enabled since startup{}",
                    p.name,
                    p.state.as_str(),
                    default,
                )?;
            }
        }
        return Ok(0);
    }

    if cfg.show_accepting {
        for p in &printers {
            let status = if p.accepting {
                "accepting requests"
            } else {
                "not accepting requests"
            };
            writeln!(writer, "{} {} since startup", p.name, status)?;
        }
        return Ok(0);
    }

    if cfg.show_devices {
        for p in &printers {
            writeln!(writer, "device for {}: {}", p.name, p.uri)?;
        }
        return Ok(0);
    }

    if cfg.show_jobs {
        if jobs.is_empty() {
            writeln!(writer, "no entries")?;
        } else {
            for job in &jobs {
                writeln!(
                    writer,
                    "{}-{:<5} {:<10} {:<6} {}",
                    job.printer,
                    job.job_id,
                    job.username,
                    format_size(job.size_bytes),
                    job.status.as_str()
                )?;
            }
        }
        return Ok(0);
    }

    // Default: show default printer
    if let Some(p) = printers.iter().find(|p| p.is_default) {
        writeln!(writer, "system default destination: {}", p.name)?;
    } else {
        writeln!(writer, "no system default destination")?;
    }

    Ok(0)
}

#[cfg(not(test))]
fn run_lprm(cfg: &Config, writer: &mut dyn Write) -> io::Result<i32> {
    if cfg.cancel_all {
        // Cancel all jobs
        let jobs = read_jobs();
        for job in &jobs {
            let path = format!("{SPOOL_DIR}/{}.job", job.job_id);
            if std::fs::remove_file(&path).is_ok() {
                writeln!(writer, "cancelled job {}", job.job_id)?;
            }
        }
        return Ok(0);
    }

    if cfg.job_ids.is_empty() {
        // Cancel current user's most recent job
        let username = env::var("USER").unwrap_or_else(|_| "root".to_string());
        let jobs = read_jobs();
        if let Some(job) = jobs.iter().rev().find(|j| j.username == username) {
            let path = format!("{SPOOL_DIR}/{}.job", job.job_id);
            if std::fs::remove_file(&path).is_ok() {
                writeln!(writer, "cancelled job {}", job.job_id)?;
            }
        } else {
            writeln!(writer, "no jobs to cancel")?;
        }
        return Ok(0);
    }

    // Cancel specific jobs
    for &job_id in &cfg.job_ids {
        let path = format!("{SPOOL_DIR}/{job_id}.job");
        if std::fs::remove_file(&path).is_ok() {
            writeln!(writer, "cancelled job {job_id}")?;
        } else {
            writeln!(writer, "lprm: job {job_id} not found")?;
        }
    }

    Ok(0)
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes}")
    } else if bytes < 1024 * 1024 {
        format!("{}K", bytes / 1024)
    } else {
        format!("{}M", bytes / (1024 * 1024))
    }
}

// ---------------------------------------------------------------------------
// Help / version
// ---------------------------------------------------------------------------

#[cfg(not(test))]
fn print_help(personality: Personality) {
    match personality {
        Personality::Lp | Personality::Lpr => {
            println!("Usage: lp [OPTIONS] [file...]");
            println!("       lpr [OPTIONS] [file...]");
            println!();
            println!("Submit a print job.");
            println!();
            println!("Options:");
            println!("  -d, -P <printer>   Destination printer");
            println!("  -n <copies>        Number of copies");
            println!("  -t, -T <title>     Job title");
            println!("  -q <priority>      Job priority (1-100)");
            println!("  -h, --help         Show this help");
            println!("  -V, --version      Show version");
        }
        Personality::Lpstat | Personality::Lpq => {
            println!("Usage: lpstat [OPTIONS]");
            println!("       lpq [OPTIONS]");
            println!();
            println!("Show printer and job status.");
            println!();
            println!("Options:");
            println!("  -a                 Show accepting status");
            println!("  -d                 Show default destination");
            println!("  -p                 Show printer status");
            println!("  -o                 Show job queue");
            println!("  -v                 Show device URIs");
            println!("  -l                 Long output");
            println!("  -t                 Show all status info");
            println!("  -h, --help         Show this help");
            println!("  -V, --version      Show version");
        }
        Personality::Lprm | Personality::Cancel => {
            println!("Usage: lprm [OPTIONS] [job-id...]");
            println!("       cancel [job-id...] [-a]");
            println!();
            println!("Cancel print jobs.");
            println!();
            println!("Options:");
            println!("  -a                 Cancel all jobs");
            println!("  -P <printer>       Cancel jobs on specific printer");
            println!("  -x                 Purge cancelled jobs");
            println!("  -h, --help         Show this help");
            println!("  -V, --version      Show version");
        }
    }
}

#[cfg(not(test))]
fn print_version(personality: Personality) {
    let name = match personality {
        Personality::Lp => "lp",
        Personality::Lpstat => "lpstat",
        Personality::Lprm => "lprm",
        Personality::Cancel => "cancel",
        Personality::Lpr => "lpr",
        Personality::Lpq => "lpq",
    };
    println!("{name} (SlateOS) 0.1.0");
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let args: Vec<String> = env::args().collect();

    let cfg = match parse_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };

    if cfg.show_help {
        print_help(cfg.personality);
        return 0;
    }

    if cfg.show_version {
        print_version(cfg.personality);
        return 0;
    }

    let stdout = io::stdout();
    let mut writer = stdout.lock();

    let result = match cfg.personality {
        Personality::Lp | Personality::Lpr => run_lp(&cfg, &mut writer),
        Personality::Lpstat | Personality::Lpq => run_lpstat(&cfg, &mut writer),
        Personality::Lprm | Personality::Cancel => run_lprm(&cfg, &mut writer),
    };

    match result {
        Ok(code) => code,
        Err(e) => {
            let name = match cfg.personality {
                Personality::Lp => "lp",
                Personality::Lpstat => "lpstat",
                Personality::Lprm => "lprm",
                Personality::Cancel => "cancel",
                Personality::Lpr => "lpr",
                Personality::Lpq => "lpq",
            };
            eprintln!("{name}: {e}");
            1
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_personality() {
        assert_eq!(detect_personality("lp"), Personality::Lp);
        assert_eq!(detect_personality("lpstat"), Personality::Lpstat);
        assert_eq!(detect_personality("lprm"), Personality::Lprm);
        assert_eq!(detect_personality("cancel"), Personality::Cancel);
        assert_eq!(detect_personality("lpr"), Personality::Lpr);
        assert_eq!(detect_personality("lpq"), Personality::Lpq);
    }

    #[test]
    fn test_parse_args_lp_basic() {
        let args = vec!["lp".to_string(), "file.txt".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.personality, Personality::Lp);
        assert_eq!(cfg.files.len(), 1);
    }

    #[test]
    fn test_parse_args_lp_printer() {
        let args = vec![
            "lp".to_string(),
            "-d".to_string(),
            "printer1".to_string(),
            "file.txt".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.printer, Some("printer1".to_string()));
    }

    #[test]
    fn test_parse_args_lp_copies() {
        let args = vec![
            "lp".to_string(),
            "-n".to_string(),
            "3".to_string(),
            "file.txt".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.copies, 3);
    }

    #[test]
    fn test_parse_args_lp_title() {
        let args = vec![
            "lp".to_string(),
            "-t".to_string(),
            "My Document".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.title, Some("My Document".to_string()));
    }

    #[test]
    fn test_parse_args_lp_stdin() {
        let args = vec!["lp".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.files, vec![PathBuf::from("-")]);
    }

    #[test]
    fn test_parse_args_lpstat_all() {
        let args = vec!["lpstat".to_string(), "-t".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_all);
    }

    #[test]
    fn test_parse_args_lpstat_printers() {
        let args = vec!["lpstat".to_string(), "-p".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_printers);
    }

    #[test]
    fn test_parse_args_lpstat_jobs() {
        let args = vec!["lpstat".to_string(), "-o".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_jobs);
    }

    #[test]
    fn test_parse_args_lprm_all() {
        let args = vec!["lprm".to_string(), "-a".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.cancel_all);
    }

    #[test]
    fn test_parse_args_lprm_ids() {
        let args = vec![
            "lprm".to_string(),
            "1".to_string(),
            "2".to_string(),
            "3".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.job_ids, vec![1, 2, 3]);
    }

    #[test]
    fn test_parse_args_help() {
        for name in &["lp", "lpstat", "lprm"] {
            let args = vec![name.to_string(), "--help".to_string()];
            let cfg = parse_args(&args).unwrap();
            assert!(cfg.show_help);
        }
    }

    #[test]
    fn test_job_status_str() {
        assert_eq!(JobStatus::Pending.as_str(), "pending");
        assert_eq!(JobStatus::Processing.as_str(), "processing");
        assert_eq!(JobStatus::Completed.as_str(), "completed");
    }

    #[test]
    fn test_printer_state_str() {
        assert_eq!(PrinterState::Idle.as_str(), "idle");
        assert_eq!(PrinterState::Processing.as_str(), "processing");
        assert_eq!(PrinterState::Stopped.as_str(), "stopped");
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(512), "512");
        assert_eq!(format_size(2048), "2K");
        assert_eq!(format_size(1048576), "1M");
    }

    #[test]
    fn test_read_printers_default() {
        let printers = read_printers();
        assert!(!printers.is_empty());
        assert!(printers[0].is_default);
    }

    #[test]
    fn test_parse_job_file() {
        let content = "id=1\nuser=root\ntitle=test\nprinter=default\nsize=1024\ncopies=2\nstatus=pending\n";
        let job = parse_job_file(content).unwrap();
        assert_eq!(job.job_id, 1);
        assert_eq!(job.username, "root");
        assert_eq!(job.title, "test");
        assert_eq!(job.copies, 2);
        assert_eq!(job.status, JobStatus::Pending);
    }

    #[test]
    fn test_parse_job_file_invalid() {
        assert!(parse_job_file("bad data").is_none());
    }

    #[test]
    fn test_run_lpstat_default() {
        let cfg = Config {
            personality: Personality::Lpstat,
            ..Default::default()
        };
        let mut buf = Vec::new();
        run_lpstat(&cfg, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("default"));
    }

    #[test]
    fn test_run_lpstat_printers() {
        let cfg = Config {
            personality: Personality::Lpstat,
            show_printers: true,
            ..Default::default()
        };
        let mut buf = Vec::new();
        run_lpstat(&cfg, &mut buf).unwrap();
    }

    #[test]
    fn test_run_lpstat_all() {
        let cfg = Config {
            personality: Personality::Lpstat,
            show_all: true,
            ..Default::default()
        };
        let mut buf = Vec::new();
        run_lpstat(&cfg, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("scheduler"));
    }

    #[test]
    fn test_default_config() {
        let cfg = Config::default();
        assert_eq!(cfg.copies, 1);
        assert_eq!(cfg.priority, 50);
        assert!(cfg.files.is_empty());
    }
}
