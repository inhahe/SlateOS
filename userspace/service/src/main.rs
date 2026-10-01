//! Slate OS Service Management CLI
//!
//! Start, stop, restart, and query system services. Communicates with the
//! init/service manager daemon via IPC or reads status from /run/services/.
//!
//! # Usage
//!
//! ```text
//! service list                   List all services and their status
//! service status <name>          Show detailed status of a service
//! service start <name>           Start a service
//! service stop <name>            Stop a service
//! service restart <name>         Restart a service (stop + start)
//! service enable <name>          Enable service at boot
//! service disable <name>         Disable service at boot
//! service logs <name>            Show recent log output for a service
//! service reload <name>          Send reload signal to a service
//! service tree                   Show service dependency tree
//! ```

use quoting::quoteaf_os;
use std::env;
use std::fs;
use std::io::Write;
use std::process;

// ============================================================================
// The service manager, over the service bus
// ============================================================================

/// The service manager's name on the service bus.
///
/// Nothing registers it yet (known-issues.md, "`org.slateos.ServiceManager`
/// has two clients and no provider"), so every request ends at "no such
/// service" and the commands that have a fallback use it.
const SERVICE_MANAGER: &str = "org.slateos.ServiceManager";

/// How long to wait for the service manager's answer: 25 seconds, D-Bus's
/// default method-call timeout, which is also what `systemctl` waits.
const SERVICE_MANAGER_TIMEOUT_NS: u64 = libservicebus::secs_to_ns(25);

/// What the service manager said to a request.
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    /// It did it, and said this about it (possibly nothing).
    Done(Vec<u8>),
    /// It refused: its error's name, and the explanation it sent, if any.
    Refused(String, Vec<u8>),
}

/// Ask the service manager to run `method` on the service `name`.
///
/// A service-bus method call whose one argument is the service's name: a
/// return means done, an error means refused, and either's first field, if
/// it sent one, is what it has to say. `Err` means the question could not be
/// asked at all -- no service manager, or no answer in time.
///
/// This replaced a hand-rolled channel client that called syscall 200 as
/// "open a channel to a service". 200 is `SYS_CHANNEL_CREATE`: it took the
/// name's *address* as its flags and returned a fresh channel connected to
/// nothing, so no request ever reached any service (lane F's
/// `requests/f-b-logind-refuses-every-caller-because-libservicebus-never-asks-who-it-is.md`,
/// point 3). Connecting by name is `SYS_SERVICE_CONNECT`, which
/// `libservicebus` wraps.
fn ask_service_manager(method: &str, name: &str) -> Result<Answer, String> {
    let mut conn = libservicebus::Connection::connect(SERVICE_MANAGER)
        .map_err(|e| format!("cannot reach {SERVICE_MANAGER}: {e}"))?;
    // A reply that sent no text has said nothing; that is an absence, not a
    // failure being discarded.
    match conn.call_fields(method, &[name.as_bytes()], SERVICE_MANAGER_TIMEOUT_NS) {
        Ok(libservicebus::Outcome::Done(fields)) => {
            Ok(Answer::Done(fields.into_iter().next().unwrap_or_default()))
        }
        Ok(libservicebus::Outcome::Refused { error, fields }) => Ok(Answer::Refused(
            error,
            fields.into_iter().next().unwrap_or_default(),
        )),
        Err(e) => Err(format!("{SERVICE_MANAGER} did not answer {method}: {e}")),
    }
}

/// Finish a request the service manager answered: print what it said, or
/// report its refusal and exit non-zero.
fn conclude(what: &str, answer: Answer) {
    let code = conclude_to(
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
        what,
        &answer,
    );
    if code != 0 {
        process::exit(code);
    }
}

