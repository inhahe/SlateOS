//! SlateOS Service Management CLI
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

use std::env;
use std::fs;
use std::process;

// ============================================================================
// Syscall interface
// ============================================================================

// Service manager IPC syscalls (from kernel syscall table).
// The service manager listens on a well-known IPC channel. We send structured
// commands and receive responses.
const SYS_CHANNEL_OPEN: u64 = 200;
const SYS_CHANNEL_SEND: u64 = 201;
const SYS_CHANNEL_RECV: u64 = 202;
const SYS_CHANNEL_CLOSE: u64 = 204;

#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller ensures arguments are valid for the given syscall.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Send a command to the service manager via IPC.
fn send_service_command(command: &str, service_name: &str) -> Result<String, String> {
    // Build the IPC message: "COMMAND service_name\n"
    let msg = format!("{command} {service_name}\0");

    // Open a channel to the service manager (well-known name: "org.slateos.ServiceManager").
    let svc_name = b"org.slateos.ServiceManager\0";
    let channel = unsafe {
        syscall3(
            SYS_CHANNEL_OPEN,
            svc_name.as_ptr() as u64,
            svc_name.len() as u64,
            0,
        )
    };

    if channel < 0 {
        return Err(format!(
            "cannot connect to service manager (error {channel})"
        ));
    }

    let ch = channel as u64;

    // Send the command.
    let send_ret = unsafe { syscall3(SYS_CHANNEL_SEND, ch, msg.as_ptr() as u64, msg.len() as u64) };

    if send_ret < 0 {
        let _ = unsafe { syscall3(SYS_CHANNEL_CLOSE, ch, 0, 0) };
        return Err(format!("send failed (error {send_ret})"));
    }

    // Receive the response.
    let mut buf = [0u8; 4096];
    let recv_ret = unsafe {
        syscall3(
            SYS_CHANNEL_RECV,
            ch,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        )
    };

    let _ = unsafe { syscall3(SYS_CHANNEL_CLOSE, ch, 0, 0) };

    if recv_ret < 0 {
        return Err(format!("recv failed (error {recv_ret})"));
    }

    let len = (recv_ret as usize).min(buf.len());
    let response = String::from_utf8_lossy(&buf[..len]).to_string();
    Ok(response)
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
///   /etc/services/<name>.service    — YAML service definition
fn read_service_status(name: &str) -> Option<ServiceInfo> {
    let run_path = format!("/run/services/{name}");
    let def_path = format!("/etc/services/{name}.service");

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

/// List all known services by scanning /etc/services/ and /run/services/.
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

    // Scan /etc/services/ for defined but not running services.
    if let Ok(entries) = fs::read_dir("/etc/services") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                let svc_name = name.strip_suffix(".service").unwrap_or(name);
                if !seen.contains(&svc_name.to_string()) {
                    let (desc, deps, exec, enabled) =
                        read_service_def(&format!("/etc/services/{name}"));
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
        println!("Service definitions go in /etc/services/<name>.service");
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
            eprintln!("Service '{}' not found", name);
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

fn cmd_start(name: &str) {
    print!("Starting {}... ", name);
    match send_service_command("START", name) {
        Ok(resp) => println!("{}", resp.trim()),
        Err(e) => {
            // Fall back to direct execution if service manager is unavailable.
            eprintln!("IPC failed ({e}), trying direct start...");
            let def_path = format!("/etc/services/{name}.service");
            let (_, _, exec_path, _) = read_service_def(&def_path);
            if exec_path.is_empty() {
                eprintln!("No exec path found for service '{name}'");
                process::exit(1);
            }
            // The actual process spawn would use SYS_SPAWN here.
            println!("Would execute: {exec_path}");
            println!("(service manager not available for proper lifecycle management)");
        }
    }
}

fn cmd_stop(name: &str) {
    print!("Stopping {}... ", name);
    match send_service_command("STOP", name) {
        Ok(resp) => println!("{}", resp.trim()),
        Err(e) => {
            eprintln!("failed: {e}");
            process::exit(1);
        }
    }
}

fn cmd_restart(name: &str) {
    print!("Restarting {}... ", name);
    match send_service_command("RESTART", name) {
        Ok(resp) => println!("{}", resp.trim()),
        Err(e) => {
            eprintln!("failed: {e}");
            process::exit(1);
        }
    }
}

fn cmd_enable(name: &str) {
    match send_service_command("ENABLE", name) {
        Ok(resp) => println!("{}", resp.trim()),
        Err(_) => {
            // Fall back: create a symlink in /etc/services/enabled/
            let link = format!("/etc/services/enabled/{name}");
            let target = format!("/etc/services/{name}.service");
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
    match send_service_command("DISABLE", name) {
        Ok(resp) => println!("{}", resp.trim()),
        Err(_) => {
            // Fall back: remove the symlink.
            let link = format!("/etc/services/enabled/{name}");
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
        println!("No log entries found for service '{name}'");
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
    println!("SlateOS Service Manager v0.1.0");
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
            print!("Reloading {}... ", &args[2]);
            match send_service_command("RELOAD", &args[2]) {
                Ok(resp) => println!("{}", resp.trim()),
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