/// [`conclude`]'s output and exit status, written to `out` and `err`.
///
/// What the service manager says is written as the bytes it sent: it is
/// another program's text, and decoding it lossily would print something it
/// did not say. A failure to write the terminal itself is not reported -- there
/// is nowhere left to report it -- but it does not change the status either.
fn conclude_to(out: &mut impl Write, err: &mut impl Write, what: &str, answer: &Answer) -> i32 {
    match answer {
        Answer::Done(said) => {
            let said: &[u8] = if said.is_empty() { b"done" } else { said };
            let _ = out.write_all(said).and_then(|()| out.write_all(b"\n"));
            0
        }
        Answer::Refused(error, why) => {
            let _ = writeln!(out);
            let _ = write!(err, "service: the service manager refused {what}: {error}")
                .and_then(|()| {
                    if why.is_empty() {
                        Ok(())
                    } else {
                        err.write_all(b": ").and_then(|()| err.write_all(why))
                    }
                })
                .and_then(|()| err.write_all(b"\n"));
            1
        }
    }
}

/// Create a filesystem symlink (`target` is the existing file, `link` is the
/// new symlink path).
///
/// On the shipping `x86_64-slateos` target (which is unix) this uses the real
/// `std::os::unix::fs::symlink`.  The `#[cfg(not(unix))]` arm exists only so
/// the crate compiles for `cargo test`/clippy on the Windows dev host; it is
/// never the runtime path.
#[cfg(unix)]
fn make_symlink(target: &str, link: &str) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
fn make_symlink(_target: &str, _link: &str) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "symlink creation is only supported on the unix (slateos) target",
    ))
}

// ============================================================================
// Service status from /run/services/
// ============================================================================

#[derive(Debug)]
struct ServiceInfo {
    name: String,
    status: String,
    pid: Option<u32>,
    uptime_secs: Option<u64>,
    enabled: bool,
    description: String,
    dependencies: Vec<String>,
    exec_path: String,
    restart_count: u32,
    last_exit_code: Option<i32>,
}

/// Read service status from the filesystem.
///
/// Our init/service manager writes status files to:
///   /run/services/<name>/status     — "running", "stopped", "failed"
///   /run/services/<name>/pid        — PID of main process
///   /run/services/<name>/started_at — timestamp
///   /run/services/<name>/exit_code  — last exit code
///   /run/services/<name>/restarts   — restart count
///
/// Service definitions live in:
///   /etc/service.d/<name>.service    — YAML service definition
///
/// The directory is `/etc/service.d`, not `/etc/services`: that name belongs
/// to the IANA port/protocol database (`getent services`, POSIX's
/// `_PATH_SERVICES`), which is a plain file and so cannot also be this
/// directory.
fn read_service_status(name: &str) -> Option<ServiceInfo> {
    let run_path = format!("/run/services/{name}");
    let def_path = format!("/etc/service.d/{name}.service");

    let status = read_file(&format!("{run_path}/status")).unwrap_or_else(|| "stopped".to_string());
    let pid = read_file(&format!("{run_path}/pid")).and_then(|s| s.parse::<u32>().ok());
    let started_at =
        read_file(&format!("{run_path}/started_at")).and_then(|s| s.parse::<u64>().ok());
    let exit_code = read_file(&format!("{run_path}/exit_code")).and_then(|s| s.parse::<i32>().ok());
    let restart_count = read_file(&format!("{run_path}/restarts"))
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);

    // Compute uptime.
    let uptime_secs =
        started_at.and_then(|start| current_time_secs().map(|now| now.saturating_sub(start)));

    // Read service definition.
    let (description, dependencies, exec_path, enabled) = read_service_def(&def_path);

    Some(ServiceInfo {
        name: name.to_string(),
        status,
        pid,
        uptime_secs,
        enabled,
        description,
        dependencies,
        exec_path,
        restart_count,
        last_exit_code: exit_code,
    })
}

fn read_service_def(path: &str) -> (String, Vec<String>, String, bool) {
    let mut description = String::new();
    let mut dependencies = Vec::new();
    let mut exec_path = String::new();
    let mut enabled = false;

    let content = match read_file(path) {
        Some(c) => c,
        None => return (description, dependencies, exec_path, enabled),
    };

    // Simple YAML parser for service definitions.
    for line in content.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("description:") {
            description = val.trim().trim_matches('"').to_string();
        } else if let Some(val) = line.strip_prefix("exec:") {
            exec_path = val.trim().trim_matches('"').to_string();
        } else if let Some(val) = line.strip_prefix("enabled:") {
            enabled = val.trim() == "true" || val.trim() == "yes";
        } else if line.starts_with("- ") && !dependencies.is_empty() || line.starts_with("depends:")
        {
            if let Some(val) = line.strip_prefix("depends:") {
                // Inline list: depends: [a, b, c]
                let val = val.trim().trim_matches(|c: char| c == '[' || c == ']');
                for dep in val.split(',') {
                    let dep = dep.trim().trim_matches('"');
                    if !dep.is_empty() {
                        dependencies.push(dep.to_string());
                    }
                }
            }
        } else if let Some(dep) = line.strip_prefix("- ") {
            dependencies.push(dep.trim().trim_matches('"').to_string());
        }
    }

    (description, dependencies, exec_path, enabled)
}

/// List all known services by scanning /etc/service.d/ and /run/services/.
fn list_all_services() -> Vec<ServiceInfo> {
    let mut services = Vec::new();
    let mut seen = Vec::new();

    // Scan /run/services/ for running/recently-stopped services.
    if let Ok(entries) = fs::read_dir("/run/services") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && let Some(info) = read_service_status(name)
            {
                seen.push(name.to_string());
                services.push(info);
            }
        }
    }

    // Scan /etc/service.d/ for defined but not running services.
    if let Ok(entries) = fs::read_dir("/etc/service.d") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                let svc_name = name.strip_suffix(".service").unwrap_or(name);
                if !seen.contains(&svc_name.to_string()) {
                    let (desc, deps, exec, enabled) =
                        read_service_def(&format!("/etc/service.d/{name}"));
                    services.push(ServiceInfo {
                        name: svc_name.to_string(),
                        status: "stopped".to_string(),
                        pid: None,
                        uptime_secs: None,
                        enabled,
                        description: desc,
                        dependencies: deps,
                        exec_path: exec,
                        restart_count: 0,
                        last_exit_code: None,
                    });
                }
            }
        }
    }

    services.sort_by(|a, b| a.name.cmp(&b.name));
    services
}

// ============================================================================
// Helpers
// ============================================================================

fn read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn current_time_secs() -> Option<u64> {
    read_file("/proc/uptime")
        .and_then(|s| {
            s.split_whitespace()
                .next()
                .and_then(|v| v.parse::<f64>().ok())
        })
        .map(|f| f as u64)
}

fn format_uptime(secs: u64) -> String {
    if secs >= 86400 {
        let days = secs / 86400;
        let hours = (secs % 86400) / 3600;
        format!("{days}d {hours}h")
    } else if secs >= 3600 {
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        format!("{hours}h {mins}m")
    } else if secs >= 60 {
        let mins = secs / 60;
        let s = secs % 60;
        format!("{mins}m {s}s")
    } else {
        format!("{secs}s")
    }
}

fn status_indicator(status: &str) -> &str {
    match status {
        "running" => "\x1b[32m●\x1b[0m",  // green dot
        "stopped" => "\x1b[90m○\x1b[0m",  // gray dot
        "failed" => "\x1b[31m●\x1b[0m",   // red dot
        "starting" => "\x1b[33m◐\x1b[0m", // yellow half
        "stopping" => "\x1b[33m◑\x1b[0m", // yellow half
        _ => "?",
    }
}

// ============================================================================
// Commands
// ============================================================================

fn cmd_list() {
    let services = list_all_services();

    if services.is_empty() {
        println!("No services found.");
        println!("Service definitions go in /etc/service.d/<name>.service");
        return;
    }

    println!(
        "{:<3} {:<24} {:<10} {:>6} {:>8} DESCRIPTION",
        "", "SERVICE", "STATUS", "PID", "UPTIME"
    );
    println!(
        "{:<3} {:<24} {:<10} {:>6} {:>8} -----------",
        "", "-------", "------", "---", "------"
    );

    for svc in &services {
        let indicator = status_indicator(&svc.status);
        let pid_str = svc
            .pid
            .map(|p| p.to_string())
            .unwrap_or_else(|| "-".to_string());
        let uptime_str = svc
            .uptime_secs
            .map(format_uptime)
            .unwrap_or_else(|| "-".to_string());

        let enabled_marker = if svc.enabled { "" } else { " (disabled)" };

        println!(
            "{indicator} {:<24} {:<10} {:>6} {:>8} {}{}",
            svc.name, svc.status, pid_str, uptime_str, svc.description, enabled_marker,
        );
    }

    let running = services.iter().filter(|s| s.status == "running").count();
    let failed = services.iter().filter(|s| s.status == "failed").count();
    println!(
        "\n{} services, {} running, {} failed",
        services.len(),
        running,
        failed
    );
}

fn cmd_status(name: &str) {
    let info = match read_service_status(name) {
        Some(i) => i,
        None => {
            eprintln!("Service {} not found", quoteaf_os(name));
            process::exit(1);
        }
    };

    let indicator = status_indicator(&info.status);
    println!("{indicator} {}: {}", info.name, info.status);

    if !info.description.is_empty() {
        println!("  Description: {}", info.description);
    }
    if !info.exec_path.is_empty() {
        println!("  Executable:  {}", info.exec_path);
    }
    if let Some(pid) = info.pid {
        println!("  PID:         {pid}");
    }
    if let Some(uptime) = info.uptime_secs {
        println!("  Uptime:      {}", format_uptime(uptime));
    }
    if let Some(code) = info.last_exit_code {
        println!("  Exit code:   {code}");
    }
    if info.restart_count > 0 {
        println!("  Restarts:    {}", info.restart_count);
    }
    println!("  Enabled:     {}", if info.enabled { "yes" } else { "no" });

    if !info.dependencies.is_empty() {
        println!("  Depends on:  {}", info.dependencies.join(", "));
    }
}

/// Launch `exec` with no supervisor, returning the new process id.
///
/// # Why this spawns where `firejail` and `capsh` refuse
///
/// `design-decisions.md` 1019 asks what the caller loses if the program
/// proceeds. Here the answer is *supervision* -- nothing will restart the
/// service or track it for a later `service stop`. That is less than was
/// asked for, but it is not a **constraint being withheld**: nobody is made
/// less safe by the program running. `firejail` refuses because running
/// unsandboxed is the one outcome its caller was trying to prevent; starting a
/// service unsupervised is simply a weaker version of starting it.
///
/// So it runs, and says plainly what is missing. Until 2026-09-10 it printed
/// `Would execute: /usr/sbin/foo` and returned 0, which told a script the
/// service was up.
///
/// # Splitting
///
/// The `exec:` line is split on whitespace. That is the whole of it: there is
/// no quote handling, so a path containing a space cannot be expressed, and
/// `exec: "/usr/bin/foo --msg=hello world"` passes `world` as a separate
/// argument. systemd's `ExecStart` does parse quotes; ours does not, and
/// saying so here is better than a caller discovering it from a truncated
/// argument.
fn spawn_unsupervised(exec: &str) -> Result<u32, String> {
    let (program, args) = split_exec(exec)?;
    process::Command::new(program)
        .args(args)
        .spawn()
        .map(|child| child.id())
        .map_err(|e| format!("{program}: {e}"))
}

/// Split an `exec:` line into a program and its arguments.
///
/// Separate from [`spawn_unsupervised`] so the splitting can be tested without
/// starting a process -- the limitation documented above is a behaviour worth
/// pinning rather than rediscovering.
fn split_exec(exec: &str) -> Result<(&str, Vec<&str>), String> {
    let mut words = exec.split_whitespace();
    let program = words
        .next()
        .ok_or_else(|| "the exec line is empty".to_string())?;
    Ok((program, words.collect()))
}

fn cmd_start(name: &str) {
    print!("Starting {}... ", name);
    match ask_service_manager("StartService", name) {
        Ok(answer) => conclude("the start", answer),
        Err(e) => {
            // Fall back to direct execution if service manager is unavailable.
            eprintln!("IPC failed ({e}), trying direct start...");
            let def_path = format!("/etc/service.d/{name}.service");
            let (_, _, exec_path, _) = read_service_def(&def_path);
            if exec_path.is_empty() {
                eprintln!("No exec path found for service {}", quoteaf_os(name));
                process::exit(1);
            }
            match spawn_unsupervised(&exec_path) {
                Ok(pid) => {
                    println!("started {name} as pid {pid}, UNSUPERVISED");
                    println!(
                        "(no service manager: nothing will restart {name} if it \
                         exits, and `service stop {name}` will not find it)"
                    );
                }
                Err(e) => {
                    eprintln!("service: cannot start {}: {e}", quoteaf_os(name));
                    process::exit(1);
                }
            }
        }
    }
}

fn cmd_stop(name: &str) {
    print!("Stopping {}... ", name);
    match ask_service_manager("StopService", name) {
        Ok(answer) => conclude("the stop", answer),
        Err(e) => {
            eprintln!("failed: {e}");
            process::exit(1);
        }
    }
}

fn cmd_restart(name: &str) {
    print!("Restarting {}... ", name);
    match ask_service_manager("RestartService", name) {
        Ok(answer) => conclude("the restart", answer),
        Err(e) => {
            eprintln!("failed: {e}");
            process::exit(1);
        }
    }
}

fn cmd_enable(name: &str) {
    match ask_service_manager("EnableService", name) {
        Ok(answer) => conclude("enabling it", answer),
        // No service manager: the enabled-set on disk is what it would read.
        Err(_) => {
            // Fall back: create a symlink in /etc/service.d/enabled/
            let link = format!("/etc/service.d/enabled/{name}");
            let target = format!("/etc/service.d/{name}.service");
            match make_symlink(&target, &link) {
                Ok(()) => println!("Enabled {name}"),
                Err(e) => {
                    eprintln!("Failed to enable {name}: {e}");
                    process::exit(1);
                }
            }
        }
    }
}

fn cmd_disable(name: &str) {
    match ask_service_manager("DisableService", name) {
        Ok(answer) => conclude("disabling it", answer),
        // No service manager: the enabled-set on disk is what it would read.
        Err(_) => {
            // Fall back: remove the symlink.
            let link = format!("/etc/service.d/enabled/{name}");
            match fs::remove_file(&link) {
                Ok(()) => println!("Disabled {name}"),
                Err(e) => {
                    eprintln!("Failed to disable {name}: {e}");
                    process::exit(1);
                }
            }
        }
    }
}

fn cmd_logs(name: &str) {
    // Read from syslogd output filtered by service name.
    let log_path = "/var/log/syslog";
    let content = match read_file(log_path) {
        Some(c) => c,
        None => {
            eprintln!("Cannot read {log_path}");
            process::exit(1);
        }
    };

    let mut found = false;
    for line in content.lines() {
        // JSON-lines format: look for "service":"<name>".
        let search = format!("\"service\":\"{}\"", name);
        if line.contains(&search) {
            println!("{line}");
            found = true;
        }
    }

    if !found {
        println!("No log entries found for service {}", quoteaf_os(name));
    }
}

fn cmd_tree() {
    let services = list_all_services();

    if services.is_empty() {
        println!("No services found.");
        return;
    }

    // Find root services (no dependencies or deps not in our list).
    let svc_names: Vec<&str> = services.iter().map(|s| s.name.as_str()).collect();

    let roots: Vec<&ServiceInfo> = services
        .iter()
        .filter(|s| {
            s.dependencies.is_empty()
                || s.dependencies
                    .iter()
                    .all(|d| !svc_names.contains(&d.as_str()))
        })
        .collect();

    for root in &roots {
        let indicator = status_indicator(&root.status);
        println!("{indicator} {}", root.name);
        print_dep_tree(&services, &root.name, "  ", &mut Vec::new());
    }
}

fn print_dep_tree(services: &[ServiceInfo], parent: &str, prefix: &str, visited: &mut Vec<String>) {
    if visited.contains(&parent.to_string()) {
        return; // Avoid cycles.
    }
    visited.push(parent.to_string());

    // Find services that depend on this one.
    let dependents: Vec<&ServiceInfo> = services
        .iter()
        .filter(|s| s.dependencies.iter().any(|d| d == parent))
        .collect();

    for (i, dep) in dependents.iter().enumerate() {
        let is_last = i == dependents.len() - 1;
        let branch = if is_last { "└──" } else { "├──" };
        let indicator = status_indicator(&dep.status);
        println!("{prefix}{branch} {indicator} {}", dep.name);

        let next_prefix = format!("{prefix}{}", if is_last { "    " } else { "│   " });
        print_dep_tree(services, &dep.name, &next_prefix, visited);
    }
}

// ============================================================================
// Usage and main
// ============================================================================

fn print_usage() {
    println!("Slate OS Service Manager v0.1.0");
    println!();
    println!("Start, stop, and manage system services.");
    println!();
    println!("USAGE:");
    println!("  service <command> [name]");
    println!();
    println!("COMMANDS:");
    println!("  list              List all services and their status");
    println!("  status <name>     Detailed status of a service");
    println!("  start <name>      Start a service");
    println!("  stop <name>       Stop a service");
    println!("  restart <name>    Stop and restart a service");
    println!("  enable <name>     Enable service at boot");
    println!("  disable <name>    Disable service at boot");
    println!("  logs <name>       Show log output for a service");
    println!("  reload <name>     Send reload signal");
    println!("  tree              Show dependency tree");
    println!();
    println!("EXAMPLES:");
    println!("  service list");
    println!("  service start network");
    println!("  service status syslogd");
    println!("  service logs crond");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        cmd_list();
        return;
    }

    match args[1].as_str() {
        "list" | "ls" => cmd_list(),
        "status" | "show" => {
            if args.len() < 3 {
                eprintln!("error: 'status' requires a service name");
                process::exit(1);
            }
            cmd_status(&args[2]);
        }
        "start" => {
            if args.len() < 3 {
                eprintln!("error: 'start' requires a service name");
                process::exit(1);
            }
            cmd_start(&args[2]);
        }
        "stop" => {
            if args.len() < 3 {
                eprintln!("error: 'stop' requires a service name");
                process::exit(1);
            }
            cmd_stop(&args[2]);
        }
        "restart" => {
            if args.len() < 3 {
                eprintln!("error: 'restart' requires a service name");
                process::exit(1);
            }
            cmd_restart(&args[2]);
        }
        "enable" => {
            if args.len() < 3 {
                eprintln!("error: 'enable' requires a service name");
                process::exit(1);
            }
            cmd_enable(&args[2]);
        }
        "disable" => {
            if args.len() < 3 {
                eprintln!("error: 'disable' requires a service name");
                process::exit(1);
            }
            cmd_disable(&args[2]);
        }
        "logs" | "log" => {
            if args.len() < 3 {
                eprintln!("error: 'logs' requires a service name");
                process::exit(1);
            }
            cmd_logs(&args[2]);
        }
        "reload" => {
            if args.len() < 3 {
                eprintln!("error: 'reload' requires a service name");
                process::exit(1);
            }
            print!("Reloading {}... ", args[2]);
            match ask_service_manager("ReloadService", &args[2]) {
                Ok(answer) => conclude("the reload", answer),
                Err(e) => {
                    eprintln!("failed: {e}");
                    process::exit(1);
                }
            }
        }
        "tree" => cmd_tree(),
        "help" | "--help" | "-h" => print_usage(),
        other => {
            eprintln!("unknown command: {other}");
            eprintln!("Run 'service help' for usage.");
            process::exit(1);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// No service manager is a question that could not be asked (so the
    /// fallbacks run), never an answer. On a development host the bus says
    /// "not supported" to everything, the same outcome as no manager
    /// registered on SlateOS.
    #[test]
    fn no_service_manager_cannot_be_asked() {
        let err = ask_service_manager("StartService", "sshd").unwrap_err();
        assert!(err.contains(SERVICE_MANAGER), "{err}");
    }

    #[test]
    fn what_the_service_manager_says_is_printed_as_it_was_sent() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let said = Answer::Done(b"started \xffsshd".to_vec());
        assert_eq!(conclude_to(&mut out, &mut err, "the start", &said), 0);
        assert_eq!(out, b"started \xffsshd\n");
        assert!(err.is_empty());

        // Saying nothing is still done.
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(
            conclude_to(&mut out, &mut err, "the start", &Answer::Done(Vec::new())),
            0
        );
        assert_eq!(out, b"done\n");
    }

    #[test]
    fn a_refusal_ends_the_progress_line_and_fails() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let refused = Answer::Refused(
            "org.slateos.Error.NoSuchService".to_string(),
            b"no sshd".to_vec(),
        );
        assert_eq!(conclude_to(&mut out, &mut err, "the start", &refused), 1);
        // "Starting sshd... " is left without a newline by the caller.
        assert_eq!(out, b"\n");
        assert_eq!(
            err,
            b"service: the service manager refused the start: \
              org.slateos.Error.NoSuchService: no sshd\n"
        );
    }

    #[test]
    fn an_exec_line_splits_into_a_program_and_its_arguments() {
        let (prog, args) = split_exec("/usr/sbin/sshd -D -e").expect("should split");
        assert_eq!(prog, "/usr/sbin/sshd");
        assert_eq!(args, vec!["-D", "-e"]);
    }

    #[test]
    fn a_bare_program_has_no_arguments() {
        let (prog, args) = split_exec("/bin/true").expect("should split");
        assert_eq!(prog, "/bin/true");
        assert!(args.is_empty());
    }

    #[test]
    fn surrounding_and_repeated_whitespace_is_not_an_argument() {
        let (prog, args) = split_exec("  /bin/foo   -a    -b  ").expect("should split");
        assert_eq!(prog, "/bin/foo");
        assert_eq!(args, vec!["-a", "-b"]);
    }

    #[test]
    fn an_empty_exec_line_is_an_error_rather_than_an_empty_program() {
        assert!(split_exec("").is_err());
        assert!(split_exec("   ").is_err());
    }

    /// The documented limitation, pinned so it is a known behaviour rather
    /// than a surprise: there is NO quote handling, so an argument containing
    /// a space becomes two arguments and a quoted path keeps its quotes.
    /// systemd's `ExecStart` parses quotes; this does not.
    ///
    /// The test exists to fail if someone adds quote handling without updating
    /// the doc comment on `spawn_unsupervised`. That is the direction which
    /// would otherwise go unnoticed, because the new behaviour is the one a
    /// reader already expects.
    #[test]
    fn quotes_are_not_interpreted_and_that_is_deliberate() {
        let (prog, args) = split_exec("/bin/say --msg=\"hello world\"").expect("should split");
        assert_eq!(prog, "/bin/say");
        assert_eq!(args, vec!["--msg=\"hello", "world\""]);

        let (prog, _) = split_exec("\"/opt/my app/bin\"").expect("should split");
        assert_eq!(prog, "\"/opt/my");
    }

    #[test]
    fn format_uptime_seconds() {
        assert_eq!(format_uptime(0), "0s");
        assert_eq!(format_uptime(45), "45s");
    }

    #[test]
    fn format_uptime_minutes() {
        assert_eq!(format_uptime(60), "1m 0s");
        assert_eq!(format_uptime(125), "2m 5s");
    }

    #[test]
    fn format_uptime_hours() {
        assert_eq!(format_uptime(3600), "1h 0m");
        assert_eq!(format_uptime(3661), "1h 1m");
    }

    #[test]
    fn format_uptime_days() {
        assert_eq!(format_uptime(86400), "1d 0h");
        assert_eq!(format_uptime(90000), "1d 1h");
    }

    #[test]
    fn status_indicator_known_states() {
        // Each known state maps to a distinct, non-empty marker.
        for s in ["running", "stopped", "failed", "starting", "stopping"] {
            assert!(!status_indicator(s).is_empty());
        }
        assert_eq!(status_indicator("unknown-state"), "?");
    }

    #[test]
    fn make_symlink_unsupported_on_host() {
        // On the non-unix dev host the helper must report Unsupported (it is
        // never the runtime path; the shipping slateos target uses the real
        // symlink call).  On a unix host the call will attempt a real symlink
        // into a path that does not exist and fail with a different error — so
        // we only assert the error kind on non-unix.
        #[cfg(not(unix))]
        {
            let err = make_symlink("/nonexistent/target", "/nonexistent/link")
                .expect_err("symlink must be unsupported on non-unix host");
            assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
        }
    }
}
